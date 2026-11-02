//! P0-02 spike: proves EmbeddingGemma 2 runs through an app-managed
//! llama-server worker, and that the worker really goes away afterwards.
//!
//! Developer tool, not shipped. Prints measured numbers only; paths are
//! reduced to file names so the report can be pasted publicly.
//!
//!   cargo run -p tamgabul-runtime --release --example eg2_spike -- \
//!       --models C:\AI\models\embeddinggemma-2
//!
//! Options:
//!   --server <llama-server.exe>   default: PATH / winget
//!   --image <file>                default: Windows wallpaper img0.jpg if present
//!   --audio <file.wav|mp3>        default: C:\Windows\Media\tada.wav if present
//!   --cpu                         force CPU (-ngl 0)
//!   --no-hash                     skip SHA-256 of the weights
//!   --probe-only                  stop after the static checks
//!   --ttl-secs <n>                idle-unload demo length (default 8)

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tamgabul_core::model::ModelStatus;
use tamgabul_runtime::client::Client;
use tamgabul_runtime::eg2::{self, Task};
use tamgabul_runtime::locate;
use tamgabul_runtime::worker::{GpuLayers, LlamaServerConfig, ProcessLauncher};
use tamgabul_runtime::{ModelManager, Policy, UnloadOutcome};

struct Args {
    models: PathBuf,
    server: Option<PathBuf>,
    image: Option<PathBuf>,
    audio: Option<PathBuf>,
    cpu: bool,
    hash: bool,
    probe_only: bool,
    ttl: u64,
}

fn args() -> Args {
    let mut a = Args {
        models: PathBuf::new(),
        server: None,
        image: None,
        audio: None,
        cpu: false,
        hash: true,
        probe_only: false,
        ttl: 8,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || {
            PathBuf::from(
                it.next()
                    .unwrap_or_else(|| die(&format!("{k} needs a value"))),
            )
        };
        match k.as_str() {
            "--models" => a.models = v(),
            "--server" => a.server = Some(v()),
            "--image" => a.image = Some(v()),
            "--audio" => a.audio = Some(v()),
            "--ttl-secs" => a.ttl = v().to_string_lossy().parse().unwrap_or(8),
            "--cpu" => a.cpu = true,
            "--no-hash" => a.hash = false,
            "--probe-only" => a.probe_only = true,
            other => die(&format!("unknown option {other}")),
        }
    }
    if a.models.as_os_str().is_empty() {
        die("--models <folder with the EmbeddingGemma 2 GGUF files> is required");
    }
    let win = |p: &str| Some(PathBuf::from(p)).filter(|p| p.is_file());
    if a.image.is_none() {
        a.image = win(r"C:\Windows\Web\Wallpaper\Windows\img0.jpg");
    }
    if a.audio.is_none() {
        a.audio = win(r"C:\Windows\Media\tada.wav");
    }
    a
}

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2)
}

fn name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "?".into())
}

fn ms(d: Duration) -> String {
    format!("{:.0} ms", d.as_secs_f64() * 1000.0)
}

fn line(k: &str, v: impl std::fmt::Display) {
    println!("  {k:<26} {v}");
}

/// What the OS says about a process: (alive, memory text). Windows: tasklist.
fn os_process(pid: u32) -> (bool, Option<String>) {
    if cfg!(windows) {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        match out {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).to_string();
                let row = s.lines().find(|l| l.contains(&format!("\"{pid}\"")));
                // "Image","PID","Session","#","Mem Usage"
                let mem = row
                    .and_then(|r| r.rsplit("\",\"").next())
                    .map(|m| m.trim_matches('"').to_owned());
                (row.is_some(), mem)
            }
            Err(_) => (false, None),
        }
    } else {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/status")).ok();
        let alive = stat.is_some();
        let rss = stat.and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS"))
                .map(|l| l[6..].trim().to_owned())
        });
        (alive, rss)
    }
}

