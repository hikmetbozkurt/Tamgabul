//! Optional explicit model downloader (docs/02, docs/04, docs/11).
//!
//! Nothing in this module ever runs automatically at startup or during search.
//! Downloads happen strictly after explicit user action + consent, from pinned
//! Hugging Face commits over HTTPS (redirects checked against downgrades),
//! streaming to `.part` files and verifying exact byte length and SHA-256
//! before renaming into place.

use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tamgabul_core::dto::DownloadPhase;
use tamgabul_core::error::ErrorCode;
use tamgabul_core::model::ModelCategory;

use crate::eg2::{self, PinnedFile};
use crate::{llm, stt, tts};

pub const ORIGIN_HOST: &str = "huggingface.co";
const MAX_REDIRECTS: usize = 5;
const BUF_BYTES: usize = 64 * 1024;
const PROGRESS_EVERY: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy)]
pub struct ModelPreset {
    pub preset_id: &'static str,
    pub category: ModelCategory,
    pub display_name: &'static str,
    pub recommended: bool,
    pub repo: &'static str,
    /// Optional subdirectory inside `repo` before file names (e.g. piper voice path).
    pub repo_prefix: &'static str,
    pub commit: &'static str,
    pub license_name: &'static str,
    pub license_url: &'static str,
    pub default_subdir: &'static str,
    pub files: &'static [PinnedFile],
    pub capabilities: &'static [&'static str],
}

impl ModelPreset {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size_bytes).sum()
    }
}

const EG2_BF16_FILES: &[PinnedFile] = &[eg2::BF16_MODEL, eg2::BF16_MMPROJ];
const EG2_Q8_FILES: &[PinnedFile] = &[eg2::Q8_MODEL, eg2::Q8_MMPROJ];
const LLM_E2B_FILES: &[PinnedFile] = &[llm::Q4_0_MODEL, llm::MMPROJ_Q8_0];
const STT_SMALL_Q5_FILES: &[PinnedFile] = &[stt::SMALL_Q5_1_MODEL];
const STT_TURBO_Q5_FILES: &[PinnedFile] = &[stt::LARGE_V3_TURBO_Q5_0_MODEL];
const TTS_TR_DFKI_FILES: &[PinnedFile] = &[tts::TR_DFKI_ONNX, tts::TR_DFKI_JSON];
const TTS_EN_LJSPEECH_FILES: &[PinnedFile] = &[tts::EN_LJSPEECH_ONNX, tts::EN_LJSPEECH_JSON];

