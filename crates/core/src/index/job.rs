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
use crate::index::media::{self, Platform};
use crate::index::store::{self, DocMeta, NewChunk};
use crate::index::text;
use crate::index::walk::{self, Limits};
use crate::index::walk::{OfficeFormat, SourceKind};

/// Turns chunk texts into vectors. The app implements it with EmbeddingGemma
/// 2 (document prompt + title); tests use a deterministic fake.
pub trait Embedder {
    fn embed_documents(&mut self, title: &str, texts: &[String])
        -> Result<Vec<Vec<f32>>, IpcError>;
    /// Vision encoder present?
    fn supports_vision(&mut self) -> bool;
    /// Audio encoder present?
    fn supports_audio(&mut self) -> bool;
    /// One encoded image (PNG) or audio clip (WAV) → one vector.
    /// `ModelIncompatible` = this input was refused (skip it, keep going).
    fn embed_media(&mut self, bytes: &[u8]) -> Result<Vec<f32>, IpcError>;
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
    /// OS OCR / codecs / PDF rendering.
    pub platform: &'a dyn Platform,
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

/// Something to embed: a text chunk (from the file, a PDF page or OCR) or
/// a media segment (image, audio).
pub enum Piece {
    Text {
        chunk: text::Chunk,
        page: Option<u32>,
        ocr: bool,
    },
    Media {
        modality: &'static str,
        bytes: Vec<u8>,
        time_ms: Option<(u64, u64)>,
    },
}

/// Pages with fewer letters than this are treated as scanned and OCR'd.
const PDF_OCR_MIN_CHARS: usize = 40;
const PDF_OCR_MAX_PAGES: usize = 300;
const PDF_RENDER_WIDTH: u32 = 1700;

fn letters(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphanumeric()).count()
}

fn push_text(out: &mut Vec<Piece>, body: &str, page: Option<u32>, ocr: bool) {
    for chunk in text::chunk(body) {
        if out.len() >= text::MAX_CHUNKS_PER_DOC {
            return;
        }
        out.push(Piece::Text { chunk, page, ocr });
    }
}

fn ocr_into(out: &mut Vec<Piece>, platform: &dyn Platform, png: &[u8], page: Option<u32>) {
    if let Some(Ok(t)) = platform.ocr(png) {
        if letters(&t) >= 3 {
            push_text(out, &t, page, true);
        }
    }
}

/// Parse one file into pieces. `Err(reason)` = skip the file (recorded).
pub fn extract_pieces(
    kind: SourceKind,
    bytes: Vec<u8>,
    ext: Option<&str>,
    platform: &dyn Platform,
    embedder: &mut dyn Embedder,
    cancel: &AtomicBool,
) -> Result<Vec<Piece>, &'static str> {
    let mut out = Vec::new();
    match kind {
        SourceKind::Markdown | SourceKind::Text => {
            let body = text::decode(&bytes).ok_or("binary")?;
            push_text(&mut out, &body, None, false);
        }
        SourceKind::Markup => {
            let body = text::decode(&bytes).ok_or("binary")?;
            push_text(
                &mut out,
                &media::strip_markup(&body, media::HTML_BLOCKS),
                None,
                false,
            );
        }
        SourceKind::Rtf => {
            let body = text::decode(&bytes).ok_or("binary")?;
            push_text(&mut out, &media::rtf_text(&body), None, false);
        }
        SourceKind::Office(f) => {
            let k = match f {
                OfficeFormat::Docx => media::OfficeKind::Docx,
                OfficeFormat::Pptx => media::OfficeKind::Pptx,
                OfficeFormat::Xlsx => media::OfficeKind::Xlsx,
                OfficeFormat::OpenDocument => media::OfficeKind::OpenDocument,
            };
            let body = media::office_text(&bytes, k).map_err(|e| e.code())?;
            push_text(&mut out, &body, None, false);
        }
        SourceKind::Pdf => {
            let pages = match extract::pdf_pages(bytes.clone(), extract::PDF_TIMEOUT) {
                Ok(p) => p,
                Err(extract::PdfError::NoText) | Err(extract::PdfError::Unreadable) => Vec::new(),
                Err(e) => return Err(e.code()),
            };
            for (i, page) in pages.iter().enumerate() {
                push_text(&mut out, page, Some(i as u32 + 1), false);
            }
            // Scanned pages (little or no text layer) go through OCR.
            if platform.ocr_available() {
                let scanned: Vec<u32> = if pages.is_empty() {
                    (0..PDF_OCR_MAX_PAGES as u32).collect()
                } else {
                    pages
                        .iter()
                        .enumerate()
                        .filter(|(_, p)| letters(p) < PDF_OCR_MIN_CHARS)
                        .map(|(i, _)| i as u32)
                        .take(PDF_OCR_MAX_PAGES)
                        .collect()
                };
                if !scanned.is_empty() && !cancel.load(Ordering::Relaxed) {
                    if let Some(renders) =
                        platform.render_pdf_pages(&bytes, &scanned, PDF_RENDER_WIDTH)
                    {
                        for (i, png) in renders {
                            if cancel.load(Ordering::Relaxed) {
                                break;
                            }
                            ocr_into(&mut out, platform, &png, Some(i + 1));
                        }
                    }
                }
            }
            if out.is_empty() {
                return Err(if platform.ocr_available() {
                    "pdf_no_text"
                } else {
                    "pdf_no_text_no_ocr"
                });
            }
        }
        SourceKind::Image => {
            let img = media::normalize_image(&bytes, ext, platform).map_err(|e| e.code())?;
            ocr_into(&mut out, platform, &img.ocr_png, None);
            if embedder.supports_vision() {
                out.push(Piece::Media {
                    modality: "image",
                    bytes: img.model_png,
                    time_ms: None,
                });
            }
            if out.is_empty() {
                return Err("no_image_encoder");
            }
        }
        SourceKind::Audio => {
            if !embedder.supports_audio() {
                return Err("no_audio_encoder");
            }
            let samples = media::decode_audio(bytes, ext).map_err(|e| e.code())?;
            for seg in media::audio_segments(&samples) {
                out.push(Piece::Media {
                    modality: "audio",
                    bytes: seg.wav,
                    time_ms: Some((seg.start_ms, seg.end_ms)),
                });
            }
            if out.is_empty() {
                return Err("silent");
            }
        }
    }
    Ok(out)
}

