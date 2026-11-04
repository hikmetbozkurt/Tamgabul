//! One search request end to end (docs/08 retrieval plan), shared by the
//! app and the evaluation runner so both measure the same thing.
//!
//! 1. Understand: rules, plus the optional LLM plan (in parallel with the
//!    first query embedding).
//! 2. "You showed me before" requests look in search history first.
//! 3. Retrieve: vectors + words + names, filtered and re-authorized.
//! 4. Pick: the LLM keeps the candidates that answer the request, may give
//!    a one-sentence answer and writes a cited summary. Without the LLM,
//!    weak tail hits are cut.
//!
//! Models are reached through [`QueryModels`]; this module never talks to
//! a runtime directly and does not record history.

use rusqlite::Connection;
use serde_json::Value;

use crate::dto::{Choice, DirectAnswer, SearchKind, Summary};
use crate::error::IpcError;
use crate::history::{self, SessionContext};
use crate::rerank;
use crate::search::{self, Acl, DocFilter, Hit, Resolved};
use crate::understand::{self, Plan, PlanIntent};

pub const RESULT_LIMIT: usize = 10;
/// Candidates the LLM looks at before picking.
pub const PICK_POOL: usize = rerank::MAX_CANDIDATES;
/// Without an LLM: hide hits weaker than this share of the best score
/// (scores are rank statistics; this is relative, not a calibrated cutoff).
pub const RELATIVE_FLOOR: f64 = 0.5;
const PRIOR_SHOW: usize = 5;
const PLAN_TOKENS: u32 = 320;

/// Models a search may use. Implementations start models on demand and
/// answer `None` when one is missing or fails (the search goes on).
pub trait QueryModels: Sync {
    /// L2-normalized query vector.
    fn embed_query(&self, text: &str) -> Option<Vec<f32>>;
    fn has_llm(&self) -> bool;
    /// One grammar-constrained JSON answer.
    fn llm_json(&self, system: &str, user: &str, schema: &Value, max_tokens: u32) -> Option<Value>;
}

pub struct Query<'a> {
    /// The full request (a picked clarification option is already joined
    /// to the request it answered).
    pub text: &'a str,
    pub ctx: &'a SessionContext,
    pub allow_clarify: bool,
    pub now: i64,
    /// Seconds east of UTC on the user's clock.
    pub utc_offset: i64,
    pub roles: Roles,
}

/// Which LLM jobs a search may use (Settings → Models, and the evaluation
/// runner's on/off comparison). Each is used only when an LLM is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Roles {
    /// Understand the request (intent, words, clarify).
    pub plan: bool,
    /// Pick, answer and summarize from the candidates.
    pub pick: bool,
}

impl Default for Roles {
    fn default() -> Self {
        Roles {
            plan: true,
            pick: true,
        }
    }
}

pub struct Found {
    pub hit: Hit,
    pub resolved: Resolved,
}

pub struct Outcome {
    pub plan: Plan,
    pub kind: SearchKind,
    pub choices: Vec<Choice>,
    pub found: Vec<Found>,
    pub direct: Option<DirectAnswer>,
    pub summary: Option<Summary>,
    pub picked_by_llm: bool,
    /// The first `picked_count` of `found` are the LLM's picks.
    pub picked_count: usize,
    pub notice: Option<String>,
}

fn llm_plan(models: &dyn QueryModels, q: &Query<'_>) -> Option<understand::LlmPlan> {
    let (system, user) = understand::llm_messages(q.text, &q.ctx.prior, &q.ctx.active);
    let v = models.llm_json(&system, &user, &understand::llm_schema(), PLAN_TOKENS)?;
    understand::parse_llm(&v).ok()
}