pub const CATALOG: &[ModelPreset] = &[
    ModelPreset {
        preset_id: "eg2-bf16",
        category: ModelCategory::Embedding,
        display_name: "EmbeddingGemma 2 (BF16 + image/audio)",
        recommended: true,
        repo: eg2::PINNED_REPO,
        repo_prefix: "",
        commit: eg2::PINNED_COMMIT,
        license_name: "Apache-2.0",
        license_url: "https://huggingface.co/ggml-org/embeddinggemma-2-GGUF",
        default_subdir: "embeddinggemma-2-bf16",
        files: EG2_BF16_FILES,
        capabilities: &["text", "image", "audio"],
    },
    ModelPreset {
        preset_id: "eg2-q8_0",
        category: ModelCategory::Embedding,
        display_name: "EmbeddingGemma 2 (Q8_0 + image/audio)",
        recommended: false,
        repo: eg2::PINNED_REPO,
        repo_prefix: "",
        commit: eg2::PINNED_COMMIT,
        license_name: "Apache-2.0",
        license_url: "https://huggingface.co/ggml-org/embeddinggemma-2-GGUF",
        default_subdir: "embeddinggemma-2-q8_0",
        files: EG2_Q8_FILES,
        capabilities: &["text", "image", "audio"],
    },
    ModelPreset {
        preset_id: "gemma4-e2b-q4_0",
        category: ModelCategory::Llm,
        display_name: "Gemma 4 E2B-it (Q4_0 + vision)",
        recommended: true,
        repo: llm::PINNED_REPO,
        repo_prefix: "",
        commit: llm::PINNED_COMMIT,
        license_name: "Apache-2.0",
        license_url: "https://huggingface.co/ggml-org/gemma-4-E2B-it-GGUF",
        default_subdir: "gemma-4-e2b-it",
        files: LLM_E2B_FILES,
        capabilities: &["search_plan", "result_pick", "image_caption"],
    },
    ModelPreset {
        preset_id: "whisper-small-q5_1",
        category: ModelCategory::Stt,
        display_name: "Whisper small (Q5_1 · TR + EN)",
        recommended: true,
        repo: stt::PINNED_REPO,
        repo_prefix: "",
        commit: stt::PINNED_COMMIT,
        license_name: "MIT",
        license_url: "https://huggingface.co/ggerganov/whisper.cpp",
        default_subdir: "whisper-small-q5_1",
        files: STT_SMALL_Q5_FILES,
        capabilities: &["voice_input", "audio_transcript", "tr", "en"],
    },
    ModelPreset {
        preset_id: "whisper-large-v3-turbo-q5_0",
        category: ModelCategory::Stt,
        display_name: "Whisper large-v3-turbo (Q5_0 · TR + EN)",
        recommended: false,
        repo: stt::PINNED_REPO,
        repo_prefix: "",
        commit: stt::PINNED_COMMIT,
        license_name: "MIT",
        license_url: "https://huggingface.co/ggerganov/whisper.cpp",
        default_subdir: "whisper-large-v3-turbo-q5_0",
        files: STT_TURBO_Q5_FILES,
        capabilities: &["voice_input", "audio_transcript", "tr", "en"],
    },
    ModelPreset {
        preset_id: "piper-tr-dfki-medium",
        category: ModelCategory::Tts,
        display_name: "Piper Turkish (dfki medium · non-commercial dataset)",
        recommended: true,
        repo: tts::PINNED_REPO,
        repo_prefix: tts::TR_DFKI_PREFIX,
        commit: tts::PINNED_COMMIT,
        license_name: "CC-BY-NC-SA-4.0 (dataset) / MIT (weights card)",
        license_url: "https://huggingface.co/rhasspy/piper-voices/blob/main/tr/tr_TR/dfki/medium/MODEL_CARD",
        default_subdir: "piper-tr-dfki-medium",
        files: TTS_TR_DFKI_FILES,
        capabilities: &["read_aloud", "tr"],
    },
    ModelPreset {
        preset_id: "piper-en-ljspeech-medium",
        category: ModelCategory::Tts,
        display_name: "Piper English US (ljspeech medium)",
        recommended: false,
        repo: tts::PINNED_REPO,
        repo_prefix: tts::EN_LJSPEECH_PREFIX,
        commit: tts::PINNED_COMMIT,
        license_name: "Public Domain (LJ Speech dataset) / MIT",
        license_url: "https://huggingface.co/rhasspy/piper-voices/blob/main/en/en_US/ljspeech/medium/MODEL_CARD",
        default_subdir: "piper-en-ljspeech-medium",
        files: TTS_EN_LJSPEECH_FILES,
        capabilities: &["read_aloud", "en"],
    },
];

pub fn catalog() -> &'static [ModelPreset] {
    CATALOG
}

pub fn find_preset(preset_id: &str) -> Option<&'static ModelPreset> {
    CATALOG.iter().find(|p| p.preset_id == preset_id)
}

