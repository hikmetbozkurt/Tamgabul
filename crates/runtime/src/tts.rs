//! Piper text-to-speech (TTS) profile, CLI wrapper, and loopback bridge (P6, V04).
//!
//! Static validation verifies the `.onnx` + `.onnx.json` voice profile without
//! starting any process. Runtime synthesis spawns the external `piper` CLI
//! (`-m <model.onnx> -c <model.onnx.json> --output_raw`) on demand behind a
//! loopback bearer-authenticated HTTP worker managed by [`ModelManager`].
//! Zero temporary audio files are written to disk, and active `piper`
//! synthesis can be interrupted cleanly via `POST /cancel` or `terminate()`.

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::client::{ensure_loopback, loopback_agent, map_ureq_err, Endpoint};
use crate::eg2::PinnedFile;
use crate::error::RuntimeError;
use crate::process::hide_console;
use crate::worker::{free_port, random_token, Launcher, Worker};

/// `rhasspy/piper-voices` Hugging Face repo & verified commit (2026-11-19).
/// Note: repository tag is `mit`, while individual voice `MODEL_CARD`s carry
/// dataset-specific licenses (see V04 in docs/19_RESEARCH_VERIFICATION.md).
pub const PINNED_REPO: &str = "rhasspy/piper-voices";
pub const PINNED_COMMIT: &str = "04ca88f5685ca5253c701af3687e155b113bd512";

pub const TR_DFKI_PREFIX: &str = "tr/tr_TR/dfki/medium/";
pub const EN_LJSPEECH_PREFIX: &str = "en/en_US/ljspeech/medium/";

pub const TR_DFKI_ONNX: PinnedFile = PinnedFile {
    name: "tr_TR-dfki-medium.onnx",
    size_bytes: 63_201_294,
    sha256: "2844717f524ab965d3fe86e60562cbb601d3e456836efcc2196cc3a14112a8fb",
};
pub const TR_DFKI_JSON: PinnedFile = PinnedFile {
    name: "tr_TR-dfki-medium.onnx.json",
    size_bytes: 4_960,
    sha256: "13ebd7810f1b61b5027583cf3131a0a233b6ea81c38f2200ebc4ff41c3cca039",
};

pub const EN_LJSPEECH_ONNX: PinnedFile = PinnedFile {
    name: "en_US-ljspeech-medium.onnx",
    size_bytes: 63_531_379,
    sha256: "6f52a751e2349abe7a76735eb09dc1875298c77ea2342ffd2fef79ff81b87f22",
};
pub const EN_LJSPEECH_JSON: PinnedFile = PinnedFile {
    name: "en_US-ljspeech-medium.onnx.json",
    size_bytes: 4_972,
    sha256: "141d612cc0a95ed7efc1ca936b845c2364967f2e9217c5dbfcf69fc4d6c65860",
};

pub const PINNED_SHA256: &[(&str, &str)] = &[
    (TR_DFKI_ONNX.name, TR_DFKI_ONNX.sha256),
    (TR_DFKI_JSON.name, TR_DFKI_JSON.sha256),
    (EN_LJSPEECH_ONNX.name, EN_LJSPEECH_ONNX.sha256),
    (EN_LJSPEECH_JSON.name, EN_LJSPEECH_JSON.sha256),
];

