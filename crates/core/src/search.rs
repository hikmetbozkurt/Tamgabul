//! Hybrid retrieval (P2, docs/08): meaning (EmbeddingGemma 2 vectors) +
//! words (FTS5 trigram) + file names, fused with reciprocal rank fusion and
//! grouped per document. Every hit is re-authorized against the live folder
//! grants before it is returned (docs/07 §5): a revoked or excluded folder
//! never shows, even while its purge is still running.
//!
//! Scores are rank statistics, not probabilities.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::dto::{FileKind, ResultCard};
use crate::error::{ErrorCode, IpcError};
use crate::index::store::{self, Scope};
use crate::index::text::fold;
use crate::index::walk::is_within;

fn err(e: impl std::fmt::Display) -> IpcError {
    IpcError::new(ErrorCode::Internal, format!("search: {e}"))
}

pub const RRF_K: f64 = 60.0;
const CANDIDATES: usize = 200;
/// Meaning-only hits must be this close to the best cosine to be shown.
const SEMANTIC_BAND: f32 = 0.12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    Semantic,
    Keyword,
    Filename,
    Image,
    Audio,
    /// Words found in text read from an image or scanned page.
    Ocr,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Semantic => "semantic",
            Reason::Keyword => "keyword",
            Reason::Filename => "filename",
            Reason::Image => "image",
            Reason::Audio => "audio",
            Reason::Ocr => "ocr",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub document_id: String,
    pub chunk_id: Option<i64>,
    pub score: f64,
    pub reason: Reason,
}

/// Authorized folders and their exclusions, loaded once per query.
pub struct Acl {
    scopes: Vec<(Scope, Vec<PathBuf>)>,
}

