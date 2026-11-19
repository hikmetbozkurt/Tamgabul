//! Whisper.cpp speech-to-text (STT) profile and loopback HTTP client (P6, V04).
//!
//! Static validation reads the Whisper GGML header (`GGML_FILE_MAGIC` =
//! `0x67676d6c` + 11 `int32_t` hyperparams) without launching any model.
//! Runtime transcription talks exclusively to a supervised local
//! `whisper-server` on `127.0.0.1` via `POST /inference` (`multipart/form-data`).

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::client::{ensure_loopback, loopback_agent, map_ureq_err, Endpoint};
use crate::eg2::{file_type_name, PinnedFile};
use crate::error::RuntimeError;

/// `ggerganov/whisper.cpp` Hugging Face repo & verified commit (2026-11-19).
pub const PINNED_REPO: &str = "ggerganov/whisper.cpp";
pub const PINNED_COMMIT: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// `GGML_FILE_MAGIC` from `ggml-org/whisper.cpp` (`0x67676d6c` LE).
pub const GGML_MAGIC: u32 = 0x6767_6d6c;
/// Minimum header bytes: 4-byte magic + 11 x i32 hyperparameters.
const HEADER_BYTES: usize = 4 + 11 * 4;

pub const BASE_MODEL: PinnedFile = PinnedFile {
    name: "ggml-base.bin",
    size_bytes: 147_951_465,
    sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
};
pub const SMALL_Q5_1_MODEL: PinnedFile = PinnedFile {
    name: "ggml-small-q5_1.bin",
    size_bytes: 190_085_487,
    sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
};
pub const SMALL_MODEL: PinnedFile = PinnedFile {
    name: "ggml-small.bin",
    size_bytes: 487_601_967,
    sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
};
pub const LARGE_V3_TURBO_Q5_0_MODEL: PinnedFile = PinnedFile {
    name: "ggml-large-v3-turbo-q5_0.bin",
    size_bytes: 574_041_195,
    sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
};

pub const PINNED_SHA256: &[(&str, &str)] = &[
    (BASE_MODEL.name, BASE_MODEL.sha256),
    (SMALL_Q5_1_MODEL.name, SMALL_Q5_1_MODEL.sha256),
    (SMALL_MODEL.name, SMALL_MODEL.sha256),
    (
        LARGE_V3_TURBO_Q5_0_MODEL.name,
        LARGE_V3_TURBO_Q5_0_MODEL.sha256,
    ),
];

pub fn is_pinned(sha256: &str) -> bool {
    PINNED_SHA256
        .iter()
        .any(|(_, h)| h.eq_ignore_ascii_case(sha256))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SttFiles {
    pub model: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SttProbe {
    pub n_vocab: i32,
    pub n_audio_ctx: i32,
    pub n_audio_layer: i32,
    pub n_text_layer: i32,
    pub n_mels: i32,
    pub ftype_name: String,
    pub size_tier: &'static str,
    pub multilingual: bool,
    pub display_name: String,
}

fn bin_files(dir: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let rd = std::fs::read_dir(dir).map_err(|e| RuntimeError::Io(format!("read folder: {e}")))?;
    let mut files: Vec<(u64, PathBuf)> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("bin")))
        .map(|p| (p.metadata().map(|m| m.len()).unwrap_or(0), p))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    Ok(files.into_iter().map(|(_, p)| p).collect())
}

/// Locate a Whisper GGML `.bin` model from either a file path or a folder.
pub fn locate(path: &Path) -> Result<SttFiles, RuntimeError> {
    if !path.exists() {
        return Err(RuntimeError::ModelMissing);
    }
    if path.is_file() {
        if !path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("bin"))
        {
            return Err(RuntimeError::Incompatible(
                "expected a Whisper GGML .bin model file".into(),
            ));
        }
        return Ok(SttFiles {
            model: path.to_path_buf(),
        });
    }
    for candidate in bin_files(path)? {
        if probe(&candidate).is_ok() {
            return Ok(SttFiles { model: candidate });
        }
    }
    Err(RuntimeError::Incompatible(
        "no Whisper GGML .bin model found in folder".into(),
    ))
}