pub fn is_pinned(sha256: &str) -> bool {
    PINNED_SHA256
        .iter()
        .any(|(_, h)| h.eq_ignore_ascii_case(sha256))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsFiles {
    pub model: PathBuf,
    pub config: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsProbe {
    pub sample_rate: u32,
    pub num_speakers: u32,
    pub language_code: Option<String>,
    pub voice_name: String,
}

fn sibling_json_for_onnx(onnx: &Path) -> PathBuf {
    let mut os = onnx.as_os_str().to_os_string();
    os.push(".json");
    PathBuf::from(os)
}

/// Locate `.onnx` and sibling `.onnx.json` from a file or folder path.
pub fn locate(path: &Path) -> Result<TtsFiles, RuntimeError> {
    if !path.exists() {
        return Err(RuntimeError::ModelMissing);
    }
    if path.is_file() {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if name.to_ascii_lowercase().ends_with(".onnx.json") {
            let onnx = path.with_extension("");
            if !onnx.is_file() {
                return Err(RuntimeError::ModelMissing);
            }
            return Ok(TtsFiles {
                model: onnx,
                config: path.to_path_buf(),
            });
        }
        if name.to_ascii_lowercase().ends_with(".onnx") {
            let json = sibling_json_for_onnx(path);
            if !json.is_file() {
                return Err(RuntimeError::InvalidModelFile(
                    "voice_profile_missing".into(),
                ));
            }
            return Ok(TtsFiles {
                model: path.to_path_buf(),
                config: json,
            });
        }
        return Err(RuntimeError::Incompatible(
            "expected a Piper .onnx voice model".into(),
        ));
    }

    let rd = std::fs::read_dir(path).map_err(|e| RuntimeError::Io(format!("read folder: {e}")))?;
    let mut onnx_files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("onnx"))
        })
        .collect();
    onnx_files.sort();
    let Some(model) = onnx_files.into_iter().next() else {
        return Err(RuntimeError::Incompatible(
            "no .onnx voice model found in folder".into(),
        ));
    };
    let config = sibling_json_for_onnx(&model);
    if !config.is_file() {
        return Err(RuntimeError::InvalidModelFile(
            "voice_profile_missing".into(),
        ));
    }
    Ok(TtsFiles { model, config })
}

/// Static validation of `.onnx` (binary non-text sanity check) + `.onnx.json`.
pub fn probe(files: &TtsFiles) -> Result<TtsProbe, RuntimeError> {
    if !files.model.is_file() {
        return Err(RuntimeError::ModelMissing);
    }
    if !files.config.is_file() {
        return Err(RuntimeError::InvalidModelFile(
            "voice_profile_missing".into(),
        ));
    }

    let mut f = File::open(&files.model).map_err(|e| RuntimeError::Io(format!("open: {e}")))?;
    let meta = f
        .metadata()
        .map_err(|e| RuntimeError::Io(format!("stat: {e}")))?;
    if meta.len() < 16 {
        return Err(RuntimeError::InvalidModelFile(
            "ONNX voice file is empty or truncated".into(),
        ));
    }
    let mut prefix = [0u8; 16];
    let _ = f.read(&mut prefix);
    let trimmed_start = prefix
        .iter()
        .copied()
        .find(|b| !b.is_ascii_whitespace())
        .unwrap_or(0);
    if matches!(trimmed_start, b'{' | b'<')
        || prefix.starts_with(b"GGUF")
        || prefix.starts_with(b"PK\x03\x04")
    {
        return Err(RuntimeError::InvalidModelFile(
            "file is not an ONNX protobuf model".into(),
        ));
    }

    let cfg_bytes =
        std::fs::read(&files.config).map_err(|e| RuntimeError::Io(format!("read profile: {e}")))?;
    let v: Value = serde_json::from_slice(&cfg_bytes)
        .map_err(|_| RuntimeError::InvalidModelFile("voice_profile_missing".into()))?;
    let sample_rate = v["audio"]["sample_rate"]
        .as_u64()
        .and_then(|r| u32::try_from(r).ok())
        .filter(|&r| (8_000..=48_000).contains(&r))
        .ok_or_else(|| RuntimeError::Incompatible("missing audio.sample_rate in profile".into()))?;
    let phonemes = v["phoneme_id_map"]
        .as_object()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| RuntimeError::Incompatible("missing phoneme_id_map in profile".into()))?;
    let _ = phonemes;
    let num_speakers = v["num_speakers"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(1)
        .max(1);
    let language_code = v["language"]["code"]
        .as_str()
        .or_else(|| v["espeak"]["voice"].as_str())
        .map(str::to_owned);
    let stem = files
        .model
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Piper");
    Ok(TtsProbe {
        sample_rate,
        num_speakers,
        language_code,
        voice_name: format!("Piper ({stem})"),
    })
}

