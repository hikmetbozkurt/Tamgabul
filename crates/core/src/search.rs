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
use crate::understand::Filters;

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
    /// Listed by file type / date only.
    Filter,
    /// Words found in the image description written at scan time.
    Caption,
    /// Shown in an earlier search (found from search history).
    Previous,
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
            Reason::Filter => "filter",
            Reason::Caption => "caption",
            Reason::Previous => "previous_result",
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

/// Request words that say nothing about the content ("bul", "dosya",
/// "the", "where"…). Compared after folding.
const STOP_WORDS: &[&str] = &[
    "bul",
    "bulur",
    "bulun",
    "bulsana",
    "bulabilir",
    "dosya",
    "dosyasi",
    "dosyayi",
    "dosyasini",
    "dosyada",
    "dosyalar",
    "dosyalari",
    "dosyalarini",
    "olan",
    "icin",
    "bana",
    "beni",
    "benim",
    "nerede",
    "hangi",
    "neydi",
    "vardi",
    "ile",
    "gibi",
    "ama",
    "veya",
    "bir",
    "icinde",
    "hakkinda",
    "ilgili",
    "goster",
    "getir",
    "misin",
    "musun",
    "lutfen",
    "tane",
    "seyi",
    "sunu",
    "bunu",
    "onu",
    "the",
    "and",
    "for",
    "with",
    "find",
    "file",
    "files",
    "where",
    "that",
    "this",
    "show",
    "about",
    "from",
    "get",
    "please",
    "was",
    "were",
    "which",
    "gecen",
    "once",
    "onceden",
    "daha",
    "sefer",
];