pub fn file_url(preset: &ModelPreset, file: &PinnedFile) -> String {
    format!(
        "https://{ORIGIN_HOST}/{}/resolve/{}/{}{}",
        preset.repo, preset.commit, preset.repo_prefix, file.name
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadError {
    InvalidDestination,
    Network(String),
    HttpStatus(u16),
    UnsafeRedirect,
    TooManyRedirects,
    SizeMismatch,
    ChecksumMismatch,
    Cancelled,
    Io(String),
}

impl DownloadError {
    pub fn error_key(&self) -> &'static str {
        match self {
            DownloadError::InvalidDestination => "addModel.downloadErr.destination",
            DownloadError::Network(_) => "addModel.downloadErr.network",
            DownloadError::HttpStatus(_) => "addModel.downloadErr.status",
            DownloadError::UnsafeRedirect | DownloadError::TooManyRedirects => {
                "addModel.downloadErr.unsafeRedirect"
            }
            DownloadError::SizeMismatch => "addModel.downloadErr.size",
            DownloadError::ChecksumMismatch => "addModel.downloadErr.checksum",
            DownloadError::Cancelled => "addModel.downloadErr.cancelled",
            DownloadError::Io(_) => "addModel.downloadErr.io",
        }
    }

    pub fn error_code(&self) -> ErrorCode {
        match self {
            DownloadError::Cancelled => ErrorCode::UserCancelled,
            DownloadError::InvalidDestination => ErrorCode::ScopeNotAuthorized,
            DownloadError::SizeMismatch | DownloadError::ChecksumMismatch => {
                ErrorCode::ModelIncompatible
            }
            DownloadError::Network(_)
            | DownloadError::HttpStatus(_)
            | DownloadError::UnsafeRedirect
            | DownloadError::TooManyRedirects
            | DownloadError::Io(_) => ErrorCode::Internal,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileProgress {
    pub phase: DownloadPhase,
    pub file_index: u32,
    pub file_count: u32,
    pub file_name: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

pub fn download_preset(
    preset: &ModelPreset,
    dest_dir: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(FileProgress),
) -> Result<PathBuf, DownloadError> {
    let items: Vec<(String, PinnedFile)> = preset
        .files
        .iter()
        .map(|f| (file_url(preset, f), *f))
        .collect();
    download_files(&items, dest_dir, false, cancel, on_progress)
}

fn is_allowed_url(url: &str, allow_loopback_http: bool) -> bool {
    if let Some(rest) = url.strip_prefix("https://") {
        let auth = rest.split('/').next().unwrap_or("");
        return !auth.is_empty() && !auth.contains('@');
    }
    if allow_loopback_http {
        if let Some(rest) = url.strip_prefix("http://") {
            let auth = rest.split('/').next().unwrap_or("");
            if auth.is_empty() || auth.contains('@') {
                return false;
            }
            let host = auth.split(':').next().unwrap_or("");
            return host == "127.0.0.1";
        }
    }
    false
}

fn origin_of(url: &str) -> Option<&str> {
    let scheme_end = url.find("://")?;
    let after_scheme = scheme_end + 3;
    match url[after_scheme..].find('/') {
        Some(rel) => Some(&url[..after_scheme + rel]),
        None => Some(url),
    }
}

fn resolve_redirect(
    base_url: &str,
    location: &str,
    allow_loopback_http: bool,
) -> Result<String, DownloadError> {
    let loc = location.trim();
    if loc.is_empty() {
        return Err(DownloadError::UnsafeRedirect);
    }
    let next = if loc.starts_with("https://") || loc.starts_with("http://") {
        loc.to_string()
    } else if loc.starts_with("//") {
        let scheme = base_url.split("://").next().unwrap_or("https");
        format!("{scheme}:{loc}")
    } else if loc.starts_with('/') {
        let origin = origin_of(base_url).ok_or(DownloadError::UnsafeRedirect)?;
        format!("{origin}{loc}")
    } else if loc
        .split(['/', '?', '#'])
        .next()
        .is_some_and(|seg| seg.contains(':'))
    {
        return Err(DownloadError::UnsafeRedirect);
    } else {
        let dir = match base_url.rfind('/') {
            Some(pos) if pos > base_url.find("://").unwrap_or(0) + 2 => &base_url[..=pos],
            _ => return Err(DownloadError::UnsafeRedirect),
        };
        format!("{dir}{loc}")
    };
    if is_allowed_url(&next, allow_loopback_http) {
        Ok(next)
    } else {
        Err(DownloadError::UnsafeRedirect)
    }
}

fn open_stream(
    agent: &ureq::Agent,
    initial_url: &str,
    allow_loopback_http: bool,
) -> Result<ureq::Response, DownloadError> {
    if !is_allowed_url(initial_url, allow_loopback_http) {
        return Err(DownloadError::UnsafeRedirect);
    }
    let mut url = initial_url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let resp = match agent.get(&url).call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => return Err(DownloadError::HttpStatus(code)),
            Err(ureq::Error::Transport(t)) => return Err(DownloadError::Network(t.to_string())),
        };
        let status = resp.status();
        if (200..=299).contains(&status) {
            return Ok(resp);
        }
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let loc = resp
                .header("Location")
                .ok_or(DownloadError::UnsafeRedirect)?;
            url = resolve_redirect(&url, loc, allow_loopback_http)?;
            continue;
        }
        return Err(DownloadError::HttpStatus(status));
    }
    Err(DownloadError::TooManyRedirects)
}

fn sha256_file_cancelable(path: &Path, cancel: &AtomicBool) -> Result<String, DownloadError> {
    let f = File::open(path).map_err(|e| DownloadError::Io(e.to_string()))?;
    let mut r = BufReader::with_capacity(BUF_BYTES, f);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF_BYTES];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let n = r
            .read(&mut buf)
            .map_err(|e| DownloadError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn download_files(
    items: &[(String, PinnedFile)],
    dest_dir: &Path,
    allow_loopback_http: bool,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(FileProgress),
) -> Result<PathBuf, DownloadError> {
    if dest_dir.as_os_str().is_empty() || dest_dir.is_file() {
        return Err(DownloadError::InvalidDestination);
    }
    fs::create_dir_all(dest_dir).map_err(|_| DownloadError::InvalidDestination)?;

    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .try_proxy_from_env(false)
        .build();

    let total_bytes: u64 = items.iter().map(|(_, f)| f.size_bytes).sum();
    let file_count = items.len() as u32;
    let mut completed_bytes = 0u64;

    for (idx, (url, file)) in items.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let file_index = idx as u32 + 1;
        let target = dest_dir.join(file.name);
        let part = dest_dir.join(format!("{}.part", file.name));

        // Reuse an already complete and verified file without hitting the network.
        if target.is_file()
            && fs::metadata(&target)
                .map(|m| m.len())
                .is_ok_and(|len| len == file.size_bytes)
        {
            on_progress(FileProgress {
                phase: DownloadPhase::Verifying,
                file_index,
                file_count,
                file_name: file.name.to_string(),
                bytes_done: completed_bytes,
                bytes_total: total_bytes,
            });
            if sha256_file_cancelable(&target, cancel)?.eq_ignore_ascii_case(file.sha256) {
                completed_bytes = completed_bytes.saturating_add(file.size_bytes);
                on_progress(FileProgress {
                    phase: DownloadPhase::Verifying,
                    file_index,
                    file_count,
                    file_name: file.name.to_string(),
                    bytes_done: completed_bytes,
                    bytes_total: total_bytes,
                });
                continue;
            }
        }

        on_progress(FileProgress {
            phase: DownloadPhase::Downloading,
            file_index,
            file_count,
            file_name: file.name.to_string(),
            bytes_done: completed_bytes,
            bytes_total: total_bytes,
        });

        let resp = open_stream(&agent, url, allow_loopback_http)?;
        if let Some(len_str) = resp.header("Content-Length") {
            let Ok(cl) = len_str.trim().parse::<u64>() else {
                return Err(DownloadError::SizeMismatch);
            };
            if cl != file.size_bytes {
                return Err(DownloadError::SizeMismatch);
            }
        }

        let mut reader = resp.into_reader();
        let mut out = File::create(&part).map_err(|e| DownloadError::Io(e.to_string()))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; BUF_BYTES];
        let mut written = 0u64;
        let mut last_emit = Instant::now();

        let stream_res = loop {
            if cancel.load(Ordering::Relaxed) {
                break Err(DownloadError::Cancelled);
            }
            let n = match reader.read(&mut buf) {
                Ok(n) => n,
                Err(e) => break Err(DownloadError::Network(e.to_string())),
            };
            if n == 0 {
                break Ok(());
            }
            let next_written = written.saturating_add(n as u64);
            if next_written > file.size_bytes {
                break Err(DownloadError::SizeMismatch);
            }
            if let Err(e) = out.write_all(&buf[..n]) {
                break Err(DownloadError::Io(e.to_string()));
            }
            hasher.update(&buf[..n]);
            written = next_written;
            if last_emit.elapsed() >= PROGRESS_EVERY {
                on_progress(FileProgress {
                    phase: DownloadPhase::Downloading,
                    file_index,
                    file_count,
                    file_name: file.name.to_string(),
                    bytes_done: completed_bytes.saturating_add(written),
                    bytes_total: total_bytes,
                });
                last_emit = Instant::now();
            }
        };

        if let Err(e) = stream_res {
            drop(out);
            let _ = fs::remove_file(&part);
            return Err(e);
        }
        if written != file.size_bytes {
            drop(out);
            let _ = fs::remove_file(&part);
            return Err(DownloadError::SizeMismatch);
        }
        if let Err(e) = out.flush().and_then(|_| out.sync_all()) {
            drop(out);
            let _ = fs::remove_file(&part);
            return Err(DownloadError::Io(e.to_string()));
        }
        drop(out);

        on_progress(FileProgress {
            phase: DownloadPhase::Verifying,
            file_index,
            file_count,
            file_name: file.name.to_string(),
            bytes_done: completed_bytes.saturating_add(written),
            bytes_total: total_bytes,
        });

        let digest: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if !digest.eq_ignore_ascii_case(file.sha256) {
            let _ = fs::remove_file(&part);
            return Err(DownloadError::ChecksumMismatch);
        }

        if target.exists() {
            fs::remove_file(&target).map_err(|e| DownloadError::Io(e.to_string()))?;
        }
        fs::rename(&part, &target).map_err(|e| DownloadError::Io(e.to_string()))?;
        completed_bytes = completed_bytes.saturating_add(file.size_bytes);
    }

    on_progress(FileProgress {
        phase: DownloadPhase::Done,
        file_index: file_count,
        file_count,
        file_name: items
            .last()
            .map(|(_, f)| f.name.to_string())
            .unwrap_or_default(),
        bytes_done: total_bytes,
        bytes_total: total_bytes,
    });
    Ok(dest_dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn sha256_hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn read_http_head(stream: &mut std::net::TcpStream) -> String {
        let mut req = Vec::new();
        let mut buf = [0u8; 512];
        while !req.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = match stream.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            req.extend_from_slice(&buf[..n]);
        }
        String::from_utf8_lossy(&req).into_owned()
    }

    #[test]
    fn catalog_is_pinned_https_with_verified_hashes() {
        assert_eq!(catalog().len(), 7);
        for p in catalog() {
            assert!(!p.license_name.is_empty());
            assert!(p.license_url.starts_with("https://huggingface.co/"));
            assert!(!p.files.is_empty());
            let mut sum = 0u64;
            for f in p.files {
                let url = file_url(p, f);
                assert!(url.starts_with("https://huggingface.co/"), "{url}");
                assert!(url.contains(p.commit), "{url}");
                assert!(url.ends_with(f.name), "{url}");
                assert!(
                    eg2::is_pinned(f.sha256)
                        || llm::is_pinned(f.sha256)
                        || stt::is_pinned(f.sha256)
                        || tts::is_pinned(f.sha256),
                    "hash for {} must be pinned",
                    f.name
                );
                sum += f.size_bytes;
            }
            assert_eq!(sum, p.total_bytes());
        }
    }

    #[test]
    fn downloads_verifies_tracks_progress_and_skips_valid_existing_files() {
        let body_a = b"model-weights-a".repeat(256);
        let body_b = b"mmproj-weights-b".repeat(128);
        let sha_a = Box::leak(sha256_hex(&body_a).into_boxed_str());
        let sha_b = Box::leak(sha256_hex(&body_b).into_boxed_str());

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let a_clone = body_a.clone();
        let b_clone = body_b.clone();

        let srv = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut s, _) = listener.accept().unwrap();
                let req = read_http_head(&mut s);
                let payload = if req.contains("/a.gguf") {
                    &a_clone
                } else {
                    &b_clone
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    payload.len()
                );
                s.write_all(head.as_bytes()).unwrap();
                s.write_all(payload).unwrap();
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let items = vec![
            (
                format!("http://{addr}/a.gguf"),
                PinnedFile {
                    name: "a.gguf",
                    size_bytes: body_a.len() as u64,
                    sha256: sha_a,
                },
            ),
            (
                format!("http://{addr}/b.gguf"),
                PinnedFile {
                    name: "b.gguf",
                    size_bytes: body_b.len() as u64,
                    sha256: sha_b,
                },
            ),
        ];

        let mut events = Vec::new();
        let out = download_files(
            &items,
            dir.path(),
            true,
            &AtomicBool::new(false),
            &mut |p| events.push(p),
        )
        .unwrap();
        assert_eq!(out, dir.path());
        srv.join().unwrap();

        assert_eq!(fs::read(dir.path().join("a.gguf")).unwrap(), body_a);
        assert_eq!(fs::read(dir.path().join("b.gguf")).unwrap(), body_b);
        assert!(!dir.path().join("a.gguf.part").exists());
        assert!(!dir.path().join("b.gguf.part").exists());
        assert_eq!(events.last().unwrap().phase, DownloadPhase::Done);
        assert_eq!(
            events.last().unwrap().bytes_done,
            (body_a.len() + body_b.len()) as u64
        );

        // Second call sees both files verified on disk and makes zero HTTP calls.
        let mut events2 = Vec::new();
        download_files(
            &items,
            dir.path(),
            true,
            &AtomicBool::new(false),
            &mut |p| events2.push(p),
        )
        .unwrap();
        assert!(!events2
            .iter()
            .any(|e| e.phase == DownloadPhase::Downloading));
        assert_eq!(events2.last().unwrap().phase, DownloadPhase::Done);
    }

    #[test]
    fn checksum_and_size_mismatches_remove_part_file() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let bad_payload = b"corrupted-model-bytes".to_vec();
        let wrong_sha = Box::leak(sha256_hex(b"expected-different").into_boxed_str());
        let ok_sha = Box::leak(sha256_hex(&bad_payload).into_boxed_str());

        let srv = std::thread::spawn(move || {
            for i in 0..2 {
                let (mut s, _) = listener.accept().unwrap();
                let _ = read_http_head(&mut s);
                let data: &[u8] = if i == 0 {
                    &bad_payload
                } else {
                    b"much-too-long-payload-for-expected-size"
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len()
                );
                s.write_all(head.as_bytes()).unwrap();
                s.write_all(data).unwrap();
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let err1 = download_files(
            &[(
                format!("http://{addr}/model.gguf"),
                PinnedFile {
                    name: "model.gguf",
                    size_bytes: b"corrupted-model-bytes".len() as u64,
                    sha256: wrong_sha,
                },
            )],
            dir.path(),
            true,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(err1, DownloadError::ChecksumMismatch);
        assert!(!dir.path().join("model.gguf").exists());
        assert!(!dir.path().join("model.gguf.part").exists());

        let err2 = download_files(
            &[(
                format!("http://{addr}/model.gguf"),
                PinnedFile {
                    name: "model.gguf",
                    size_bytes: 5,
                    sha256: ok_sha,
                },
            )],
            dir.path(),
            true,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(err2, DownloadError::SizeMismatch);
        assert!(!dir.path().join("model.gguf.part").exists());
        srv.join().unwrap();
    }

    #[test]
    fn cancel_mid_download_cleans_temp_file() {
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let srv = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let _ = read_http_head(&mut s);
            let total = 256 * 1024usize;
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n");
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&vec![b'x'; 80 * 1024]);
            std::thread::sleep(Duration::from_millis(80));
            let _ = s.write_all(&vec![b'x'; 80 * 1024]);
        });

        let dir = tempfile::tempdir().unwrap();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            c2.store(true, Ordering::SeqCst);
        });

        let err = download_files(
            &[(
                format!("http://{addr}/slow.gguf"),
                PinnedFile {
                    name: "slow.gguf",
                    size_bytes: 256 * 1024,
                    sha256: "00",
                },
            )],
            dir.path(),
            true,
            &cancel,
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(err, DownloadError::Cancelled);
        assert!(!dir.path().join("slow.gguf").exists());
        assert!(!dir.path().join("slow.gguf.part").exists());
        srv.join().unwrap();
    }

    #[test]
    fn rejects_unsafe_redirects_and_loops() {
        assert_eq!(
            resolve_redirect(
                "https://huggingface.co/org/repo/resolve/abc/f.gguf",
                "http://cdn.example.com/f.gguf",
                false
            ),
            Err(DownloadError::UnsafeRedirect)
        );
        assert_eq!(
            resolve_redirect(
                "https://huggingface.co/org/repo/resolve/abc/f.gguf",
                "file:///etc/passwd",
                false
            ),
            Err(DownloadError::UnsafeRedirect)
        );
        assert_eq!(
            resolve_redirect(
                "https://huggingface.co/org/repo/resolve/abc/f.gguf",
                "/cdn/blob",
                false
            )
            .unwrap(),
            "https://huggingface.co/cdn/blob"
        );

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = std::thread::spawn(move || {
            for _ in 0..=MAX_REDIRECTS {
                let (mut s, _) = listener.accept().unwrap();
                let _ = read_http_head(&mut s);
                let head = format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://{addr}/loop\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = s.write_all(head.as_bytes());
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let err = download_files(
            &[(
                format!("http://{addr}/start"),
                PinnedFile {
                    name: "m.gguf",
                    size_bytes: 1,
                    sha256: "00",
                },
            )],
            dir.path(),
            true,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(err, DownloadError::TooManyRedirects);
        srv.join().unwrap();
    }
}