fn read_i32_le(buf: &[u8], idx: usize) -> i32 {
    let off = 4 + idx * 4;
    i32::from_le_bytes(buf[off..off + 4].try_into().expect("4-byte slice"))
}

/// Read and validate the fixed 48-byte Whisper GGML header without loading weights.
pub fn probe(model: &Path) -> Result<SttProbe, RuntimeError> {
    if !model.is_file() {
        return Err(RuntimeError::ModelMissing);
    }
    let mut f = File::open(model).map_err(|e| RuntimeError::Io(format!("open: {e}")))?;
    let mut buf = [0u8; HEADER_BYTES];
    if f.read_exact(&mut buf).is_err() {
        return Err(RuntimeError::InvalidModelFile(
            "Whisper model header is truncated".into(),
        ));
    }
    let magic = u32::from_le_bytes(buf[0..4].try_into().expect("4-byte magic"));
    if magic != GGML_MAGIC {
        return Err(RuntimeError::InvalidModelFile(
            "invalid Whisper GGML magic header".into(),
        ));
    }
    let n_vocab = read_i32_le(&buf, 0);
    let n_audio_ctx = read_i32_le(&buf, 1);
    let n_audio_state = read_i32_le(&buf, 2);
    let n_audio_head = read_i32_le(&buf, 3);
    let n_audio_layer = read_i32_le(&buf, 4);
    let n_text_ctx = read_i32_le(&buf, 5);
    let n_text_state = read_i32_le(&buf, 6);
    let n_text_head = read_i32_le(&buf, 7);
    let n_text_layer = read_i32_le(&buf, 8);
    let n_mels = read_i32_le(&buf, 9);
    let raw_ftype = read_i32_le(&buf, 10);

    if n_vocab <= 0
        || n_audio_ctx <= 0
        || n_audio_state <= 0
        || n_audio_head <= 0
        || n_audio_layer <= 0
        || n_text_ctx <= 0
        || n_text_state != n_audio_state
        || n_text_head <= 0
        || n_text_layer <= 0
        || !matches!(n_mels, 80 | 128)
        || raw_ftype < 0
    {
        return Err(RuntimeError::Incompatible(
            "unrecognized Whisper hyperparameter layout".into(),
        ));
    }

    let size_tier = match n_audio_layer {
        4 => "tiny",
        6 => "base",
        12 => "small",
        24 => "medium",
        32 if n_text_layer == 4 && n_mels == 128 => "large-v3-turbo",
        32 => "large",
        _ => "whisper",
    };
    let ftype_base = (raw_ftype % 1000) as u64;
    let ftype_name = file_type_name(ftype_base);
    let multilingual = n_vocab >= 51_865;
    let stem = model
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Whisper");

    Ok(SttProbe {
        n_vocab,
        n_audio_ctx,
        n_audio_layer,
        n_text_layer,
        n_mels,
        ftype_name,
        size_tier,
        multilingual,
        display_name: format!("Whisper ({stem})"),
    })
}

#[derive(Debug, Clone)]
pub struct WhisperServerConfig {
    pub exe: PathBuf,
    pub model: PathBuf,
    pub threads: Option<u32>,
    pub no_gpu: bool,
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
}

impl WhisperServerConfig {
    pub fn new(exe: PathBuf, model: PathBuf) -> Self {
        Self {
            exe,
            model,
            threads: None,
            no_gpu: false,
            startup_timeout: Duration::from_secs(90),
            request_timeout: Duration::from_secs(60),
        }
    }
}

/// CLI arguments for `whisper-server` (`examples/server/server.cpp`).
pub fn whisper_server_args(cfg: &WhisperServerConfig, port: u16) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![
        "-m".into(),
        cfg.model.clone().into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string().into(),
        "--inference-path".into(),
        "/inference".into(),
        "-l".into(),
        "auto".into(),
        "-nt".into(),
    ];
    if cfg.no_gpu {
        a.push("-ng".into());
    }
    if let Some(t) = cfg.threads {
        a.push("-t".into());
        a.push(t.to_string().into());
    }
    a
}