/// Wrap raw 16-bit LE mono PCM bytes in a 44-byte RIFF/WAVE container.
pub fn pcm16_to_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() & !1) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&sample_rate.to_le_bytes());
    w.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    w.extend_from_slice(&pcm[..data_len as usize]);
    w
}

/// Extract sample rate from a validated mono/PCM RIFF/WAVE byte slice.
pub fn parse_wav_sample_rate(wav: &[u8]) -> Option<u32> {
    if wav.len() < 44 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    Some(u32::from_le_bytes(wav[24..28].try_into().ok()?))
}

#[derive(Debug, Clone)]
pub struct PiperConfig {
    pub exe: PathBuf,
    pub model: PathBuf,
    pub config: PathBuf,
    pub sample_rate: u32,
    pub request_timeout: Duration,
}

impl PiperConfig {
    pub fn new(exe: PathBuf, model: PathBuf, config: PathBuf, sample_rate: u32) -> Self {
        Self {
            exe,
            model,
            config,
            sample_rate,
            request_timeout: Duration::from_secs(60),
        }
    }
}

/// CLI arguments common to `rhasspy/piper` and `OHF-Voice/piper1-gpl`.
pub fn piper_args(cfg: &PiperConfig) -> Vec<OsString> {
    vec![
        "-m".into(),
        cfg.model.clone().into(),
        "-c".into(),
        cfg.config.clone().into(),
        "--output_raw".into(),
    ]
}

const MAX_PCM_BYTES: usize = 32 * 1024 * 1024;
const MAX_HTTP_BODY_BYTES: usize = 64 * 1024;

#[derive(Default)]
struct PiperRuntimeState {
    active_child: Option<Child>,
    active_pid: Option<u32>,
}

/// Supervised Piper TTS worker exposing loopback `/health`, `/synthesize`, and `/cancel`.
pub struct PiperWorker {
    endpoint: Endpoint,
    stop: Arc<AtomicBool>,
    runtime: Arc<Mutex<PiperRuntimeState>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl PiperWorker {
    pub fn start(cfg: PiperConfig) -> Result<Self, RuntimeError> {
        let port = free_port()?;
        let token = random_token()?;
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| RuntimeError::LoadFailed(format!("bind piper bridge: {e}")))?;
        listener
            .set_nonblocking(false)
            .map_err(|e| RuntimeError::LoadFailed(format!("listener config: {e}")))?;
        let endpoint = Endpoint {
            base_url: format!("http://127.0.0.1:{port}"),
            token: token.clone(),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let runtime = Arc::new(Mutex::new(PiperRuntimeState::default()));

        let stop2 = stop.clone();
        let rt2 = runtime.clone();
        let handle = std::thread::Builder::new()
            .name("tamgabul-piper-bridge".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stop2.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        if stop2.load(Ordering::SeqCst) {
                            break;
                        }
                        let token = token.clone();
                        let cfg = cfg.clone();
                        let stop = stop2.clone();
                        let rt = rt2.clone();
                        std::thread::spawn(move || {
                            handle_bridge_conn(stream, &token, &cfg, &stop, &rt);
                        });
                    }
                }
            })
            .map_err(|e| RuntimeError::LoadFailed(format!("piper thread: {e}")))?;

        Ok(Self {
            endpoint,
            stop,
            runtime,
            thread: Mutex::new(Some(handle)),
        })
    }

    fn kill_active(&self) {
        kill_active_child(&self.runtime);
    }
}

fn kill_active_child(runtime: &Mutex<PiperRuntimeState>) {
    if let Ok(mut rt) = runtime.lock() {
        if let Some(mut c) = rt.active_child.take() {
            let pid = c.id();
            let _ = c.kill();
            let _ = c.wait();
            crate::process::unrecord_pid(pid);
        }
        rt.active_pid = None;
    }
}

