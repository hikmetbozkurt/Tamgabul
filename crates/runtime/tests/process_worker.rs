//! Real child processes through the production supervision path. The child
//! is this test binary re-run as a stand-in server, so no model is needed.

mod support;

use std::net::TcpListener;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::Arc;
use std::time::Duration;

use tamgabul_core::model::ModelStatus;
use tamgabul_runtime::client::{Client, Endpoint};
use tamgabul_runtime::eg2;
use tamgabul_runtime::worker::ProcessLauncher;
use tamgabul_runtime::{ModelManager, Policy, RuntimeError, UnloadOutcome};

/// Entry point of the stand-in server process (skipped in normal runs).
#[test]
#[ignore]
fn fake_server_process() {
    let (Ok(port), Ok(token)) = (std::env::var("FAKE_PORT"), std::env::var("FAKE_TOKEN")) else {
        return;
    };
    let l = TcpListener::bind(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
    support::serve(
        l,
        token,
        Duration::from_millis(300),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU32::new(0)),
    );
}

fn stand_in() -> ProcessLauncher {
    let exe = std::env::current_exe().unwrap();
    ProcessLauncher::new(
        Box::new(move |port, token| {
            let mut c = Command::new(&exe);
            c.args([
                "fake_server_process",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("FAKE_PORT", port.to_string())
            .env("FAKE_TOKEN", token);
            c
        }),
        Duration::from_secs(20),
        Duration::from_secs(10),
    )
}

#[cfg(target_os = "linux")]
fn process_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| !s.contains(") Z"))
        .unwrap_or(false)
}

#[test]
fn worker_process_loads_on_demand_serves_and_exits_on_unload() {
    let m = ModelManager::with_poll(None, Duration::from_millis(50));
    m.register("eg2", Arc::new(stand_in()), Policy::KeepLoaded);
    assert_eq!(
        m.snapshot("eg2").unwrap().pid,
        None,
        "no process before use"
    );

    let lease = m.acquire("eg2").unwrap();
    let pid = lease.pid().unwrap();
    #[cfg(target_os = "linux")]
    assert!(process_alive(pid));

    let c = Client::new(lease.endpoint().clone(), Duration::from_secs(5));
    let props = c.props().unwrap();
    assert!(props.vision && props.audio);
    let q = eg2::query_prompt(eg2::Task::SearchResult, "timer bug");
    let v = c
        .embed_texts(&[q, eg2::document_prompt(None, "doc")])
        .unwrap();
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].len(), eg2::NATIVE_DIM);
    assert!(eg2::is_valid_vector(&v[1]));
    let img = c
        .embed_media(&props.media_marker, &props.media_marker, &[b"\x89PNG fake"])
        .unwrap();
    assert_eq!(img.len(), eg2::NATIVE_DIM);
    assert!(c.embed_media("no marker", "<__media__>", &[b"x"]).is_err());

    // Wrong token is refused: only our process can talk to the worker.
    let bad = Client::new(
        Endpoint {
            base_url: lease.endpoint().base_url.clone(),
            token: "wrong".into(),
        },
        Duration::from_secs(5),
    );
    assert!(matches!(bad.props(), Err(RuntimeError::Request(m)) if m.contains("401")));

    drop(lease);
    assert_eq!(m.unload("eg2").unwrap(), UnloadOutcome::Unloaded);
    let s = m.snapshot("eg2").unwrap();
    assert_eq!(s.status, ModelStatus::Available);
    assert_eq!(s.pid, None);
    #[cfg(target_os = "linux")]
    assert!(!process_alive(pid), "unload must end the OS process");
    let _ = pid;
}

#[test]
fn quit_ends_worker_processes() {
    let m = ModelManager::with_poll(None, Duration::from_millis(50));
    m.register("eg2", Arc::new(stand_in()), Policy::KeepLoaded);
    let pid = m.acquire("eg2").unwrap().pid().unwrap();
    m.shutdown();
    #[cfg(target_os = "linux")]
    assert!(!process_alive(pid));
    let _ = pid;
}

#[cfg(unix)]
#[test]
fn startup_crash_reports_the_runtime_log() {
    let l = ProcessLauncher::new(
        Box::new(|_, _| {
            let mut c = Command::new("sh");
            c.args(["-c", "echo 'error: failed to load model' >&2; exit 1"]);
            c
        }),
        Duration::from_secs(10),
        Duration::from_secs(1),
    );
    match l.launch_process(&AtomicBool::new(false)) {
        Err(RuntimeError::WorkerExited(msg)) => {
            assert!(msg.contains("failed to load model"), "{msg}")
        }
        other => panic!("unexpected: {:?}", other.err()),
    }
}

#[cfg(unix)]
#[test]
fn startup_timeout_terminates_the_process() {
    let l = ProcessLauncher::new(
        Box::new(|_, _| {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        }),
        Duration::from_millis(300),
        Duration::from_secs(1),
    );
    let t = std::time::Instant::now();
    assert_eq!(
        l.launch_process(&AtomicBool::new(false)).err(),
        Some(RuntimeError::LoadTimeout)
    );
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[cfg(unix)]
#[test]
fn cancel_during_startup() {
    let l = ProcessLauncher::new(
        Box::new(|_, _| {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        }),
        Duration::from_secs(30),
        Duration::from_secs(1),
    );
    let cancel = AtomicBool::new(true);
    assert_eq!(
        l.launch_process(&cancel).err(),
        Some(RuntimeError::Cancelled)
    );
}