/// Words worth matching: trimmed of punctuation, 3+ characters (trigram FTS),
/// without request filler.
pub fn terms(q: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for w in q
        .split(|c: char| c.is_whitespace() || ",.;:!?()[]{}\"'`".contains(c))
        .map(str::trim)
        .filter(|w| w.chars().count() >= 3)
    {
        let f = fold(w);
        if STOP_WORDS.contains(&f.as_str()) || v.iter().any(|x| fold(x) == f) {
            continue;
        }
        v.push(w.to_string());
    }
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

/// Which kind of embedding a chunk has. Cosines between a text query and
/// image or audio vectors sit on a different scale than text-to-text, so
/// each group is banded against its own best match.
fn vector_group(modality: &str) -> Reason {
    match modality {
        "image" => Reason::Image,
        "audio" => Reason::Audio,
        _ => Reason::Semantic,
    }
}

/// Top chunks by cosine (vectors are L2-normalized), per modality group,
/// from the in-memory index (`vindex`): int8 scan + exact re-scoring.
fn vector_candidates(
    conn: &Connection,
    generation: &str,
    q: &[f32],
) -> Result<Vec<(i64, String, f32, Reason)>, IpcError> {
    let idx = crate::vindex::for_generation(conn, generation)?;
    Ok(idx
        .search(conn, q, CANDIDATES)?
        .into_iter()
        .map(|(r, s)| {
            (
                r.chunk_id,
                r.document_id.to_string(),
                s,
                vector_group(&r.modality),
            )
        })
        .collect())
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
            let reason = match m.as_str() {
                "ocr" => Reason::Ocr,
                "caption" => Reason::Caption,
                _ => Reason::Keyword,
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

/// Which documents may be returned, from the request's filters (docs/08
/// slots). Applied after fusion, together with the authorization check.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocFilter {
    /// Allowed `documents.file_type` values; `None` = all.
    pub types: Option<&'static [&'static str]>,
    /// `[from, to)` Unix seconds on the file's modification time.
    pub window: Option<(i64, i64)>,
    /// Folded words; a document whose name or matched text contains one
    /// is dropped.
    pub exclude: Vec<String>,
}

impl DocFilter {
    pub fn new(f: &Filters, now: i64, utc_offset: i64) -> Self {
        Self {
            types: f.file_type.db_types(),
            window: f.date.window(now, utc_offset),
            exclude: f
                .exclude
                .iter()
                .map(|w| fold(w.trim()))
                .filter(|w| !w.is_empty())
                .collect(),
        }
    }

    /// Only images or only audio: rank those vectors on their own scale.
    fn media_only(&self) -> bool {
        matches!(self.types, Some(t) if t == ["image"] || t == ["audio"])
    }

    fn passes(
        &self,
        conn: &Connection,
        path: &str,
        file_type: &str,
        mtime: &str,
        chunk: Option<i64>,
    ) -> bool {
        if self.types.is_some_and(|t| !t.contains(&file_type)) {
            return false;
        }
        if let Some((from, to)) = self.window {
            let secs = mtime_secs(mtime);
            if secs < from || secs >= to {
                return false;
            }
        }
        if !self.exclude.is_empty() {
            let name = fold(
                &Path::new(path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
            if self.exclude.iter().any(|w| name.contains(w.as_str())) {
                return false;
            }
            let text: Option<String> = chunk.and_then(|c| {
                conn.query_row("SELECT text FROM chunks WHERE chunk_id = ?1", [c], |r| {
                    r.get(0)
                })
                .ok()
                .flatten()
            });
            if let Some(t) = text {
                let t = fold(&t);
                if self.exclude.iter().any(|w| t.contains(w.as_str())) {
                    return false;
                }
            }
        }
        true
    }
}

/// `secs.nanos` (as stored) → whole seconds.
fn mtime_secs(m: &str) -> i64 {
    m.split('.')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
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
    search_terms(
        conn,
        generation,
        query_vec,
        &terms(text),
        &DocFilter::default(),
        limit,
    )
}

/// [`search`] with explicit match words and a document filter.
pub fn search_terms(
    conn: &Connection,
    generation: Option<&str>,
    query_vec: Option<&[f32]>,
    terms: &[String],
    filter: &DocFilter,
    limit: usize,
) -> Result<Vec<Hit>, IpcError> {
    let acl = Acl::load(conn)?;
    if acl.is_empty() {
        return Ok(Vec::new());
    }
    // chunk → (fused score, best reason)
    let mut fused: HashMap<(String, Option<i64>), (f64, Reason)> = HashMap::new();
    let mut add = |doc: &str, chunk: Option<i64>, rank: usize, reason: Reason, weight: f64| {
        let e = fused
            .entry((doc.to_string(), chunk))
            .or_insert((0.0, reason));
        e.0 += weight / (RRF_K + rank as f64 + 1.0);
        // Exact words explain a hit better than "meaning".
        if matches!(
            reason,
            Reason::Keyword | Reason::Filename | Reason::Ocr | Reason::Caption
        ) {
            e.1 = reason;
        }
    };

    if let (Some(g), Some(q)) = (generation, query_vec) {
        let vec = vector_candidates(conn, g, q)?;
        if filter.media_only() {
            // Text-to-image/audio cosines are lower than text-to-text: band
            // each media group against its own best.
            for group in [Reason::Image, Reason::Audio] {
                let mut in_group = vec.iter().filter(|c| c.3 == group).peekable();
                let Some(best) = in_group.peek().map(|c| c.2) else {
                    continue;
                };
                for (rank, (chunk, doc, cos, r)) in in_group.enumerate() {
                    if *cos >= best - SEMANTIC_BAND {
                        add(doc, Some(*chunk), rank, *r, 1.0);
                    }
                }
            }
        } else {
            let mut all: Vec<&(i64, String, f32, Reason)> = vec.iter().collect();
            all.sort_by(|a, b| b.2.total_cmp(&a.2));
            if let Some(best) = all.first().map(|v| v.2) {
                for (rank, (chunk, doc, cos, r)) in all.into_iter().enumerate() {
                    if *cos >= best - SEMANTIC_BAND {
                        add(doc, Some(*chunk), rank, *r, 1.0);
                    }
                }
            }
        }
    }
    if let Some(g) = generation {
        for (rank, (chunk, doc, reason)) in keyword_candidates(conn, g, terms)?.iter().enumerate() {
            add(doc, Some(*chunk), rank, *reason, 1.0);
        }
    }
    for (rank, doc) in filename_candidates(conn, terms)?.iter().enumerate() {
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

    // Re-authorize every result by its current path, then apply filters.
    let mut out = Vec::new();
    for h in hits {
        let Some((path, ft, mtime)) = doc_row(conn, &h.document_id) else {
            continue;
        };
        if acl.allows(Path::new(&path)) && filter.passes(conn, &path, &ft, &mtime, h.chunk_id) {
            out.push(h);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

fn doc_row(conn: &Connection, doc: &str) -> Option<(String, String, String)> {
    conn.query_row(
        "SELECT canonical_path, file_type, mtime FROM documents WHERE document_id = ?1",
        [doc],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

/// Filter-only listing, newest first ("dünkü ekran görüntüleri", "bu
/// haftaki PDF'ler"): used when a request has no words to match.
pub fn browse(conn: &Connection, filter: &DocFilter, limit: usize) -> Result<Vec<Hit>, IpcError> {
    let acl = Acl::load(conn)?;
    if acl.is_empty() {
        return Ok(Vec::new());
    }
    let mut st = conn
        .prepare(
            "SELECT document_id, canonical_path, file_type, mtime FROM documents
             WHERE parse_status = 'PARSED'",
        )
        .map_err(err)?;
    let mut rows: Vec<(String, String, String, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    rows.sort_by(|a, b| mtime_secs(&b.3).cmp(&mtime_secs(&a.3)).then(a.0.cmp(&b.0)));
    let mut out = Vec::new();
    for (doc, path, ft, mtime) in rows {
        if acl.allows(Path::new(&path)) && filter.passes(conn, &path, &ft, &mtime, None) {
            out.push(Hit {
                document_id: doc,
                chunk_id: None,
                score: 0.0,
                reason: Reason::Filter,
            });
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
        assert_eq!(terms("the auction, timer?  ab"), ["auction", "timer"]);
        assert_eq!(terms("Toplantı dosyasını bul toplanti"), ["Toplantı"]);
        assert_eq!(terms("timer dosyası nerede"), ["timer"]);
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