fn write_http(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn handle_bridge_conn(
    mut stream: TcpStream,
    token: &str,
    cfg: &PiperConfig,
    stop: &AtomicBool,
    runtime: &Arc<Mutex<PiperRuntimeState>>,
) {
    let Ok(cloned) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(cloned);
    let mut req_line = String::new();
    if reader.read_line(&mut req_line).is_err() {
        return;
    }
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let mut content_len = 0usize;
    let mut auth = String::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.is_empty() || line == "\r\n" {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            if k == "content-length" {
                content_len = v.trim().parse().unwrap_or(0);
            } else if k == "authorization" {
                auth = v.trim().to_string();
            }
        }
    }

    if method == "GET" && path == "/health" {
        write_http(
            &mut stream,
            "200 OK",
            "application/json",
            br#"{"status":"ok"}"#,
        );
        return;
    }
    if auth != format!("Bearer {token}") {
        write_http(
            &mut stream,
            "401 Unauthorized",
            "application/json",
            br#"{"error":"unauthorized"}"#,
        );
        return;
    }
    if method == "POST" && path == "/cancel" {
        kill_active_child(runtime);
        write_http(
            &mut stream,
            "200 OK",
            "application/json",
            br#"{"cancelled":true}"#,
        );
        return;
    }
    if method != "POST" || path != "/synthesize" {
        write_http(
            &mut stream,
            "404 Not Found",
            "application/json",
            br#"{"error":"not found"}"#,
        );
        return;
    }
    if content_len > MAX_HTTP_BODY_BYTES {
        write_http(
            &mut stream,
            "413 Payload Too Large",
            "application/json",
            br#"{"error":"payload too large"}"#,
        );
        return;
    }
    let mut body = vec![0u8; content_len];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let Ok(val) = serde_json::from_slice::<Value>(&body) else {
        write_http(
            &mut stream,
            "400 Bad Request",
            "application/json",
            br#"{"error":"invalid json"}"#,
        );
        return;
    };
    let text = val["text"].as_str().map(str::trim).unwrap_or("");
    if text.is_empty() {
        write_http(
            &mut stream,
            "400 Bad Request",
            "application/json",
            br#"{"error":"empty text"}"#,
        );
        return;
    }

    match synthesize_with_cli(cfg, text, stop, runtime) {
        Ok(wav) => write_http(&mut stream, "200 OK", "audio/wav", &wav),
        Err(RuntimeError::Cancelled) => write_http(
            &mut stream,
            "499 Client Closed Request",
            "application/json",
            br#"{"error":"cancelled"}"#,
        ),
        Err(_) => write_http(
            &mut stream,
            "500 Internal Server Error",
            "application/json",
            br#"{"error":"synthesis failed"}"#,
        ),
    }
}

