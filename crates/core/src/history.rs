//! Conversation history in SQLite (P3 core, docs/06). Results are stored as
//! references (document + chunk), never as copied text, and are rebuilt and
//! re-authorized every time they are shown — a revoked folder's results
//! disappear from old conversations too.
//!
//! With history off, sessions are marked not persisted and removed at the
//! next start.

use rusqlite::{params, Connection, OptionalExtension};

use crate::dto::{
    Choice, DirectAnswer, HistorySession, HistoryTurn, SearchKind, Summary, TurnRole, Understood,
};
use crate::error::{ErrorCode, IpcError};
use crate::index::store::new_id;
use crate::search::{self, Acl, Hit};
use crate::understand::{Filters, Plan, PlanIntent, PriorTurn};

fn err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("history: {e}"))
}

pub fn ensure_session(
    conn: &Connection,
    id: Option<&str>,
    persisted: bool,
) -> Result<String, IpcError> {
    if let Some(id) = id {
        let exists: Option<String> = conn
            .query_row(
                "SELECT session_id FROM sessions WHERE session_id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if exists.is_some() {
            conn.execute(
                "UPDATE sessions SET last_active_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE session_id = ?1",
                [id],
            )
            .map_err(err)?;
            return Ok(id.to_string());
        }
    }
    let id = new_id("c", "session");
    conn.execute(
        "INSERT INTO sessions(session_id, persisted) VALUES (?1, ?2)",
        params![id, persisted as i64],
    )
    .map_err(err)?;
    Ok(id)
}

fn next_ordinal(conn: &Connection, session: &str) -> Result<i64, IpcError> {
    conn.query_row(
        "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM turns WHERE session_id = ?1",
        [session],
        |r| r.get(0),
    )
    .map_err(err)
}

pub struct ShownResult<'a> {
    pub hit: &'a Hit,
    pub scope_id: Option<String>,
    pub title: String,
}

/// The assistant side of one exchange.
pub struct Answer<'a> {
    pub kind: SearchKind,
    pub results: &'a [ShownResult<'a>],
    pub plan: &'a Plan,
    pub choices: &'a [Choice],
    /// Direct answer read from one result (its `result_ref`).
    pub direct: Option<&'a DirectAnswer>,
    pub summary: Option<&'a Summary>,
    pub picked_by_llm: bool,
    pub notice: Option<&'a str>,
}

