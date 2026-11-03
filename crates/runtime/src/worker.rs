//! Launching a worker process and waiting until it is healthy.
//!
//! The worker binds 127.0.0.1 on a free port and requires a random bearer
//! token (docs/05 "Workers bind 127.0.0.1 only ... per-process token").

use std::ffi::OsString;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::client::{Client, Endpoint};
use crate::error::RuntimeError;
use crate::process::Supervised;

/// A running, healthy worker.
pub trait Worker: Send + Sync {
    fn endpoint(&self) -> &Endpoint;
    /// `Some(reason)` if the process is gone.
    fn exit_reason(&self) -> Option<String>;
    /// Stop and block until the OS reports the process gone.
    fn terminate(&self) -> Result<(), RuntimeError>;
    fn pid(&self) -> Option<u32>;
}

/// Starts workers for one registered model.
pub trait Launcher: Send + Sync {
    /// Blocks until the worker is healthy, failed, or `cancel` is set.
    fn launch(&self, cancel: &AtomicBool) -> Result<Arc<dyn Worker>, RuntimeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuLayers {
    /// llama.cpp decides (`-ngl auto`, its default).
    Auto,
    /// Everything on the GPU.
    All,
    /// CPU only.
    None,
}

#[derive(Debug, Clone)]
pub struct LlamaServerConfig {
    pub exe: PathBuf,
    pub model: PathBuf,
    pub mmproj: Option<PathBuf>,
    /// `--embeddings`: embedding-only server (EG2).
    pub embeddings: bool,
    /// Context size (tokens).
    pub ctx: u32,
    /// Logical and physical batch size. EG2 is non-causal, so one input must
    /// fit in a single ubatch (= ctx). `None` = llama.cpp defaults (chat).
    pub batch: Option<u32>,
    /// `--jinja`: format chat requests with the model's own template.
    pub jinja: bool,
    pub gpu_layers: GpuLayers,
    pub threads: Option<u32>,
    pub startup_timeout: Duration,
    pub request_timeout: Duration,
}

impl LlamaServerConfig {
    pub fn eg2(exe: PathBuf, model: PathBuf, mmproj: Option<PathBuf>) -> Self {
        Self {
            exe,
            model,
            mmproj,
            embeddings: true,
            // The model's documented max sequence length.
            ctx: 8192,
            batch: Some(8192),
            jinja: false,
            gpu_layers: GpuLayers::Auto,
            threads: None,
            startup_timeout: Duration::from_secs(180),
            request_timeout: Duration::from_secs(120),
        }
    }

    /// Small instruction model: search plans, result picks and (with a
    /// projector) image descriptions (docs/08, ADR 021). Prompts are short,
    /// so a small context keeps memory low.
    pub fn chat(exe: PathBuf, model: PathBuf, mmproj: Option<PathBuf>) -> Self {
        Self {
            exe,
            model,
            mmproj,
            embeddings: false,
            ctx: 4096,
            batch: None,
            jinja: true,
            gpu_layers: GpuLayers::Auto,
            threads: None,
            startup_timeout: Duration::from_secs(180),
            request_timeout: Duration::from_secs(30),
        }
    }
}

/// Arguments for `llama-server`. Pure, so it is unit tested.
pub fn llama_server_args(cfg: &LlamaServerConfig, port: u16, token: &str) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![
        "--model".into(),
        cfg.model.clone().into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string().into(),
        "--api-key".into(),
        token.into(),
        "--ctx-size".into(),
        cfg.ctx.to_string().into(),
        "--parallel".into(),
        "1".into(),
        "--no-webui".into(),
    ];
    if let Some(b) = cfg.batch {
        a.push("--batch-size".into());
        a.push(b.to_string().into());
        a.push("--ubatch-size".into());
        a.push(b.to_string().into());
    }
    if cfg.jinja {
        a.push("--jinja".into());
    }
    if let Some(mm) = &cfg.mmproj {
        a.push("--mmproj".into());
        a.push(mm.clone().into());
    }
    if cfg.embeddings {
        a.push("--embeddings".into());
    }
    match cfg.gpu_layers {
        GpuLayers::Auto => {}
        GpuLayers::All => {
            a.push("--n-gpu-layers".into());
            a.push("all".into());
        }
        GpuLayers::None => {
            a.push("--n-gpu-layers".into());
            a.push("0".into());
            a.push("--no-mmproj-offload".into());
        }
    }
    if let Some(t) = cfg.threads {
        a.push("--threads".into());
        a.push(t.to_string().into());
    }
    a
}

pub fn random_token() -> Result<String, RuntimeError> {
    let mut b = [0u8; 24];
    getrandom::fill(&mut b).map_err(|e| RuntimeError::LoadFailed(format!("rng: {e}")))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

pub fn free_port() -> Result<u16, RuntimeError> {
    TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| RuntimeError::LoadFailed(format!("no free port: {e}")))
}

/// Builds the command for a given port/token. Lets tests launch a stand-in
/// process through the exact same supervision path.
pub type CommandFactory = dyn Fn(u16, &str) -> Command + Send + Sync;

pub struct ProcessLauncher {
    factory: Box<CommandFactory>,
    startup_timeout: Duration,
    request_timeout: Duration,
}

pub struct ProcessWorker {
    proc: Supervised,
    endpoint: Endpoint,
}

impl Worker for ProcessWorker {
    fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
    fn exit_reason(&self) -> Option<String> {
        self.proc.exit_reason()
    }
    fn terminate(&self) -> Result<(), RuntimeError> {
        self.proc.terminate(Duration::from_millis(1500))
    }
    fn pid(&self) -> Option<u32> {
        Some(self.proc.pid())
    }
}