/// VRAM used by `pid` according to nvidia-smi, if the tool exists.
fn vram(pid: u32) -> Option<String> {
    let o = Command::new("nvidia-smi")
        .args([
            "--query-compute-apps=pid,used_memory",
            "--format=csv,noheader",
        ])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    let found = s
        .lines()
        .find(|l| l.split(',').next().map(str::trim) == Some(&pid.to_string()))
        .map(|l| l.split(',').nth(1).unwrap_or("").trim().to_owned());
    Some(found.unwrap_or_else(|| "0 (no allocation)".into()))
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn main() {
    let a = args();
    println!("== Tamgabul P0-02 · EmbeddingGemma 2 spike ==");

    // 1. Runtime --------------------------------------------------------
    println!("\n[1] runtime");
    let found = locate::find_llama_server(a.server.as_deref())
        .unwrap_or_else(|_| die("llama-server not found (pass --server <path> or add it to PATH)"));
    line(
        "llama-server",
        format!("{} (found via {:?})", name(&found.path), found.source),
    );
    match locate::version(&found.path) {
        Ok(v) => line("version", v),
        Err(e) => line("version", format!("unknown: {e}")),
    }

    // 2. Files ----------------------------------------------------------
    println!("\n[2] model files");
    let files = eg2::locate(&a.models).unwrap_or_else(|e| die(&format!("model folder: {e}")));
    line("model", name(&files.model));
    line(
        "mmproj",
        files
            .mmproj
            .as_deref()
            .map(name)
            .unwrap_or_else(|| "none (text only)".into()),
    );
    let t = Instant::now();
    let probe = eg2::probe(&files).unwrap_or_else(|e| die(&format!("static check failed: {e}")));
    line("header probe", ms(t.elapsed()));
    line("output dim", probe.output_dim);
    line("precision", &probe.precision);
    line("size label", probe.size_label.as_deref().unwrap_or("-"));
    line("vision encoder", probe.vision);
    line("audio encoder", probe.audio);
    if a.hash {
        for p in std::iter::once(&files.model).chain(files.mmproj.iter()) {
            let t = Instant::now();
            match eg2::sha256_file(p, |_, _| {}) {
                Ok(h) => line(
                    &format!("sha256 {}", name(p)),
                    format!(
                        "{}… {} ({})",
                        &h[..16],
                        if eg2::is_pinned(&h) {
                            "PINNED"
                        } else {
                            "NOT PINNED"
                        },
                        ms(t.elapsed())
                    ),
                ),
                Err(e) => line("sha256", format!("failed: {e}")),
            }
        }
        line("pinned revision", eg2::PINNED_REVISION);
    }
    if a.probe_only {
        println!("\nprobe only: done");
        return;
    }

    // 3. Load on demand -------------------------------------------------
    println!("\n[3] load on demand");
    let mut cfg = LlamaServerConfig::eg2(
        found.path.clone(),
        files.model.clone(),
        files.mmproj.clone(),
    );
    if a.cpu {
        cfg.gpu_layers = GpuLayers::None;
    }
    let launcher = Arc::new(ProcessLauncher::llama_server(cfg.clone()));
    let mgr = ModelManager::new(None);
    mgr.register("eg2", launcher, Policy::KeepLoaded);
    line(
        "status before use",
        format!("{:?}", mgr.snapshot("eg2").unwrap().status),
    );
    let t = Instant::now();
    let lease = mgr
        .acquire("eg2")
        .unwrap_or_else(|e| die(&format!("load failed: {e}")));
    line("cold load", ms(t.elapsed()));
    let pid = lease.pid().unwrap_or(0);
    line("worker pid", pid);
    let c = Client::new(lease.endpoint().clone(), Duration::from_secs(120));
    let props = c.props().unwrap_or_default();
    line(
        "server modalities",
        format!("vision={} audio={}", props.vision, props.audio),
    );
    line("media marker", &props.media_marker);

    // 4. Text -----------------------------------------------------------
    println!("\n[4] text");
    let q = eg2::query_prompt(
        Task::SearchResult,
        "how do I reset the auction countdown timer",
    );
    let docs = [
        ("relevant EN", "The auction countdown timer restarts when a bid arrives in the last 30 seconds. To reset it manually call resetTimer()."),
        ("relevant TR", "Açık artırma geri sayım sayacı son 30 saniyede teklif gelince yeniden başlar. Elle sıfırlamak için resetTimer() çağrılır."),
        ("unrelated", "Preheat the oven to 180 degrees and bake the bread for forty minutes."),
    ];
    let mut inputs = vec![q];
    inputs.extend(
        docs.iter()
            .map(|(_, d)| eg2::document_prompt(Some("notes.md"), d)),
    );
    let t = Instant::now();
    let v = match c.embed_texts(&inputs) {
        Ok(v) => v,
        Err(e) => die(&format!("text embedding failed: {e}")),
    };
    line("batch of 4", ms(t.elapsed()));
    line("dimension", v[0].len());
    let norm: f32 = v[0].iter().map(|x| x * x).sum::<f32>().sqrt();
    line("query L2 norm", format!("{norm:.4}"));
    line(
        "all finite/non-zero",
        v.iter().all(|x| eg2::is_valid_vector(x)),
    );
    let mut ranks: Vec<(f32, &str)> = docs
        .iter()
        .zip(&v[1..])
        .map(|((n, _), d)| (eg2::cosine(&v[0], d), *n))
        .collect();
    for (s, n) in &ranks {
        line(&format!("cos(query, {n})"), format!("{s:.4}"));
    }
    ranks.sort_by(|x, y| y.0.total_cmp(&x.0));
    line(
        "unrelated ranked last",
        if ranks[2].1 == "unrelated" {
            "PASS"
        } else {
            "FAIL"
        },
    );
    let q256 = eg2::truncate(&v[0], 256).unwrap();
    let d256 = eg2::truncate(&v[1], 256).unwrap();
    line(
        "matryoshka 256 cos",
        format!("{:.4}", eg2::cosine(&q256, &d256)),
    );
    let warm: Vec<Duration> = (0..5)
        .map(|_| {
            let t = Instant::now();
            let _ = c.embed_texts(&inputs[1..2]);
            t.elapsed()
        })
        .collect();
    line("warm single doc (median)", ms(median(warm)));

    // 5. Image / audio --------------------------------------------------
    let media =
        |label: &str, path: &Option<PathBuf>, enabled: bool, related: &str, unrelated: &str| {
            println!("\n[{label}]");
            let Some(p) = path else {
                line("result", "SKIPPED (no file; pass a path)");
                return;
            };
            if !enabled {
                line("result", "SKIPPED (server reports no support)");
                return;
            }
            let bytes = std::fs::read(p).unwrap_or_else(|e| die(&format!("read {}: {e}", name(p))));
            line("file", format!("{} ({} KB)", name(p), bytes.len() / 1024));
            let t = Instant::now();
            match c.embed_media(&props.media_marker, &props.media_marker, &[&bytes]) {
                Ok(m) => {
                    line("embed", ms(t.elapsed()));
                    line("dimension", m.len());
                    line("finite/non-zero", eg2::is_valid_vector(&m));
                    let tv = c
                        .embed_texts(&[
                            eg2::query_prompt(Task::SearchResult, related),
                            eg2::query_prompt(Task::SearchResult, unrelated),
                        ])
                        .unwrap_or_default();
                    if tv.len() == 2 {
                        let (r, u) = (eg2::cosine(&tv[0], &m), eg2::cosine(&tv[1], &m));
                        line(&format!("cos('{related}')"), format!("{r:.4}"));
                        line(&format!("cos('{unrelated}')"), format!("{u:.4}"));
                        line("related > unrelated", if r > u { "PASS" } else { "FAIL" });
                    }
                }
                Err(e) => line("result", format!("FAILED: {e}")),
            }
        };
    media(
        "5 image",
        &a.image,
        props.vision,
        "blue abstract desktop wallpaper with light rays",
        "a bowl of soup on a kitchen table",
    );
    media(
        "6 audio",
        &a.audio,
        props.audio,
        "a short musical notification chime",
        "a man giving a long speech",
    );

    // 7. Unload = process gone -----------------------------------------
    println!("\n[7] unload");
    let (alive, mem) = os_process(pid);
    line("worker alive (OS)", alive);
    line("worker memory (OS)", mem.unwrap_or_else(|| "-".into()));
    if let Some(v) = vram(pid) {
        line("worker VRAM (nvidia-smi)", v);
    }
    drop(lease);
    let t = Instant::now();
    let out = mgr.unload("eg2").unwrap();
    line("unload", format!("{out:?} in {}", ms(t.elapsed())));
    line(
        "status after unload",
        format!("{:?}", mgr.snapshot("eg2").unwrap().status),
    );
    let (alive, _) = os_process(pid);
    line("worker alive (OS)", alive);
    line(
        "resources released",
        if !alive {
            "PASS (process exited)"
        } else {
            "FAIL"
        },
    );
    if let Some(v) = vram(pid) {
        line("worker VRAM after", v);
    }

    // 8. Idle timeout demo ---------------------------------------------
    println!("\n[8] idle unload after {} s", a.ttl);
    mgr.set_policy("eg2", Policy::IdleTimeout(Duration::from_secs(a.ttl)))
        .unwrap();
    let pid2 = {
        let l = mgr
            .acquire("eg2")
            .unwrap_or_else(|e| die(&format!("reload failed: {e}")));
        let _ =
            Client::new(l.endpoint().clone(), Duration::from_secs(60)).embed_texts(&inputs[1..2]);
        l.pid().unwrap_or(0)
    };
    line(
        "after request",
        format!("{:?}", mgr.snapshot("eg2").unwrap().status),
    );
    let t = Instant::now();
    while mgr.snapshot("eg2").unwrap().status != ModelStatus::Available {
        if t.elapsed() > Duration::from_secs(a.ttl + 30) {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    line("unloaded by itself after", ms(t.elapsed()));
    let (alive, _) = os_process(pid2);
    line("worker alive (OS)", alive);
    line("idle unload", if !alive { "PASS" } else { "FAIL" });

    mgr.shutdown();
    let _ = UnloadOutcome::Unloaded;
    println!("\n== done ==");
}
