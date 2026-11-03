//! One scan of one folder: discover → hash/stat → parse → chunk → embed →
//! atomic per-document commit (docs/07 index pipeline).
//!
//! - Unchanged files (same size + mtime, already embedded in the current
//!   generation) are not read again; same content hash → not re-embedded.
//! - Every document commit re-checks the scope is still ACTIVE, so a revoke
//!   during a scan stops new data immediately.
//! - Cancel stops between files; what was committed stays consistent.
//! - Source files are opened read-only.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::error::{ErrorCode, IpcError};
use crate::index::extract;
use crate::index::store::{self, DocMeta, NewChunk};
use crate::index::text;
use crate::index::walk::SourceKind;
use crate::index::walk::{self, Limits};

/// Turns chunk texts into vectors. The app implements it with EmbeddingGemma
/// 2 (document prompt + title); tests use a deterministic fake.
pub trait Embedder {
    fn embed_documents(&mut self, title: &str, texts: &[String])
        -> Result<Vec<Vec<f32>>, IpcError>;
    /// Can this model embed images (vision encoder present)?
    fn supports_images(&mut self) -> bool;
    /// Raw image file bytes (PNG/JPEG/…) → one vector.
    fn embed_image(&mut self, bytes: &[u8]) -> Result<Vec<f32>, IpcError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Progress {
    pub processed: u32,
    pub total: u32,
    pub indexed: u32,
    pub unchanged: u32,
    pub skipped: u32,
    pub errors: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Cancelled,
    /// The scope was paused/revoked while scanning.
    ScopeChanged,
}

pub struct JobInput<'a> {
    pub scope_id: &'a str,
    pub generation: &'a str,
    /// Paths never entered (app data, model folders).
    pub deny: &'a [PathBuf],
    pub limits: Limits,
    pub batch: usize,
}

fn meta<'a>(c: &'a walk::Candidate, mtime: &'a str, hash: Option<&'a str>) -> DocMeta<'a> {
    DocMeta {
        path: &c.path,
        kind: c.kind,
        size: c.size,
        mtime,
        content_hash: hash,
    }
}

const MAX_READ: u64 = 64 * 1024 * 1024;

