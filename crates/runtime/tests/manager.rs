//! Lifecycle tests (docs/05 "Tests"): on-demand load, idle unload, shared
//! load, unload-on-busy, stale READY, crash, shutdown, policy change.

mod support;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use support::FakeLauncher;
use tamgabul_core::model::ModelStatus;
use tamgabul_runtime::client::Client;
use tamgabul_runtime::manager::Listener;
use tamgabul_runtime::{ModelManager, ModelSnapshot, Policy, RuntimeError, UnloadOutcome};

const ID: &str = "eg2";

fn mgr() -> ModelManager {
    ModelManager::with_poll(None, Duration::from_millis(20))
}

fn status(m: &ModelManager) -> ModelStatus {
    m.snapshot(ID).unwrap().status
}

fn wait_for(m: &ModelManager, want: ModelStatus, within: Duration) {
    let until = Instant::now() + within;
    while status(m) != want {
        assert!(
            Instant::now() < until,
            "expected {want:?}, still {:?}",
            status(m)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn nothing_runs_until_work_needs_it() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(ID, l.clone(), Policy::default());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(status(&m), ModelStatus::Available);
    assert_eq!(l.counters.launches.load(Ordering::SeqCst), 0);
}

#[test]
fn idle_timeout_stops_the_worker() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::from_millis(30));
    m.register(
        ID,
        l.clone(),
        Policy::IdleTimeout(Duration::from_millis(200)),
    );

    let lease = m.acquire(ID).unwrap();
    assert_eq!(status(&m), ModelStatus::Busy);
    let v = Client::new(lease.endpoint().clone(), Duration::from_secs(5))
        .embed_texts(&["hello".into()])
        .unwrap();
    assert_eq!(v[0].len(), support::DIM);
    drop(lease);

    let s = m.snapshot(ID).unwrap();
    assert_eq!(s.status, ModelStatus::Idle);
    assert!(s.unload_in.unwrap() <= Duration::from_millis(200));
    assert_eq!(l.alive(), 1);

    wait_for(&m, ModelStatus::Available, Duration::from_secs(2));
    assert_eq!(
        l.alive(),
        0,
        "worker process must be gone after idle unload"
    );
}

#[test]
fn use_resets_the_idle_timer() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(
        ID,
        l.clone(),
        Policy::IdleTimeout(Duration::from_millis(250)),
    );
    for _ in 0..5 {
        drop(m.acquire(ID).unwrap());
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        l.counters.launches.load(Ordering::SeqCst),
        1,
        "kept warm while used"
    );
    wait_for(&m, ModelStatus::Available, Duration::from_secs(2));
}

#[test]
fn concurrent_acquires_share_one_load() {
    let m = Arc::new(mgr());
    let l = FakeLauncher::new(Duration::from_millis(150));
    m.register(ID, l.clone(), Policy::default());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let m = m.clone();
            std::thread::spawn(move || {
                let lease = m.acquire(ID).unwrap();
                std::thread::sleep(Duration::from_millis(20));
                drop(lease);
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(l.counters.launches.load(Ordering::SeqCst), 1);
    assert_eq!(l.alive(), 1);
}

#[test]
fn manual_unload_while_busy_is_deferred_until_the_request_ends() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(ID, l.clone(), Policy::KeepLoaded);
    let lease = m.acquire(ID).unwrap();
    assert_eq!(m.unload(ID).unwrap(), UnloadOutcome::Deferred);
    assert_eq!(status(&m), ModelStatus::Busy, "never killed mid-request");
    assert_eq!(l.alive(), 1);
    drop(lease);
    wait_for(&m, ModelStatus::Available, Duration::from_secs(2));
    assert_eq!(l.alive(), 0);
}

#[test]
fn unload_after_job_policy() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(ID, l.clone(), Policy::UnloadAfterJob);
    drop(m.acquire(ID).unwrap());
    wait_for(&m, ModelStatus::Available, Duration::from_secs(1));
    assert_eq!(l.alive(), 0);
}

#[test]
fn keep_loaded_stays_until_manual_unload() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(ID, l.clone(), Policy::KeepLoaded);
    drop(m.acquire(ID).unwrap());
    std::thread::sleep(Duration::from_millis(150));
    let s = m.snapshot(ID).unwrap();
    assert_eq!(s.status, ModelStatus::Ready);
    assert_eq!(s.unload_in, None);
    assert_eq!(m.unload(ID).unwrap(), UnloadOutcome::Unloaded);
    assert_eq!(status(&m), ModelStatus::Available);
    assert_eq!(l.alive(), 0);
    assert_eq!(m.unload(ID).unwrap(), UnloadOutcome::AlreadyUnloaded);
}

#[test]
fn shortening_the_policy_while_loaded_applies_immediately() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(ID, l.clone(), Policy::KeepLoaded);
    drop(m.acquire(ID).unwrap());
    m.set_policy(ID, Policy::IdleTimeout(Duration::from_millis(50)))
        .unwrap();
    wait_for(&m, ModelStatus::Available, Duration::from_secs(1));
    assert_eq!(l.alive(), 0);
}