fn synthesize_with_cli(
    cfg: &PiperConfig,
    text: &str,
    stop: &AtomicBool,
    runtime: &Arc<Mutex<PiperRuntimeState>>,
) -> Result<Vec<u8>, RuntimeError> {
    kill_active_child(runtime);
    let mut cmd = Command::new(&cfg.exe);
    cmd.args(piper_args(cfg))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| RuntimeError::LoadFailed(format!("spawn piper: {e}")))?;
    let pid = child.id();
    crate::process::record_exe_pid(pid, &cfg.exe);

    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    {
        let mut rt = runtime
            .lock()
            .map_err(|_| RuntimeError::Io("piper state poisoned".into()))?;
        rt.active_child = Some(child);
        rt.active_pid = Some(pid);
    }

    if let Some(mut sin) = stdin {
        let _ = sin.write_all(text.as_bytes());
        let _ = sin.write_all(b"\n");
        let _ = sin.flush();
    }

    // Drain stderr continuously so piper cannot block on verbose logs.
    if let Some(serr) = stderr {
        std::thread::spawn(move || {
            let mut sink = [0u8; 1024];
            let mut r = serr;
            while matches!(r.read(&mut sink), Ok(n) if n > 0) {}
        });
    }

    let mut pcm = Vec::new();
    if let Some(mut sout) = stdout {
        let mut buf = [0u8; 8192];
        loop {
            if stop.load(Ordering::SeqCst) {
                kill_active_child(runtime);
                return Err(RuntimeError::Cancelled);
            }
            match sout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let room = MAX_PCM_BYTES.saturating_sub(pcm.len());
                    pcm.extend_from_slice(&buf[..n.min(room)]);
                    if pcm.len() >= MAX_PCM_BYTES {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    }

    let deadline = Instant::now() + cfg.request_timeout;
    let status = loop {
        if stop.load(Ordering::SeqCst) || Instant::now() > deadline {
            kill_active_child(runtime);
            return Err(if stop.load(Ordering::SeqCst) {
                RuntimeError::Cancelled
            } else {
                RuntimeError::Timeout
            });
        }
        let waited = {
            let mut rt = runtime
                .lock()
                .map_err(|_| RuntimeError::Io("piper state poisoned".into()))?;
            match rt.active_child.as_mut() {
                Some(c) => match c.try_wait() {
                    Ok(Some(st)) => {
                        rt.active_child = None;
                        rt.active_pid = None;
                        Some(Ok(st))
                    }
                    Ok(None) => None,
                    Err(e) => {
                        rt.active_child = None;
                        rt.active_pid = None;
                        Some(Err(RuntimeError::Io(format!("wait piper: {e}"))))
                    }
                },
                None => Some(Err(RuntimeError::Cancelled)),
            }
        };
        if let Some(res) = waited {
            crate::process::unrecord_pid(pid);
            break res?;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    if !status.success() {
        return Err(RuntimeError::WorkerExited(status.to_string()));
    }
    if pcm.len() < 2 {
        return Err(RuntimeError::BadResponse(
            "piper produced empty audio".into(),
        ));
    }
    Ok(pcm16_to_wav(&pcm, cfg.sample_rate))
}

impl Worker for PiperWorker {
    fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    fn exit_reason(&self) -> Option<String> {
        self.stop
            .load(Ordering::SeqCst)
            .then(|| "piper stopped".to_string())
    }

    fn terminate(&self) -> Result<(), RuntimeError> {
        self.stop.store(true, Ordering::SeqCst);
        self.kill_active();
        if let Some(addr) = self.endpoint.base_url.strip_prefix("http://") {
            let _ = TcpStream::connect(addr);
        }
        if let Ok(mut guard) = self.thread.lock() {
            if let Some(h) = guard.take() {
                let _ = h.join();
            }
        }
        Ok(())
    }

    fn pid(&self) -> Option<u32> {
        self.runtime.lock().ok().and_then(|rt| rt.active_pid)
    }
}

impl Drop for PiperWorker {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

pub struct PiperLauncher {
    cfg: PiperConfig,
}

impl PiperLauncher {
    pub fn new(cfg: PiperConfig) -> Self {
        Self { cfg }
    }
}

impl Launcher for PiperLauncher {
    fn launch(&self, cancel: &AtomicBool) -> Result<Arc<dyn Worker>, RuntimeError> {
        if cancel.load(Ordering::SeqCst) {
            return Err(RuntimeError::Cancelled);
        }
        if !self.cfg.exe.is_file() {
            return Err(RuntimeError::RuntimeNotFound);
        }
        Ok(Arc::new(PiperWorker::start(self.cfg.clone())?))
    }
}

/// Loopback client for [`PiperWorker`].
pub struct TtsClient {
    ep: Endpoint,
    agent: ureq::Agent,
}

impl TtsClient {
    pub fn new(ep: Endpoint, request_timeout: Duration) -> Self {
        Self {
            ep,
            agent: loopback_agent(request_timeout),
        }
    }

    pub fn synthesize(&self, text: &str) -> Result<Vec<u8>, RuntimeError> {
        ensure_loopback(&self.ep)?;
        let resp = self
            .agent
            .post(&format!("{}/synthesize", self.ep.base_url))
            .set("Authorization", &format!("Bearer {}", self.ep.token))
            .send_json(json!({ "text": text }))
            .map_err(map_ureq_err)?;
        let mut wav = Vec::new();
        resp.into_reader()
            .take((MAX_PCM_BYTES + 44) as u64)
            .read_to_end(&mut wav)
            .map_err(|e| RuntimeError::Io(e.to_string()))?;
        if parse_wav_sample_rate(&wav).is_none() {
            return Err(RuntimeError::BadResponse("invalid WAV returned".into()));
        }
        Ok(wav)
    }

    pub fn cancel(&self) -> Result<(), RuntimeError> {
        ensure_loopback(&self.ep)?;
        let _ = self
            .agent
            .post(&format!("{}/cancel", self.ep.base_url))
            .set("Authorization", &format!("Bearer {}", self.ep.token))
            .call();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_profile(rate: u32) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "audio": { "sample_rate": rate },
            "espeak": { "voice": "tr" },
            "language": { "code": "tr_TR" },
            "num_speakers": 1,
            "phoneme_id_map": { "_": [0], "a": [14] }
        }))
        .unwrap()
    }

    #[test]
    fn locates_and_probes_piper_pair() {
        let dir = tempfile::tempdir().unwrap();
        let onnx = dir.path().join("tr_TR-dfki-medium.onnx");
        let json = dir.path().join("tr_TR-dfki-medium.onnx.json");
        std::fs::write(&onnx, vec![0x08u8; 64]).unwrap();
        std::fs::write(&json, valid_profile(22_050)).unwrap();

        let files = locate(dir.path()).unwrap();
        assert_eq!(files.model, onnx);
        assert_eq!(files.config, json);
        assert_eq!(locate(&onnx).unwrap(), files);
        assert_eq!(locate(&json).unwrap(), files);

        let pr = probe(&files).unwrap();
        assert_eq!(pr.sample_rate, 22_050);
        assert_eq!(pr.num_speakers, 1);
        assert_eq!(pr.language_code.as_deref(), Some("tr_TR"));
    }

    #[test]
    fn missing_profile_or_corrupt_onnx_reports_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let onnx = dir.path().join("voice.onnx");
        std::fs::write(&onnx, vec![0x08u8; 64]).unwrap();
        assert_eq!(
            locate(&onnx),
            Err(RuntimeError::InvalidModelFile(
                "voice_profile_missing".into()
            ))
        );

        let json = dir.path().join("voice.onnx.json");
        std::fs::write(&json, valid_profile(22_050)).unwrap();
        std::fs::write(&onnx, b"{not onnx json").unwrap();
        assert!(matches!(
            probe(&TtsFiles {
                model: onnx.clone(),
                config: json.clone(),
            }),
            Err(RuntimeError::InvalidModelFile(_))
        ));

        std::fs::write(&onnx, vec![0x08u8; 64]).unwrap();
        std::fs::write(&json, br#"{"audio":{}}"#).unwrap();
        assert!(matches!(
            probe(&TtsFiles {
                model: onnx,
                config: json
            }),
            Err(RuntimeError::Incompatible(_))
        ));
    }

    #[test]
    fn pcm16_to_wav_builds_valid_header_and_builds_piper_args() {
        let wav = pcm16_to_wav(&[1, 0, 2, 0, 3], 22_050);
        assert_eq!(parse_wav_sample_rate(&wav), Some(22_050));
        // Odd byte dropped cleanly to keep 16-bit sample alignment
        assert_eq!(wav.len(), 44 + 4);

        let cfg = PiperConfig::new(
            "piper".into(),
            "m.onnx".into(),
            "m.onnx.json".into(),
            22_050,
        );
        let args: Vec<String> = piper_args(&cfg)
            .into_iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec!["-m", "m.onnx", "-c", "m.onnx.json", "--output_raw"]
        );
    }

    #[test]
    fn tts_client_refuses_non_loopback() {
        let c = TtsClient::new(
            Endpoint {
                base_url: "http://192.168.1.20:8080".into(),
                token: "tok".into(),
            },
            Duration::from_secs(1),
        );
        assert!(matches!(c.synthesize("hi"), Err(RuntimeError::Request(_))));
        assert!(matches!(c.cancel(), Err(RuntimeError::Request(_))));
    }
}