impl ProcessWorker {
    pub fn log_tail(&self, n: usize) -> Vec<String> {
        self.proc.log_tail(n)
    }
}

impl ProcessLauncher {
    pub fn new(
        factory: Box<CommandFactory>,
        startup_timeout: Duration,
        request_timeout: Duration,
    ) -> Self {
        Self {
            factory,
            startup_timeout,
            request_timeout,
        }
    }

    pub fn llama_server(cfg: LlamaServerConfig) -> Self {
        let (st, rt) = (cfg.startup_timeout, cfg.request_timeout);
        Self::new(
            Box::new(move |port, token| {
                let mut c = Command::new(&cfg.exe);
                c.args(llama_server_args(&cfg, port, token));
                c
            }),
            st,
            rt,
        )
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    /// Launch and return the concrete worker (the spike tool wants its log).
    pub fn launch_process(&self, cancel: &AtomicBool) -> Result<ProcessWorker, RuntimeError> {
        // A port can be taken between probing and binding; retry once.
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.try_launch(cancel) {
                Err((RuntimeError::WorkerExited(msg), true)) if attempt < 3 => {
                    let _ = msg;
                    continue;
                }
                r => return r.map_err(|(e, _)| e),
            }
        }
    }

    /// Error side carries "was a port clash" for the retry above.
    fn try_launch(&self, cancel: &AtomicBool) -> Result<ProcessWorker, (RuntimeError, bool)> {
        let port = free_port().map_err(|e| (e, false))?;
        let token = random_token().map_err(|e| (e, false))?;
        let proc = Supervised::spawn((self.factory)(port, &token)).map_err(|e| (e, false))?;
        let endpoint = Endpoint {
            base_url: format!("http://127.0.0.1:{port}"),
            token,
        };
        let worker = ProcessWorker { proc, endpoint };
        let client = Client::new(worker.endpoint.clone(), Duration::from_secs(2));
        let deadline = Instant::now() + self.startup_timeout;
        loop {
            if let Some(reason) = worker.exit_reason() {
                // Give the log drain a moment to catch the last lines.
                std::thread::sleep(Duration::from_millis(100));
                let tail = worker.log_tail(12).join("\n");
                let clash = tail.contains("bind") || tail.contains("address already in use");
                return Err((
                    RuntimeError::WorkerExited(format!("{reason}\n{tail}")),
                    clash,
                ));
            }
            if cancel.load(Ordering::SeqCst) {
                let _ = worker.terminate();
                return Err((RuntimeError::Cancelled, false));
            }
            if Instant::now() > deadline {
                let _ = worker.terminate();
                return Err((RuntimeError::LoadTimeout, false));
            }
            if let Ok(true) = client.health() {
                return Ok(worker);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Launcher for ProcessLauncher {
    fn launch(&self, cancel: &AtomicBool) -> Result<Arc<dyn Worker>, RuntimeError> {
        Ok(Arc::new(self.launch_process(cancel)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> LlamaServerConfig {
        LlamaServerConfig::eg2(
            "llama-server".into(),
            "m.gguf".into(),
            Some("p.gguf".into()),
        )
    }

    fn strs(a: &[OsString]) -> Vec<String> {
        a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn args_bind_localhost_with_token_and_embeddings() {
        let a = strs(&llama_server_args(&cfg(), 4567, "tok"));
        let pair = |k: &str| a.iter().position(|x| x == k).map(|i| a[i + 1].clone());
        assert_eq!(pair("--host").as_deref(), Some("127.0.0.1"));
        assert_eq!(pair("--port").as_deref(), Some("4567"));
        assert_eq!(pair("--api-key").as_deref(), Some("tok"));
        assert_eq!(pair("--mmproj").as_deref(), Some("p.gguf"));
        assert_eq!(pair("--ubatch-size").as_deref(), Some("8192"));
        assert!(a.contains(&"--embeddings".to_string()));
        assert!(a.contains(&"--no-webui".to_string()));
        assert!(
            !a.contains(&"--n-gpu-layers".to_string()),
            "auto = llama.cpp default"
        );
    }

    #[test]
    fn cpu_only_keeps_projector_off_gpu() {
        let mut c = cfg();
        c.gpu_layers = GpuLayers::None;
        c.mmproj = None;
        let a = strs(&llama_server_args(&c, 1, "t"));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--n-gpu-layers" && w[1] == "0"));
        assert!(!a.contains(&"--mmproj".to_string()));
    }

    #[test]
    fn chat_worker_uses_template_and_default_batches() {
        let c = LlamaServerConfig::chat("llama-server".into(), "llm.gguf".into(), None);
        let a = strs(&llama_server_args(&c, 9, "t"));
        assert!(a.contains(&"--jinja".to_string()));
        assert!(!a.contains(&"--embeddings".to_string()));
        assert!(!a.contains(&"--ubatch-size".to_string()));
        assert!(!a.contains(&"--mmproj".to_string()));
        assert!(a
            .windows(2)
            .any(|w| w[0] == "--host" && w[1] == "127.0.0.1"));
        assert!(a.windows(2).any(|w| w[0] == "--ctx-size" && w[1] == "4096"));
    }

    #[test]
    fn tokens_are_random_and_long() {
        let (a, b) = (random_token().unwrap(), random_token().unwrap());
        assert_eq!(a.len(), 48);
        assert_ne!(a, b);
    }
}