#[test]
fn load_failure_is_shared_by_waiters_and_retried_later() {
    let m = Arc::new(mgr());
    let l = FakeLauncher::new(Duration::from_millis(100));
    *l.fail.lock().unwrap() = Some(RuntimeError::LoadFailed("bad weights".into()));
    m.register(ID, l.clone(), Policy::default());
    let waiters: Vec<_> = (0..4)
        .map(|_| {
            let m = m.clone();
            std::thread::spawn(move || m.acquire(ID).err())
        })
        .collect();
    for w in waiters {
        assert_eq!(
            w.join().unwrap(),
            Some(RuntimeError::LoadFailed("bad weights".into()))
        );
    }
    assert_eq!(l.counters.launches.load(Ordering::SeqCst), 1);
    let s = m.snapshot(ID).unwrap();
    assert_eq!(s.status, ModelStatus::Error);
    assert!(s.last_error.is_some());

    *l.fail.lock().unwrap() = None;
    drop(m.acquire(ID).unwrap());
    assert_eq!(l.counters.launches.load(Ordering::SeqCst), 2);
    assert_eq!(m.snapshot(ID).unwrap().last_error, None);
}

#[test]
fn unload_during_load_never_leaves_a_stale_ready() {
    let m = Arc::new(mgr());
    let l = FakeLauncher::new(Duration::from_millis(300));
    m.register(ID, l.clone(), Policy::default());
    let t = {
        let m = m.clone();
        std::thread::spawn(move || m.acquire(ID).err())
    };
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(status(&m), ModelStatus::Loading);
    assert_eq!(m.unload(ID).unwrap(), UnloadOutcome::CancelledLoad);
    assert_eq!(t.join().unwrap(), Some(RuntimeError::Cancelled));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(status(&m), ModelStatus::Available);
    assert_eq!(l.alive(), 0);
}

#[test]
fn crashed_worker_is_reported_and_restarted_on_next_use() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(ID, l.clone(), Policy::KeepLoaded);
    drop(m.acquire(ID).unwrap());
    l.last().crash();
    wait_for(&m, ModelStatus::Error, Duration::from_secs(1));
    assert!(matches!(
        m.snapshot(ID).unwrap().last_error,
        Some(RuntimeError::WorkerExited(_))
    ));
    drop(m.acquire(ID).unwrap());
    assert_eq!(l.counters.launches.load(Ordering::SeqCst), 2);
    assert_eq!(l.alive(), 1);
}

#[test]
fn shutdown_stops_busy_workers_and_rejects_new_work() {
    let m = mgr();
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(ID, l.clone(), Policy::KeepLoaded);
    let lease = m.acquire(ID).unwrap();
    m.shutdown();
    assert_eq!(l.alive(), 0);
    assert_eq!(m.acquire(ID).err(), Some(RuntimeError::ShuttingDown));
    drop(lease);
    m.shutdown(); // idempotent
}

#[test]
fn dropping_the_manager_stops_workers() {
    let l = FakeLauncher::new(Duration::ZERO);
    {
        let m = mgr();
        m.register(ID, l.clone(), Policy::KeepLoaded);
        drop(m.acquire(ID).unwrap());
        assert_eq!(l.alive(), 1);
    }
    assert_eq!(l.alive(), 0);
}

#[test]
fn reregistering_replaces_the_running_worker() {
    let m = mgr();
    let a = FakeLauncher::new(Duration::ZERO);
    let b = FakeLauncher::new(Duration::ZERO);
    m.register(ID, a.clone(), Policy::KeepLoaded);
    drop(m.acquire(ID).unwrap());
    m.register(ID, b.clone(), Policy::KeepLoaded);
    assert_eq!(a.alive(), 0);
    assert_eq!(status(&m), ModelStatus::Available);
    drop(m.acquire(ID).unwrap());
    assert_eq!(b.alive(), 1);
    m.remove(ID);
    assert_eq!(b.alive(), 0);
    assert!(m.snapshot(ID).is_none());
}

#[test]
fn unknown_model() {
    let m = mgr();
    assert_eq!(m.acquire("nope").err(), Some(RuntimeError::UnknownModel));
    assert_eq!(m.unload("nope").err(), Some(RuntimeError::UnknownModel));
}

#[test]
fn listener_sees_the_full_lifecycle() {
    let seen: Arc<Mutex<Vec<ModelStatus>>> = Arc::default();
    let listener: Listener = {
        let seen = seen.clone();
        Arc::new(move |s: &ModelSnapshot| seen.lock().unwrap().push(s.status))
    };
    let m = ModelManager::with_poll(Some(listener), Duration::from_millis(20));
    let l = FakeLauncher::new(Duration::ZERO);
    m.register(ID, l, Policy::IdleTimeout(Duration::from_millis(50)));
    drop(m.acquire(ID).unwrap());
    wait_for(&m, ModelStatus::Available, Duration::from_secs(1));
    let mut s = seen.lock().unwrap().clone();
    s.dedup();
    assert_eq!(
        s,
        vec![
            ModelStatus::Available,
            ModelStatus::Loading,
            ModelStatus::Busy,
            ModelStatus::Idle,
            ModelStatus::Unloading,
            ModelStatus::Available,
        ]
    );
}