fn read_file(p: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    let f = std::fs::File::open(p)?;
    let mut buf = Vec::new();
    f.take(max.min(MAX_READ) + 1).read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn run(
    conn: &mut Connection,
    input: JobInput<'_>,
    embedder: &mut dyn Embedder,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(Progress),
) -> Result<(Outcome, Progress), IpcError> {
    let scope = store::get_scope(conn, input.scope_id)?
        .ok_or_else(|| IpcError::new(ErrorCode::ScopeRevoked, "unknown folder"))?;
    if scope.state != store::ScopeState::Active {
        return Err(IpcError::new(
            ErrorCode::ScopeRevoked,
            "folder is not active",
        ));
    }
    let mut deny: Vec<PathBuf> = input.deny.to_vec();
    deny.extend(store::exclusions_for(conn, input.scope_id)?);
    let mut limits = input.limits;
    if let Some(m) = scope.max_file_bytes {
        limits.max_file_bytes = m;
    }
    let job = store::create_job(conn, input.scope_id, input.generation)?;

    let report = walk::walk(&scope.canonical_path, &deny, limits, cancel);
    let mut p = Progress {
        total: report.files.len() as u32,
        ..Default::default()
    };
    progress(p);
    let mut seen: HashSet<String> = HashSet::new();
    let mut last_emit = Instant::now();
    let mut outcome = if cancel.load(Ordering::Relaxed) {
        Outcome::Cancelled
    } else {
        Outcome::Done
    };

    for c in report.files.iter().take(if outcome == Outcome::Done {
        usize::MAX
    } else {
        0
    }) {
        if cancel.load(Ordering::Relaxed) {
            outcome = Outcome::Cancelled;
            break;
        }
        let doc_id = store::document_id(&c.path);
        seen.insert(doc_id.clone());
        let mtime = store::mtime_str(c.mtime);
        let prior = store::doc_by_path(conn, &c.path, input.generation)?;

        let committed = (|| -> Result<bool, IpcError> {
            if let Some(d) = &prior {
                if d.size == c.size
                    && d.mtime == mtime
                    && (d.indexed_in_generation || d.permanently_skipped())
                {
                    p.unchanged += 1;
                    return store::keep_document(conn, input.scope_id, &d.document_id, &mtime);
                }
            }
            let bytes = match read_file(&c.path, c.kind.max_bytes(limits.max_file_bytes)) {
                Ok(b) => b,
                Err(_) => {
                    p.errors += 1;
                    return store::record_skipped(
                        conn,
                        input.scope_id,
                        &meta(c, &mtime, None),
                        "FAILED",
                        "unreadable",
                    );
                }
            };
            let hash = blake3::hash(&bytes).to_hex().to_string();
            if let Some(d) = &prior {
                if d.content_hash.as_deref() == Some(hash.as_str()) && d.indexed_in_generation {
                    p.unchanged += 1;
                    return store::keep_document(conn, input.scope_id, &d.document_id, &mtime);
                }
            }
            let title = c
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // (page, chunk) pairs of text, or a single image.
            let mut pieces: Vec<(Option<u32>, text::Chunk)> = Vec::new();
            let mut image: Option<Vec<f32>> = None;
            match c.kind {
                SourceKind::Markdown | SourceKind::Text => {
                    let Some(body) = text::decode(&bytes) else {
                        p.skipped += 1;
                        return store::record_skipped(
                            conn,
                            input.scope_id,
                            &meta(c, &mtime, Some(&hash)),
                            "SKIPPED",
                            "binary",
                        );
                    };
                    pieces.extend(text::chunk(&body).into_iter().map(|ch| (None, ch)));
                }
                SourceKind::Pdf => match extract::pdf_pages(bytes, extract::PDF_TIMEOUT) {
                    Ok(pages) => {
                        for (i, page) in pages.iter().enumerate() {
                            for ch in text::chunk(page) {
                                if pieces.len() >= text::MAX_CHUNKS_PER_DOC {
                                    break;
                                }
                                pieces.push((Some(i as u32 + 1), ch));
                            }
                        }
                    }
                    Err(e) => {
                        p.skipped += 1;
                        return store::record_skipped(
                            conn,
                            input.scope_id,
                            &meta(c, &mtime, Some(&hash)),
                            "SKIPPED",
                            e.code(),
                        );
                    }
                },
                SourceKind::Image => {
                    if !embedder.supports_images() {
                        p.skipped += 1;
                        return store::record_skipped(
                            conn,
                            input.scope_id,
                            &meta(c, &mtime, Some(&hash)),
                            "SKIPPED",
                            "no_image_encoder",
                        );
                    }
                    match embedder.embed_image(&bytes) {
                        Ok(v) => image = Some(v),
                        // One undecodable image must not stop the scan.
                        Err(e) if e.code == ErrorCode::ModelIncompatible => {
                            p.skipped += 1;
                            return store::record_skipped(
                                conn,
                                input.scope_id,
                                &meta(c, &mtime, Some(&hash)),
                                "SKIPPED",
                                "image_unreadable",
                            );
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            let mut out = Vec::with_capacity(pieces.len().max(1));
            if let Some(v) = image {
                out.push(NewChunk {
                    ordinal: 0,
                    modality: "image",
                    text: None,
                    page: None,
                    line_start: None,
                    line_end: None,
                    vector: v,
                });
            }
            for (n, batch) in pieces.chunks(input.batch.max(1)).enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Ok(false);
                }
                let texts: Vec<String> = batch.iter().map(|(_, c)| c.text.clone()).collect();
                let vecs = embedder.embed_documents(&title, &texts)?;
                if vecs.len() != batch.len() {
                    return Err(IpcError::new(
                        ErrorCode::Internal,
                        "embedding count mismatch",
                    ));
                }
                for (i, ((page, ch), v)) in batch.iter().zip(vecs).enumerate() {
                    out.push(NewChunk {
                        ordinal: (n * input.batch.max(1) + i) as u32,
                        modality: "text",
                        text: Some(ch.text.clone()),
                        page: *page,
                        line_start: Some(ch.line_start),
                        line_end: Some(ch.line_end),
                        vector: v,
                    });
                }
            }
            if out.is_empty() {
                p.skipped += 1;
                return store::record_skipped(
                    conn,
                    input.scope_id,
                    &meta(c, &mtime, Some(&hash)),
                    "SKIPPED",
                    "empty",
                );
            }
            p.indexed += 1;
            store::commit_document(
                conn,
                input.scope_id,
                input.generation,
                &meta(c, &mtime, Some(&hash)),
                &out,
            )
        })();

        match committed {
            Ok(true) => {}
            Ok(false) if cancel.load(Ordering::Relaxed) => {
                outcome = Outcome::Cancelled;
                break;
            }
            Ok(false) => {
                outcome = Outcome::ScopeChanged;
                break;
            }
            // The embedder failing (model can't start, timeout) stops the job:
            // continuing would only fail every file the same way.
            Err(e) => {
                let _ = store::update_job(conn, &job, "FAILED", p.processed, p.errors + 1);
                return Err(e);
            }
        }
        p.processed += 1;
        if last_emit.elapsed() > Duration::from_millis(250) {
            store::update_job(conn, &job, "RUNNING", p.processed, p.errors)?;
            progress(p);
            last_emit = Instant::now();
        }
    }

    match outcome {
        Outcome::Done => {
            // Only a complete walk may drop documents that were not seen.
            if report.complete {
                store::unlink_missing(conn, input.scope_id, &seen)?;
            }
            store::finish_scan(conn, input.scope_id, input.generation)?;
            store::update_job(conn, &job, "DONE", p.processed, p.errors)?;
        }
        Outcome::Cancelled => store::update_job(conn, &job, "CANCELLED", p.processed, p.errors)?,
        Outcome::ScopeChanged => store::update_job(conn, &job, "CANCELLED", p.processed, p.errors)?,
    }
    progress(p);
    Ok((outcome, p))
}