/// First text of a document (several chunks), for "summarize" requests.
fn doc_text(conn: &Connection, doc: &str, max: usize) -> Option<String> {
    let mut st = conn
        .prepare_cached(
            "SELECT text FROM chunks WHERE document_id = ?1 AND text IS NOT NULL ORDER BY ordinal LIMIT 4",
        )
        .ok()?;
    let parts: Vec<String> = st
        .query_map([doc], |r| r.get(0))
        .ok()?
        .filter_map(Result::ok)
        .collect();
    let mut s = parts.join("\n");
    if let Some((i, _)) = s.char_indices().nth(max) {
        s.truncate(i);
    }
    (!s.is_empty()).then_some(s)
}

fn candidates(conn: &Connection, found: &[Found], mode: rerank::Mode) -> Vec<rerank::Candidate> {
    found
        .iter()
        .take(rerank::MAX_CANDIDATES)
        .enumerate()
        .map(|(i, f)| {
            let r = &f.resolved;
            let long = mode == rerank::Mode::Summarize && i < rerank::SUMMARY_LONG_CANDIDATES;
            let excerpt = if long {
                doc_text(conn, &f.hit.document_id, rerank::SUMMARY_EXCERPT_CHARS)
                    .or_else(|| r.chunk_text.clone())
            } else {
                r.chunk_text.clone()
            };
            rerank::Candidate {
                title: r.card.title.clone(),
                kind: serde_json::to_value(&r.card.file_kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                modified: r.card.modified_at.chars().take(10).collect(),
                excerpt: excerpt.unwrap_or_default(),
            }
        })
        .collect()
}

fn resolve_all(
    conn: &Connection,
    acl: &Acl,
    hits: Vec<Hit>,
    terms: &[String],
) -> Result<Vec<Found>, IpcError> {
    let mut out = Vec::new();
    for h in hits {
        if let Some(r) = search::resolve(
            conn,
            acl,
            &h.document_id,
            h.chunk_id,
            h.reason.as_str(),
            terms,
        )? {
            out.push(Found {
                hit: h,
                resolved: r,
            });
        }
    }
    Ok(out)
}

pub fn run(
    conn: &Connection,
    generation: Option<&str>,
    models: &dyn QueryModels,
    q: &Query<'_>,
) -> Result<Outcome, IpcError> {
    let acl = Acl::load(conn)?;
    let searchable = !acl.is_empty();

    // 1. Understand (the LLM plans while the query is embedded once).
    let rules_plan = understand::plan(q.text, None, q.allow_clarify);
    let use_llm = q.roles.plan && searchable && models.has_llm();
    let (llm_out, mut qv) = std::thread::scope(|s| {
        let h = use_llm.then(|| s.spawn(|| llm_plan(models, q)));
        let v = (searchable && generation.is_some())
            .then(|| models.embed_query(&rules_plan.embed_text))
            .flatten();
        (h.and_then(|h| h.join().ok().flatten()), v)
    });
    let mut plan = match &llm_out {
        Some(l) => understand::plan(q.text, Some(l), q.allow_clarify),
        None => rules_plan.clone(),
    };
    if plan.embed_text != rules_plan.embed_text && generation.is_some() && searchable {
        qv = models.embed_query(&plan.embed_text).or(qv);
    }

    let mut out = Outcome {
        plan: plan.clone(),
        kind: SearchKind::NotFound,
        choices: Vec::new(),
        found: Vec::new(),
        direct: None,
        summary: None,
        picked_by_llm: false,
        picked_count: 0,
        notice: None,
    };

    if plan.intent == PlanIntent::Clarify {
        out.kind = SearchKind::Clarify;
        out.choices = plan
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| Choice {
                id: format!("opt:{i}"),
                label: o.clone(),
                count: 0,
            })
            .collect();
        return Ok(out);
    }

    // 2. Earlier results first for "geçen bana bulmuştun…".
    if plan.intent == PlanIntent::LookupPrior {
        let prior = history::prior_results(conn, &plan.terms, PRIOR_SHOW)?;
        let found = resolve_all(conn, &acl, prior.hits, &plan.terms)?;
        if !found.is_empty() {
            out.kind = SearchKind::Results;
            out.found = found;
            return Ok(out);
        }
        if prior.withheld {
            out.notice = Some("prior_not_accessible".into());
        }
        // Nothing usable in history: search the files instead.
        plan.intent = PlanIntent::Search;
        out.plan.intent = PlanIntent::Search;
    }
    if !searchable {
        return Ok(out);
    }

    // 3. Retrieve.
    let filter = DocFilter::new(&plan.filters, q.now, q.utc_offset);
    let browse = plan.is_browse();
    let pool = if q.roles.pick && models.has_llm() {
        PICK_POOL
    } else {
        RESULT_LIMIT
    };
    let hits = if browse {
        search::browse(conn, &filter, RESULT_LIMIT)?
    } else {
        search::search_terms(conn, generation, qv.as_deref(), &plan.terms, &filter, pool)?
    };
    let mut found = resolve_all(conn, &acl, hits, &plan.terms)?;
    // Nothing of that type / date: rather than "not found", show the best
    // matches without those filters and say so.
    let typed = filter.types.is_some() || filter.window.is_some();
    if found.is_empty() && !browse && typed {
        let loose = DocFilter {
            types: None,
            window: None,
            exclude: filter.exclude.clone(),
        };
        let hits =
            search::search_terms(conn, generation, qv.as_deref(), &plan.terms, &loose, pool)?;
        found = resolve_all(conn, &acl, hits, &plan.terms)?;
        if !found.is_empty() {
            out.notice = Some("filters_relaxed".into());
        }
    }

    // 4. Pick (and summarize).
    let mode = if plan.summarize {
        rerank::Mode::Summarize
    } else {
        rerank::Mode::Pick
    };
    let min = if plan.summarize { 1 } else { 2 };
    let pick_on = q.roles.pick && models.has_llm() && searchable;
    if !browse && found.len() >= min {
        let cands = if pick_on {
            candidates(conn, &found, mode)
        } else {
            Vec::new()
        };
        let pick = if pick_on {
            let (system, user) = rerank::rerank_messages(q.text, &cands, mode);
            models
                .llm_json(
                    &system,
                    &user,
                    &rerank::rerank_schema(cands.len(), mode),
                    mode.max_tokens(),
                )
                .and_then(|v| rerank::parse_rerank(&v, cands.len()))
        } else {
            None
        };
        let best = found.first().map(|f| f.hit.score).unwrap_or(0.0);
        match pick {
            Some(p) if !p.order.is_empty() => {
                let p = rerank::ground(p, q.text, &cands);
                out.picked_by_llm = true;
                out.picked_count = p.order.len();
                if let Some((text, i)) = &p.answer {
                    out.direct = Some(DirectAnswer {
                        text: text.clone(),
                        result_ref: found[*i].resolved.card.result_ref.clone(),
                    });
                }
                // The picks go first; the rest stay visible under them
                // (the LLM may reorder, never hide).
                let mut slots: Vec<Option<Found>> = found.into_iter().map(Some).collect();
                let mut picked: Vec<Found> =
                    p.order.iter().filter_map(|i| slots[*i].take()).collect();
                out.summary = p.summary.map(|text| Summary {
                    text,
                    sources: picked
                        .iter()
                        .map(|f| f.resolved.card.result_ref.clone())
                        .collect(),
                });
                let rest = slots
                    .into_iter()
                    .flatten()
                    .filter(|f| f.hit.score >= best * RELATIVE_FLOOR);
                picked.extend(rest);
                picked.truncate(RESULT_LIMIT.max(out.picked_count));
                found = picked;
            }
            other => {
                if other.is_some() && out.notice.is_none() {
                    out.notice = Some("llm_unsure".into());
                }
                found.retain(|f| f.hit.score >= best * RELATIVE_FLOOR);
                found.truncate(RESULT_LIMIT);
            }
        }
    }
    out.kind = if found.is_empty() {
        SearchKind::NotFound
    } else {
        SearchKind::Results
    };
    out.found = found;
    Ok(out)
}
