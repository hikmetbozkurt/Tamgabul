//! Test-only stand-in for llama-server (AGENTS.md: fakes live only in tests).
//! Speaks just enough of its HTTP API: /health, /props, /v1/embeddings,
//! /embedding. Vectors are deterministic hashes of the input, not semantics.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tamgabul_runtime::client::Endpoint;
use tamgabul_runtime::worker::{Launcher, Worker};
use tamgabul_runtime::RuntimeError;

pub const DIM: usize = 768;

pub fn fake_vector(seed_text: &str) -> Vec<f32> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed_text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let mut v: Vec<f32> = (0..DIM)
        .map(|_| {
            h ^= h << 13;
            h ^= h >> 7;
            h ^= h << 17;
            ((h >> 11) as f64 / (1u64 << 53) as f64) as f32 - 0.5
        })
        .collect();
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter_mut().for_each(|x| *x /= n);
    v
}

fn respond(s: &mut TcpStream, code: u16, body: &Value) {
    let b = body.to_string();
    let reason = match code {
        200 => "OK",
        401 => "Unauthorized",
        503 => "Service Unavailable",
        _ => "Error",
    };
    let _ = write!(
        s,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    );
}

fn handle(mut s: TcpStream, token: &str, ready_at: Instant, requests: &AtomicU32) {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut line = String::new();
    if r.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or("").to_owned(),
    );
    let mut len = 0usize;
    let mut auth = String::new();
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
            break;
        }
        let lower = h.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("authorization:") {
            auth = h["authorization:".len()..].trim().to_owned();
        }
    }
    let mut body = vec![0u8; len];
    let _ = r.read_exact(&mut body);
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

    if path == "/health" {
        if Instant::now() < ready_at {
            return respond(
                &mut s,
                503,
                &json!({"error":{"code":503,"message":"Loading model"}}),
            );
        }
        return respond(&mut s, 200, &json!({"status":"ok"}));
    }
    if auth != format!("Bearer {token}") {
        return respond(&mut s, 401, &json!({"error":"invalid api key"}));
    }
    requests.fetch_add(1, Ordering::SeqCst);
    match (method, path.as_str()) {
        ("GET", "/props") => respond(
            &mut s,
            200,
            &json!({"modalities":{"vision":true,"audio":true},"media_marker":"<__media__>","build_info":"fake"}),
        ),
        ("POST", "/v1/embeddings") => {
            let data: Vec<Value> = body["input"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .enumerate()
                .map(|(i, t)| json!({"index": i, "embedding": fake_vector(t.as_str().unwrap_or(""))}))
                .collect();
            respond(&mut s, 200, &json!({ "data": data }))
        }
        ("POST", "/embedding") => {
            let c = &body["content"];
            let seed = format!("{}|{}", c["prompt_string"], c["multimodal_data"]);
            respond(
                &mut s,
                200,
                &json!([{"index":0,"embedding":[fake_vector(&seed)]}]),
            )
        }
        _ => respond(&mut s, 404, &json!({"error":"not found"})),
    }
}

/// Serve on `listener` until `stop` is set.
pub fn serve(
    listener: TcpListener,
    token: String,
    load_time: Duration,
    stop: Arc<AtomicBool>,
    requests: Arc<AtomicU32>,
) {
    listener.set_nonblocking(true).unwrap();
    let ready_at = Instant::now() + load_time;
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_nonblocking(false).unwrap();
                let (t, r) = (token.clone(), requests.clone());
                std::thread::spawn(move || handle(s, &t, ready_at, &r));
            }
            Err(_) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// In-process worker: a server thread instead of a process.
pub struct ThreadWorker {
    endpoint: Endpoint,
    stop: Arc<AtomicBool>,
    pub requests: Arc<AtomicU32>,
    dead: Arc<Mutex<Option<String>>>,
}

impl ThreadWorker {
    /// Simulate the worker crashing on its own.
    pub fn crash(&self) {
        self.stop.store(true, Ordering::SeqCst);
        *self.dead.lock().unwrap() = Some("exit code: 139".into());
    }
}

impl Worker for ThreadWorker {
    fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }
    fn exit_reason(&self) -> Option<String> {
        self.dead.lock().unwrap().clone()
    }
    fn terminate(&self) -> Result<(), RuntimeError> {
        self.stop.store(true, Ordering::SeqCst);
        let mut d = self.dead.lock().unwrap();
        if d.is_none() {
            *d = Some("terminated".into());
        }
        Ok(())
    }
    fn pid(&self) -> Option<u32> {
        None
    }
}

#[derive(Default)]
pub struct Counters {
    pub launches: AtomicU32,
    pub live: AtomicU32,
}

/// Launcher with a configurable load time and failure mode.
pub struct FakeLauncher {
    pub load_time: Duration,
    pub fail: Mutex<Option<RuntimeError>>,
    pub counters: Arc<Counters>,
    pub workers: Mutex<Vec<Arc<ThreadWorker>>>,
}

impl FakeLauncher {
    pub fn new(load_time: Duration) -> Arc<Self> {
        Arc::new(Self {
            load_time,
            fail: Mutex::new(None),
            counters: Arc::new(Counters::default()),
            workers: Mutex::new(Vec::new()),
        })
    }

    /// Workers whose process would still be alive.
    pub fn alive(&self) -> usize {
        self.workers
            .lock()
            .unwrap()
            .iter()
            .filter(|w| w.exit_reason().is_none())
            .count()
    }

    pub fn last(&self) -> Arc<ThreadWorker> {
        self.workers.lock().unwrap().last().unwrap().clone()
    }
}

impl Launcher for FakeLauncher {
    fn launch(&self, cancel: &AtomicBool) -> Result<Arc<dyn Worker>, RuntimeError> {
        self.counters.launches.fetch_add(1, Ordering::SeqCst);
        let until = Instant::now() + self.load_time;
        while Instant::now() < until {
            if cancel.load(Ordering::SeqCst) {
                return Err(RuntimeError::Cancelled);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(e) = self.fail.lock().unwrap().clone() {
            return Err(e);
        }
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicU32::new(0));
        let token = "test-token".to_string();
        {
            let (stop, requests, token) = (stop.clone(), requests.clone(), token.clone());
            std::thread::spawn(move || serve(listener, token, Duration::ZERO, stop, requests));
        }
        let w = Arc::new(ThreadWorker {
            endpoint: Endpoint {
                base_url: format!("http://127.0.0.1:{port}"),
                token,
            },
            stop,
            requests,
            dead: Arc::new(Mutex::new(None)),
        });
        self.workers.lock().unwrap().push(w.clone());
        Ok(w)
    }
}