/// Tiny 16 kHz mono 16-bit silent WAV for `stt_smoke` validation.
pub fn silent_wav_16khz(duration_ms: u32) -> Vec<u8> {
    let rate = 16_000u32;
    let samples = ((rate as u64 * duration_ms as u64) / 1000).max(160) as u32;
    let data_bytes = samples * 2;
    let mut w = Vec::with_capacity(44 + data_bytes as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_bytes.to_le_bytes());
    w.resize(44 + data_bytes as usize, 0);
    w
}

const BOUNDARY: &str = "----TamgabulWhisperBoundary7f3a9d";

pub fn build_multipart_inference(wav_bytes: &[u8], language: Option<&str>) -> Vec<u8> {
    let mut body = Vec::with_capacity(wav_bytes.len() + 512);
    let mut push_field = |name: &str, value: &str| {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    };
    push_field("temperature", "0.0");
    push_field("response_format", "json");
    if let Some(lang) = language.map(str::trim).filter(|l| !l.is_empty()) {
        push_field("language", lang);
    }
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n",
    );
    body.extend_from_slice(wav_bytes);
    body.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    body
}

pub fn clean_transcript(raw: &str) -> String {
    let mut cleaned = raw.trim().to_string();
    for marker in ["[BLANK_AUDIO]", "[SILENCE]", "(silence)", "(blank audio)"] {
        if cleaned.eq_ignore_ascii_case(marker) {
            return String::new();
        }
        if let Some(idx) = cleaned
            .to_ascii_lowercase()
            .find(&marker.to_ascii_lowercase())
        {
            let mut next = String::with_capacity(cleaned.len());
            next.push_str(&cleaned[..idx]);
            next.push_str(&cleaned[idx + marker.len()..]);
            cleaned = next;
        }
    }
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Loopback client for `whisper-server` `/inference`.
pub struct WhisperClient {
    ep: Endpoint,
    agent: ureq::Agent,
}

impl WhisperClient {
    pub fn new(ep: Endpoint, request_timeout: Duration) -> Self {
        Self {
            ep,
            agent: loopback_agent(request_timeout),
        }
    }

    pub fn transcribe_wav(
        &self,
        wav_bytes: &[u8],
        language: Option<&str>,
    ) -> Result<String, RuntimeError> {
        ensure_loopback(&self.ep)?;
        if wav_bytes.len() < 44 || &wav_bytes[..4] != b"RIFF" || &wav_bytes[8..12] != b"WAVE" {
            return Err(RuntimeError::Request("invalid WAV input".into()));
        }
        let body = build_multipart_inference(wav_bytes, language);
        let url = format!("{}/inference", self.ep.base_url);
        let ctype = format!("multipart/form-data; boundary={BOUNDARY}");
        let v: Value = self
            .agent
            .post(&url)
            .set("Content-Type", &ctype)
            .send_bytes(&body)
            .map_err(map_ureq_err)?
            .into_json()
            .map_err(|e| RuntimeError::BadResponse(e.to_string()))?;
        let text = v["text"]
            .as_str()
            .ok_or_else(|| RuntimeError::BadResponse("missing transcript text field".into()))?;
        Ok(clean_transcript(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    fn ggml_header(n_vocab: i32, n_audio_layer: i32, n_text_layer: i32, n_mels: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&GGML_MAGIC.to_le_bytes());
        let hparams = [
            n_vocab,
            1500,          // n_audio_ctx
            768,           // n_audio_state
            12,            // n_audio_head
            n_audio_layer, // n_audio_layer
            448,           // n_text_ctx
            768,           // n_text_state
            12,            // n_text_head
            n_text_layer,  // n_text_layer
            n_mels,        // n_mels
            9,             // Q5_1
        ];
        for v in hparams {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }

    #[test]
    fn locates_and_probes_valid_whisper_ggml() {
        let dir = tempfile::tempdir().unwrap();
        let turbo = dir.path().join("ggml-large-v3-turbo-q5_0.bin");
        std::fs::write(&turbo, ggml_header(51_866, 32, 4, 128)).unwrap();
        std::fs::write(dir.path().join("not-a-model.txt"), b"hello").unwrap();

        let files = locate(dir.path()).unwrap();
        assert_eq!(files.model, turbo);
        let pr = probe(&files.model).unwrap();
        assert_eq!(pr.size_tier, "large-v3-turbo");
        assert!(pr.multilingual);
        assert_eq!(pr.n_mels, 128);
        assert_eq!(pr.ftype_name, "Q5_1");

        // English-only small model
        let en_small = dir.path().join("ggml-small.en.bin");
        std::fs::write(&en_small, ggml_header(51_864, 12, 12, 80)).unwrap();
        let pr_en = probe(&en_small).unwrap();
        assert_eq!(pr_en.size_tier, "small");
        assert!(!pr_en.multilingual);
        assert_eq!(pr_en.n_mels, 80);
    }

    #[test]
    fn rejects_corrupt_or_wrong_files() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("wrong.gguf");
        std::fs::write(&gguf, b"GGUF0000").unwrap();
        assert!(matches!(locate(&gguf), Err(RuntimeError::Incompatible(_))));

        let bad_magic = dir.path().join("bad.bin");
        std::fs::write(&bad_magic, vec![0u8; 64]).unwrap();
        assert!(matches!(
            probe(&bad_magic),
            Err(RuntimeError::InvalidModelFile(_))
        ));

        let truncated = dir.path().join("tiny.bin");
        std::fs::write(&truncated, GGML_MAGIC.to_le_bytes()).unwrap();
        assert!(matches!(
            probe(&truncated),
            Err(RuntimeError::InvalidModelFile(_))
        ));
    }

    #[test]
    fn builds_localhost_args_and_cleans_blank_markers() {
        let mut cfg = WhisperServerConfig::new("whisper-server".into(), "m.bin".into());
        cfg.no_gpu = true;
        cfg.threads = Some(4);
        let args: Vec<String> = whisper_server_args(&cfg, 9123)
            .into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
        assert!(args.windows(2).any(|w| w == ["--port", "9123"]));
        assert!(args
            .windows(2)
            .any(|w| w == ["--inference-path", "/inference"]));
        assert!(args.windows(2).any(|w| w == ["-l", "auto"]));
        assert!(args.contains(&"-ng".to_string()));
        assert!(args.windows(2).any(|w| w == ["-t", "4"]));

        assert_eq!(clean_transcript("  [BLANK_AUDIO] "), "");
        assert_eq!(
            clean_transcript("  merhaba   dünya [BLANK_AUDIO] "),
            "merhaba dünya"
        );
    }

    #[test]
    fn multipart_transcribe_works_against_loopback_server_and_rejects_remote() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut req_line = String::new();
            r.read_line(&mut req_line).unwrap();
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; len];
            r.read_exact(&mut body).unwrap();
            let reply = r#"{"text":" Toplantı notları "}"#;
            write!(
                s,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                reply.len(),
                reply
            )
            .unwrap();
            (req_line, body)
        });

        let c = WhisperClient::new(
            Endpoint {
                base_url: format!("http://{addr}"),
                token: String::new(),
            },
            Duration::from_secs(5),
        );
        let text = c
            .transcribe_wav(&silent_wav_16khz(200), Some("tr"))
            .unwrap();
        assert_eq!(text, "Toplantı notları");
        let (line, body) = srv.join().unwrap();
        assert!(line.starts_with("POST /inference"));
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("name=\"file\"; filename=\"audio.wav\""));
        assert!(body_str.contains("name=\"language\"\r\n\r\ntr"));

        let remote = WhisperClient::new(
            Endpoint {
                base_url: "https://example.com".into(),
                token: String::new(),
            },
            Duration::from_secs(1),
        );
        assert!(matches!(
            remote.transcribe_wav(&silent_wav_16khz(200), None),
            Err(RuntimeError::Request(_))
        ));
    }
}