impl Acl {
    pub fn load(conn: &Connection) -> Result<Self, IpcError> {
        let mut scopes = Vec::new();
        for s in store::visible_scopes(conn)? {
            if s.state.is_authorized() {
                let ex = store::exclusions_for(conn, &s.scope_id)?;
                scopes.push((s, ex));
            }
        }
        Ok(Self { scopes })
    }
    pub fn allows(&self, path: &Path) -> bool {
        self.scopes.iter().any(|(s, ex)| {
            is_within(path, &s.canonical_path) && !ex.iter().any(|e| is_within(path, e))
        })
    }
    pub fn scope_of(&self, path: &Path) -> Option<&Scope> {
        self.scopes
            .iter()
            .filter(|(s, _)| is_within(path, &s.canonical_path))
            .map(|(s, _)| s)
            .max_by_key(|s| s.canonical_path.as_os_str().len())
    }
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// Words worth matching: trimmed of punctuation, 3+ characters (trigram FTS).
pub fn terms(q: &str) -> Vec<String> {
    let mut v: Vec<String> = q
        .split(|c: char| c.is_whitespace() || ",.;:!?()[]{}\"'`".contains(c))
        .map(|w| w.trim().to_string())
        .filter(|w| w.chars().count() >= 3)
        .collect();
    v.dedup();
    v.truncate(12);
    v
}

/// FTS query over folded text (the index stores `tg_fold(text)`).
fn fts_query(terms: &[String]) -> Option<String> {
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .iter()
            .map(|t| format!("\"{}\"", fold(t).replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Top chunks by cosine (vectors are L2-normalized).
fn vector_candidates(
    conn: &Connection,
    generation: &str,
    q: &[f32],
) -> Result<Vec<(i64, String, f32, bool)>, IpcError> {
    let mut st = conn
        .prepare(
            "SELECT e.chunk_id, c.document_id, e.vector, c.modality FROM embedding_records e
             JOIN chunks c ON c.chunk_id = e.chunk_id WHERE e.generation_id = ?1",
        )
        .map_err(err)?;
    let mut rows = st.query([generation]).map_err(err)?;
    let mut top: Vec<(i64, String, f32, bool)> = Vec::new();
    while let Some(r) = rows.next().map_err(err)? {
        let blob: Vec<u8> = r.get(2).map_err(err)?;
        let v = store::vector_from_bytes(&blob);
        if v.len() != q.len() {
            continue;
        }
        let s = dot(q, &v);
        let modality: String = r.get(3).map_err(err)?;
        top.push((
            r.get(0).map_err(err)?,
            r.get(1).map_err(err)?,
            s,
            modality == "image",
        ));
    }
    top.sort_by(|a, b| b.2.total_cmp(&a.2));
    top.truncate(CANDIDATES);
    Ok(top)
}

fn keyword_candidates(
    conn: &Connection,
    generation: &str,
    terms: &[String],
) -> Result<Vec<(i64, String, Reason)>, IpcError> {
    let Some(q) = fts_query(terms) else {
        return Ok(Vec::new());
    };
    let mut st = conn
        .prepare(
            "SELECT c.chunk_id, c.document_id, c.modality FROM chunk_fts f JOIN chunks c ON c.chunk_id = f.rowid
             WHERE chunk_fts MATCH ?1 AND c.generation_id = ?2 ORDER BY f.rank LIMIT ?3",
        )
        .map_err(err)?;
    let v = st
        .query_map(params![q, generation, CANDIDATES as i64], |r| {
            let m: String = r.get(2)?;
            let reason = if m == "ocr" {
                Reason::Ocr
            } else {
                Reason::Keyword
            };
            Ok((r.get(0)?, r.get(1)?, reason))
        })
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    Ok(v)
}

fn filename_candidates(conn: &Connection, terms: &[String]) -> Result<Vec<String>, IpcError> {
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let mut st = conn
        .prepare("SELECT document_id, canonical_path FROM documents WHERE parse_status = 'PARSED'")
        .map_err(err)?;
    let rows = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(err)?;
    let lower: Vec<String> = terms.iter().map(|t| fold(t)).collect();
    let mut hits: Vec<(usize, String)> = rows
        .into_iter()
        .filter_map(|(id, p)| {
            let name = fold(&Path::new(&p).file_name()?.to_string_lossy());
            let n = lower.iter().filter(|t| name.contains(t.as_str())).count();
            (n > 0).then_some((n, id))
        })
        .collect();
    hits.sort_by_key(|h| std::cmp::Reverse(h.0));
    hits.truncate(CANDIDATES);
    Ok(hits.into_iter().map(|(_, id)| id).collect())
}

/// Ranked documents for a query. `query_vec` is `None` when the embedding
/// model is unavailable (word + file-name search still works).
pub fn search(
    conn: &Connection,
    generation: Option<&str>,
    query_vec: Option<&[f32]>,
    text: &str,
    limit: usize,
) -> Result<Vec<Hit>, IpcError> {
    let acl = Acl::load(conn)?;
    if acl.is_empty() {
        return Ok(Vec::new());
    }
    let terms = terms(text);
    // chunk → (fused score, best reason)
    let mut fused: HashMap<(String, Option<i64>), (f64, Reason)> = HashMap::new();
    let mut add = |doc: &str, chunk: Option<i64>, rank: usize, reason: Reason, weight: f64| {
        let e = fused
            .entry((doc.to_string(), chunk))
            .or_insert((0.0, reason));
        e.0 += weight / (RRF_K + rank as f64 + 1.0);
        // Exact words explain a hit better than "meaning".
        if matches!(reason, Reason::Keyword | Reason::Filename | Reason::Ocr) {
            e.1 = reason;
        }
    };

    if let (Some(g), Some(q)) = (generation, query_vec) {
        let vec = vector_candidates(conn, g, q)?;
        if let Some(best) = vec.first().map(|v| v.2) {
            for (rank, (chunk, doc, cos, image)) in vec.iter().enumerate() {
                if *cos >= best - SEMANTIC_BAND {
                    let r = if *image {
                        Reason::Image
                    } else {
                        Reason::Semantic
                    };
                    add(doc, Some(*chunk), rank, r, 1.0);
                }
            }
        }
    }
    if let Some(g) = generation {
        for (rank, (chunk, doc, reason)) in keyword_candidates(conn, g, &terms)?.iter().enumerate()
        {
            add(doc, Some(*chunk), rank, *reason, 1.0);
        }
    }
    for (rank, doc) in filename_candidates(conn, &terms)?.iter().enumerate() {
        add(doc, None, rank, Reason::Filename, 0.8);
    }

    // Group per document: sum of its best chunk and file-name score.
    let mut docs: HashMap<String, Hit> = HashMap::new();
    for ((doc, chunk), (score, reason)) in fused {
        let e = docs.entry(doc.clone()).or_insert(Hit {
            document_id: doc,
            chunk_id: None,
            score: 0.0,
            reason,
        });
        if chunk.is_none() {
            e.score += score;
            if e.chunk_id.is_none() {
                e.reason = reason;
            }
        } else if e.chunk_id.is_none() || score > e.score {
            let name_bonus = if e.chunk_id.is_none() { e.score } else { 0.0 };
            e.score = score + name_bonus;
            e.chunk_id = chunk;
            e.reason = reason;
        }
    }
    let mut hits: Vec<Hit> = docs.into_values().collect();
    hits.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.document_id.cmp(&b.document_id))
    });

    // Re-authorize every result by its current path.
    let mut out = Vec::new();
    for h in hits {
        let path: Option<String> = conn
            .query_row(
                "SELECT canonical_path FROM documents WHERE document_id = ?1",
                [&h.document_id],
                |r| r.get(0),
            )
            .ok();
        if path.is_some_and(|p| acl.allows(Path::new(&p))) {
            out.push(h);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- cards

pub fn file_kind(db: &str) -> FileKind {
    match db {
        "markdown" => FileKind::Markdown,
        "text" => FileKind::Text,
        "pdf" => FileKind::Pdf,
        "image" => FileKind::Image,
        "audio" => FileKind::Audio,
        "docx" | "pptx" | "xlsx" | "odf" | "rtf" => FileKind::Document,
        "markup" => FileKind::Text,
        _ => FileKind::Other,
    }
}

/// `secs.nanos` (as stored) → RFC 3339 UTC.
pub fn rfc3339_from_mtime(m: &str) -> String {
    let secs: i64 = m
        .split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Char index of the first folded match of any term.
fn first_match(chars: &[char], terms: &[String]) -> Option<usize> {
    let folded: Vec<char> = chars
        .iter()
        .map(|c| crate::index::text::fold_char(*c))
        .collect();
    terms
        .iter()
        .filter_map(|t| {
            let t: Vec<char> = fold(t).chars().collect();
            (!t.is_empty())
                .then(|| folded.windows(t.len()).position(|w| w == t.as_slice()))
                .flatten()
        })
        .min()
}

/// Snippet of about `max` characters around the first matched term
/// (accent/case-insensitive, Turkish-aware).
pub fn snippet(text: &str, terms: &[String], max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = flat.chars().collect();
    let center = first_match(&chars, terms).unwrap_or(0);
    let start = center.saturating_sub(max / 3);
    let end = (start + max).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s = format!("…{}", s.trim_start());
    }
    if end < chars.len() {
        s = format!("{}…", s.trim_end());
    }
    s
}

/// The snippet's own spelling of each matched term ("Toplantı" for the
/// query "toplanti"), for highlighting.
pub fn highlights(snippet: &str, terms: &[String]) -> Vec<String> {
    let chars: Vec<char> = snippet.chars().collect();
    let folded: Vec<char> = chars
        .iter()
        .map(|c| crate::index::text::fold_char(*c))
        .collect();
    let mut v = Vec::new();
    for t in terms {
        let tf: Vec<char> = fold(t).chars().collect();
        if tf.is_empty() {
            continue;
        }
        if let Some(i) = folded.windows(tf.len()).position(|w| w == tf.as_slice()) {
            let orig: String = chars[i..i + tf.len()].iter().collect();
            if !v.contains(&orig) {
                v.push(orig);
            }
        }
    }
    v
}

pub const RESULT_REF_PREFIX: &str = "r1";

pub fn result_ref(doc: &str, chunk: Option<i64>) -> String {
    format!(
        "{RESULT_REF_PREFIX}:{doc}:{}",
        chunk.map(|c| c.to_string()).unwrap_or_default()
    )
}

pub fn parse_ref(r: &str) -> Option<(String, Option<i64>)> {
    let mut it = r.splitn(3, ':');
    if it.next()? != RESULT_REF_PREFIX {
        return None;
    }
    let doc = it.next()?.to_string();
    let chunk = it.next().and_then(|c| c.parse().ok());
    Some((doc, chunk))
}

pub struct Resolved {
    pub path: PathBuf,
    pub card: ResultCard,
    pub chunk_text: Option<String>,
}

/// Build the card for a document (+ chunk), or `None` if it is gone or no
/// longer authorized.
pub fn resolve(
    conn: &Connection,
    acl: &Acl,
    doc: &str,
    chunk: Option<i64>,
    reason: &str,
    terms: &[String],
) -> Result<Option<Resolved>, IpcError> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT canonical_path, file_type, mtime FROM documents WHERE document_id = ?1",
            [doc],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let Some((path, kind, mtime)) = row else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if !acl.allows(&path) {
        return Ok(None);
    }
    let chunk_row: Option<(Option<String>, Option<i64>)> = match chunk {
        Some(c) => conn
            .query_row(
                "SELECT text, page FROM chunks WHERE chunk_id = ?1 AND document_id = ?2",
                params![c, doc],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok(),
        None => conn
            .query_row(
                "SELECT text, page FROM chunks WHERE document_id = ?1 ORDER BY ordinal LIMIT 1",
                [doc],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok(),
    };
    let (mut text, _page) = chunk_row.unwrap_or((None, None));
    if text.is_none() {
        // Image/audio hit: show the text read from the image, if any.
        text = conn
            .query_row(
                "SELECT text FROM chunks WHERE document_id = ?1 AND text IS NOT NULL ORDER BY ordinal LIMIT 1",
                [doc],
                |r| r.get(0),
            )
            .ok();
    }
    let snip = text.as_deref().map(|t| snippet(t, terms, 220));
    let marks = snip
        .as_deref()
        .map(|s| highlights(s, terms))
        .unwrap_or_default();
    let folder = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let project = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned());
    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Some(Resolved {
        card: ResultCard {
            result_ref: result_ref(doc, chunk),
            title,
            display_path: folder,
            project,
            file_kind: file_kind(&kind),
            snippet: snip,
            highlights: marks,
            modified_at: rfc3339_from_mtime(&mtime),
            match_reason: reason.to_string(),
        },
        chunk_text: text,
        path,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terms_and_fts_query() {
        assert_eq!(
            terms("the auction, timer?  ab"),
            ["the", "auction", "timer"]
        );
        assert_eq!(fts_query(&["a\"b".into()]).unwrap(), "\"a\"\"b\"");
        assert!(fts_query(&[]).is_none());
    }

    #[test]
    fn rfc3339() {
        assert_eq!(rfc3339_from_mtime("0.0"), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_from_mtime("1793600000.5"), "2026-11-02T06:13:20Z");
        assert_eq!(rfc3339_from_mtime("951782400.0"), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn snippet_centers_on_term_and_marks_it() {
        let text = format!(
            "{} the Auction countdown timer restarts {}",
            "x ".repeat(200),
            "y ".repeat(200)
        );
        let t = terms("auction timer");
        let s = snippet(&text, &t, 80);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.contains("Auction countdown timer"));
        assert_eq!(highlights(&s, &t), ["Auction", "timer"]);
    }

    #[test]
    fn matching_ignores_case_accents_and_turkish_i() {
        let t = terms("toplanti surum");
        let s = snippet("Dün yapılan Toplantı notları: sürüm cuma.", &t, 200);
        assert_eq!(highlights(&s, &t), ["Toplantı", "sürüm"]);
        assert_eq!(fts_query(&terms("Sürüm İŞ")).unwrap(), "\"surum\"");
    }

    #[test]
    fn refs_round_trip() {
        assert_eq!(
            parse_ref(&result_ref("d-1", Some(5))),
            Some(("d-1".into(), Some(5)))
        );
        assert_eq!(
            parse_ref(&result_ref("d-1", None)),
            Some(("d-1".into(), None))
        );
        assert_eq!(parse_ref("x:d:1"), None);
    }
}