/// Store one exchange: the user's text with the plan it was understood as,
/// and the assistant answer with its result references. Returns the
/// assistant turn id.
pub fn record_search(
    conn: &mut Connection,
    session: &str,
    user_text: &str,
    answer: &Answer<'_>,
    generation: Option<&str>,
) -> Result<String, IpcError> {
    let plan = answer.plan;
    let results = answer.results;
    let tx = conn.transaction().map_err(err)?;
    let ord = next_ordinal(&tx, session)?;
    let user_turn = new_id("u", session);
    let plan_json = serde_json::to_string(plan).map_err(err)?;
    tx.execute(
        "INSERT INTO turns(turn_id, session_id, ordinal, role, input_mode, text, intent, slots_json)
         VALUES (?1, ?2, ?3, 'user', 'typed', ?4, ?5, ?6)",
        params![user_turn, session, ord, user_text, plan.intent.db_intent(), plan_json],
    )
    .map_err(err)?;
    let ai_turn = new_id("a", session);
    let reasons: Vec<&str> = results.iter().map(|r| r.hit.reason.as_str()).collect();
    let slots = serde_json::json!({
        "kind": answer.kind,
        "count": results.len(),
        "reasons": reasons,
        "question": plan.question,
        "choices": answer.choices,
        "understood": plan.understood(),
        "answer": answer.direct,
        "summary": answer.summary,
        "picked_by_llm": answer.picked_by_llm,
        "notice": answer.notice,
    });
    tx.execute(
        "INSERT INTO turns(turn_id, session_id, ordinal, role, intent, slots_json)
         VALUES (?1, ?2, ?3, 'assistant', ?4, ?5)",
        params![
            ai_turn,
            session,
            ord + 1,
            plan.intent.db_intent(),
            slots.to_string()
        ],
    )
    .map_err(err)?;
    let event = new_id("e", session);
    let event_type = if answer.kind == SearchKind::Clarify {
        "clarified"
    } else if results.is_empty() {
        "search_failed"
    } else {
        "results_shown"
    };
    let filters = serde_json::to_string(&plan.filters).map_err(err)?;
    tx.execute(
        "INSERT INTO search_events(event_id, session_id, turn_id, event_type, query_terms, filters_json, generation_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![event, session, ai_turn, event_type, user_text, filters, generation],
    )
    .map_err(err)?;
    for (rank, r) in results.iter().enumerate() {
        tx.execute(
            "INSERT INTO result_interactions(interaction_id, event_id, document_id, chunk_id, scope_id, rank, score, action, title_snapshot)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'shown', ?8)",
            params![
                new_id("i", &r.hit.document_id),
                event,
                r.hit.document_id,
                r.hit.chunk_id,
                r.scope_id,
                rank as i64,
                r.hit.score,
                r.title
            ],
        )
        .map_err(err)?;
    }
    if answer.kind == SearchKind::Clarify {
        tx.execute(
            "UPDATE sessions SET last_active_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE session_id = ?1",
            [session],
        )
        .map_err(err)?;
    } else {
        // Filters stay active for follow-ups ("sadece PDF'ler").
        tx.execute(
            "UPDATE sessions SET last_active_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'),
                 active_slots_json = ?2 WHERE session_id = ?1",
            params![session, filters],
        )
        .map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(ai_turn)
}

/// Earlier results for "geçen bana bulmuştun…" (docs/08 memory-first
/// retrieval).
#[derive(Debug, Clone, Default)]
pub struct PriorLookup {
    /// Still authorized, best first.
    pub hits: Vec<Hit>,
    /// Matching earlier results exist but none is accessible any more
    /// (folder revoked or file gone). Their names are not revealed.
    pub withheld: bool,
}

const PRIOR_EVENTS: usize = 500;

/// Search history for results shown before: earlier requests and shown
/// file names are matched against `topic` (folded words); without topic
/// words, the most recent results. Every hit is re-authorized.
pub fn prior_results(
    conn: &Connection,
    topic: &[String],
    limit: usize,
) -> Result<PriorLookup, IpcError> {
    use crate::index::text::fold;
    let acl = Acl::load(conn)?;
    let topic: Vec<String> = topic.iter().map(|t| fold(t)).collect();
    let mut st = conn
        .prepare(
            "SELECT e.event_id, COALESCE(e.query_terms, ''), i.document_id, i.chunk_id,
                    COALESCE(i.title_snapshot, ''), i.rank, i.action
             FROM search_events e JOIN result_interactions i ON i.event_id = e.event_id
             WHERE e.event_id IN (SELECT event_id FROM search_events
                                  WHERE event_type = 'results_shown'
                                  ORDER BY created_at DESC LIMIT ?1)
             ORDER BY e.created_at DESC, i.rank",
        )
        .map_err(err)?;
    // document_id is NULL for results of a revoked folder (tombstoned).
    type Row = (
        String,
        String,
        Option<String>,
        Option<i64>,
        String,
        Option<i64>,
        String,
    );
    let rows: Vec<Row> = st
        .query_map([PRIOR_EVENTS as i64], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;

    // Score per document: topic words in the earlier request or the shown
    // name; opened results and more recent events break ties.
    let mut best: Vec<(String, Option<i64>, f64)> = Vec::new();
    let mut withheld = false;
    for (age, (_, query, doc, chunk, title, rank, action)) in rows.iter().enumerate() {
        let hay = fold(&format!("{query} {title}"));
        let matched = topic.iter().filter(|t| hay.contains(t.as_str())).count();
        if !topic.is_empty() && matched == 0 {
            continue;
        }
        let Some(doc) = doc else {
            // A matching earlier result whose folder was revoked.
            withheld = true;
            continue;
        };
        let score = matched as f64 * 10.0
            + if action == "opened" { 3.0 } else { 0.0 }
            + 1.0 / (1.0 + rank.unwrap_or(0) as f64)
            - age as f64 * 1e-4;
        match best.iter_mut().find(|b| &b.0 == doc) {
            Some(b) if b.2 >= score => {}
            Some(b) => *b = (doc.clone(), *chunk, score),
            None => best.push((doc.clone(), *chunk, score)),
        }
    }
    best.sort_by(|a, b| b.2.total_cmp(&a.2));
    if topic.is_empty() {
        // "the last thing you showed me": the most recent event only.
        if let Some((first_event, ..)) = rows.first() {
            let docs: Vec<&String> = rows
                .iter()
                .filter(|r| &r.0 == first_event)
                .filter_map(|r| r.2.as_ref())
                .collect();
            best.retain(|b| docs.contains(&&b.0));
        }
    }
    let mut out = PriorLookup {
        hits: Vec::new(),
        withheld,
    };
    for (doc, chunk, score) in best {
        let path: Option<String> = conn
            .query_row(
                "SELECT canonical_path FROM documents WHERE document_id = ?1",
                [&doc],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        let ok = path.is_some_and(|p| {
            let p = std::path::Path::new(&p);
            acl.allows(p) && p.exists()
        });
        if !ok {
            out.withheld = true;
            continue;
        }
        out.hits.push(Hit {
            document_id: doc,
            chunk_id: chunk,
            score,
            reason: crate::search::Reason::Previous,
        });
        if out.hits.len() >= limit {
            break;
        }
    }
    if !out.hits.is_empty() {
        out.withheld = false;
    }
    Ok(out)
}

/// What the next request in a session is read against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionContext {
    /// Earlier exchanges, oldest first (at most the last 3).
    pub prior: Vec<PriorTurn>,
    /// Filters of the last search.
    pub active: Filters,
    /// The last answer was a clarifying question.
    pub last_was_clarify: bool,
    /// The request that question was about.
    pub last_user: Option<String>,
}

pub fn context(conn: &Connection, session: Option<&str>) -> Result<SessionContext, IpcError> {
    let Some(session) = session else {
        return Ok(SessionContext::default());
    };
    let active: Filters = conn
        .query_row(
            "SELECT active_slots_json FROM sessions WHERE session_id = ?1",
            [session],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(err)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut st = conn
        .prepare(
            "SELECT t.role, t.text, t.intent,
                (SELECT event_id FROM search_events e WHERE e.turn_id = t.turn_id LIMIT 1)
             FROM turns t WHERE t.session_id = ?1 ORDER BY t.ordinal DESC LIMIT 6",
        )
        .map_err(err)?;
    type Row = (String, Option<String>, Option<String>, Option<String>);
    let mut rows: Vec<Row> = st
        .query_map([session], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    rows.reverse();
    let mut ctx = SessionContext {
        active,
        ..Default::default()
    };
    for (role, text, intent, event) in rows {
        if role == "user" {
            ctx.prior.push(PriorTurn {
                user: text.clone().unwrap_or_default(),
                shown: Vec::new(),
            });
            ctx.last_user = text;
            ctx.last_was_clarify = false;
            continue;
        }
        ctx.last_was_clarify = intent.as_deref() == Some(PlanIntent::Clarify.db_intent());
        if let (Some(event), Some(last)) = (event, ctx.prior.last_mut()) {
            // Titles only; revoked results are tombstoned and left out.
            let mut st = conn
                .prepare(
                    "SELECT title_snapshot FROM result_interactions
                     WHERE event_id = ?1 AND action = 'shown' AND is_tombstone = 0
                       AND title_snapshot IS NOT NULL ORDER BY rank LIMIT 3",
                )
                .map_err(err)?;
            last.shown = st
                .query_map([event], |r| r.get(0))
                .map_err(err)?
                .collect::<Result<_, _>>()
                .map_err(err)?;
        }
    }
    Ok(ctx)
}

/// Note that a shown result was opened (for "opened" badges and history).
pub fn mark_opened(conn: &Connection, document_id: &str) -> Result<(), IpcError> {
    type Shown = (String, Option<i64>, Option<String>, Option<String>);
    let row: Option<Shown> = conn
        .query_row(
            "SELECT event_id, chunk_id, scope_id, title_snapshot FROM result_interactions
             WHERE document_id = ?1 AND action = 'shown' ORDER BY created_at DESC LIMIT 1",
            [document_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(err)?;
    if let Some((event, chunk, scope, title)) = row {
        conn.execute(
            "INSERT INTO result_interactions(interaction_id, event_id, document_id, chunk_id, scope_id, action, title_snapshot)
             VALUES (?1, ?2, ?3, ?4, ?5, 'opened', ?6)",
            params![new_id("i", document_id), event, document_id, chunk, scope, title],
        )
        .map_err(err)?;
    }
    Ok(())
}

pub fn session_turns(conn: &Connection, session: &str) -> Result<Vec<HistoryTurn>, IpcError> {
    let acl = Acl::load(conn)?;
    let mut st = conn
        .prepare(
            "SELECT t.turn_id, t.role, t.created_at, t.text, t.slots_json,
                (SELECT event_id FROM search_events e WHERE e.turn_id = t.turn_id LIMIT 1)
             FROM turns t WHERE t.session_id = ?1 ORDER BY t.ordinal",
        )
        .map_err(err)?;
    type Row = (
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = st
        .query_map([session], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    let mut out = Vec::new();
    let mut terms: Vec<String> = Vec::new();
    for (id, role, at, text, slots, event) in rows {
        if role == "user" {
            // Highlight the words that were actually searched.
            terms = slots
                .as_deref()
                .and_then(|s| serde_json::from_str::<Plan>(s).ok())
                .map(|p| p.terms)
                .unwrap_or_else(|| search::terms(text.as_deref().unwrap_or_default()));
            out.push(HistoryTurn {
                turn_id: id,
                role: TurnRole::User,
                at,
                text,
                kind: None,
                count: None,
                choices: Vec::new(),
                results: Vec::new(),
                opened: false,
                question: None,
                understood: None,
                answer: None,
                summary: None,
                picked_by_llm: false,
                notice: None,
            });
            continue;
        }
        let slots: serde_json::Value = slots
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let reasons: Vec<String> = slots["reasons"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let mut results = Vec::new();
        let mut opened = false;
        if let Some(event) = &event {
            let mut st = conn
                .prepare(
                    "SELECT document_id, chunk_id, rank FROM result_interactions
                     WHERE event_id = ?1 AND action = 'shown' AND is_tombstone = 0 AND document_id IS NOT NULL
                     ORDER BY rank",
                )
                .map_err(err)?;
            let shown: Vec<(String, Option<i64>, Option<i64>)> = st
                .query_map([event], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(err)?
                .collect::<Result<_, _>>()
                .map_err(err)?;
            for (doc, chunk, rank) in shown {
                let reason = rank
                    .and_then(|r| reasons.get(r as usize))
                    .map(String::as_str)
                    .unwrap_or("semantic");
                if let Some(r) = search::resolve(conn, &acl, &doc, chunk, reason, &terms)? {
                    results.push(r.card);
                }
            }
            opened = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM result_interactions WHERE event_id = ?1 AND action = 'opened')",
                    [event],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(err)?
                != 0;
        }
        let clarify = slots["kind"].as_str() == Some("clarify");
        let kind = if clarify {
            SearchKind::Clarify
        } else if results.is_empty() {
            SearchKind::NotFound
        } else {
            SearchKind::Results
        };
        let choices: Vec<Choice> = if clarify {
            serde_json::from_value(slots["choices"].clone()).unwrap_or_default()
        } else {
            Vec::new()
        };
        out.push(HistoryTurn {
            turn_id: id,
            role: TurnRole::Assistant,
            at,
            text: None,
            kind: Some(kind),
            count: Some(results.len() as u32),
            choices,
            opened,
            question: slots["question"].as_str().map(str::to_owned),
            understood: serde_json::from_value::<Option<Understood>>(slots["understood"].clone())
                .ok()
                .flatten(),
            // Shown only while its source is still shown (not revoked).
            answer: serde_json::from_value::<Option<DirectAnswer>>(slots["answer"].clone())
                .ok()
                .flatten()
                .filter(|a| results.iter().any(|r| r.result_ref == a.result_ref)),
            // A summary stays only while every file it cites is shown.
            summary: serde_json::from_value::<Option<Summary>>(slots["summary"].clone())
                .ok()
                .flatten()
                .filter(|s| {
                    s.sources
                        .iter()
                        .all(|src| results.iter().any(|r| &r.result_ref == src))
                }),
            picked_by_llm: slots["picked_by_llm"].as_bool().unwrap_or(false),
            notice: slots["notice"].as_str().map(str::to_owned),
            results,
        });
    }
    Ok(out)
}

pub fn list_sessions(conn: &Connection) -> Result<Vec<HistorySession>, IpcError> {
    let mut st = conn
        .prepare(
            "SELECT s.session_id, s.started_at,
                (SELECT text FROM turns t WHERE t.session_id = s.session_id AND t.role = 'user' ORDER BY ordinal LIMIT 1),
                (SELECT COUNT(*) FROM result_interactions i JOIN search_events e ON e.event_id = i.event_id
                   WHERE e.session_id = s.session_id AND i.action = 'shown' AND i.is_tombstone = 0),
                (SELECT COUNT(*) FROM result_interactions i JOIN search_events e ON e.event_id = i.event_id
                   WHERE e.session_id = s.session_id AND i.action = 'opened' AND i.is_tombstone = 0)
             FROM sessions s WHERE s.persisted = 1
               AND EXISTS (SELECT 1 FROM turns t WHERE t.session_id = s.session_id)
             ORDER BY s.last_active_at DESC LIMIT 200",
        )
        .map_err(err)?;
    let v = st
        .query_map([], |r| {
            let title: Option<String> = r.get(2)?;
            Ok(HistorySession {
                session_id: r.get(0)?,
                started_at: r.get(1)?,
                title: title
                    .map(|t| t.chars().take(80).collect())
                    .unwrap_or_default(),
                result_count: r.get(3)?,
                opened_count: r.get(4)?,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(v)
}

pub fn delete_session(conn: &Connection, session: &str) -> Result<(), IpcError> {
    // Turns, events and interactions go with it (ON DELETE CASCADE).
    conn.execute("DELETE FROM sessions WHERE session_id = ?1", [session])
        .map_err(err)?;
    Ok(())
}

pub fn clear_all(conn: &Connection) -> Result<(), IpcError> {
    conn.execute("DELETE FROM sessions", []).map_err(err)?;
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}

/// Delete conversations not used within the retention window (Settings →
/// Privacy). Their events and result references go with them. Returns how
/// many sessions were removed.
pub fn apply_retention(
    conn: &Connection,
    retention: crate::settings::HistoryRetention,
) -> Result<usize, IpcError> {
    use crate::settings::HistoryRetention;
    let days = match retention {
        HistoryRetention::Days30 => 30,
        HistoryRetention::Days90 => 90,
        HistoryRetention::Forever => return Ok(0),
    };
    conn.execute(
        "DELETE FROM sessions WHERE last_active_at < strftime('%Y-%m-%dT%H:%M:%fZ','now', ?1)",
        [format!("-{days} days")],
    )
    .map_err(err)
}

/// App start: drop sessions recorded while history was off.
pub fn purge_unpersisted(conn: &Connection) -> Result<(), IpcError> {
    conn.execute("DELETE FROM sessions WHERE persisted = 0", [])
        .map_err(err)?;
    Ok(())
}