/// Embed pieces. Text in batches (document prompt with the file name as
/// title); media one at a time. `Ok(None)` = cancelled.
pub fn embed_pieces(
    title: &str,
    pieces: Vec<Piece>,
    embedder: &mut dyn Embedder,
    batch: usize,
    cancel: &AtomicBool,
) -> Result<Option<Vec<NewChunk>>, IpcError> {
    let mut out: Vec<NewChunk> = Vec::with_capacity(pieces.len());
    let mut texts: Vec<(text::Chunk, Option<u32>, bool)> = Vec::new();
    let mut media_pieces = Vec::new();
    for p in pieces {
        match p {
            Piece::Text { chunk, page, ocr } => texts.push((chunk, page, ocr)),
            m => media_pieces.push(m),
        }
    }
    for group in texts.chunks(batch.max(1)) {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let inputs: Vec<String> = group.iter().map(|(c, _, _)| c.text.clone()).collect();
        let vecs = embedder.embed_documents(title, &inputs)?;
        if vecs.len() != group.len() {
            return Err(IpcError::new(
                ErrorCode::Internal,
                "embedding count mismatch",
            ));
        }
        for ((c, page, ocr), v) in group.iter().zip(vecs) {
            out.push(NewChunk {
                ordinal: out.len() as u32,
                modality: if *ocr { "ocr" } else { "text" },
                text: Some(c.text.clone()),
                page: *page,
                line_start: (!*ocr).then_some(c.line_start),
                line_end: (!*ocr).then_some(c.line_end),
                time_ms: None,
                vector: v,
            });
        }
    }
    for m in media_pieces {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let Piece::Media {
            modality,
            bytes,
            time_ms,
        } = m
        else {
            continue;
        };
        match embedder.embed_media(&bytes) {
            Ok(v) => out.push(NewChunk {
                ordinal: out.len() as u32,
                modality,
                text: None,
                page: None,
                line_start: None,
                line_end: None,
                time_ms,
                vector: v,
            }),
            // The model refused this one input (e.g. undecodable): keep the
            // rest of the file.
            Err(e) if e.code == ErrorCode::ModelIncompatible => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Some(out))
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
            let ext = c
                .path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            let pieces = match extract_pieces(
                c.kind,
                bytes,
                ext.as_deref(),
                input.platform,
                embedder,
                cancel,
            ) {
                Ok(p) => p,
                Err(reason) => {
                    p.skipped += 1;
                    return store::record_skipped(
                        conn,
                        input.scope_id,
                        &meta(c, &mtime, Some(&hash)),
                        "SKIPPED",
                        reason,
                    );
                }
            };
            let out = match embed_pieces(&title, pieces, embedder, input.batch, cancel)? {
                Some(out) => out,
                None => return Ok(false), // cancelled
            };
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
