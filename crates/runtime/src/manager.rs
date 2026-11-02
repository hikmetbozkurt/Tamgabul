//! Single lifecycle authority for model workers (docs/05).
//!
//! - Nothing is loaded at startup. A model is started only when work needs
//!   it ([`ModelManager::acquire`]) and stopped when it has been idle for its
//!   policy's timeout (default 5 min), after each job, or on explicit unload.
//! - "Unloaded" means the worker *process has exited*, so the OS has released
//!   its RAM and VRAM; we wait for that before reporting AVAILABLE.
//! - Concurrent acquirers share one in-flight load. Unload never kills a
//!   model that is serving a request: it is deferred until the lease drops.
//! - Every load has an epoch; a load that finishes after an unload/cancel is
//!   terminated instead of becoming a stale READY.
//! - A worker that dies on its own is detected within ~1 s and reported as
//!   ERROR with the reason; the next acquire starts a fresh one.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tamgabul_core::model::ModelStatus;

use crate::client::Endpoint;
use crate::error::RuntimeError;
use crate::worker::{Launcher, Worker};

/// How long a loaded model may stay resident without work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// Unload this long after the last request finished.
    IdleTimeout(Duration),
    /// Unload as soon as the last request finished.
    UnloadAfterJob,
    /// Stay resident until unloaded manually or the app quits (opt-in).
    KeepLoaded,
}

impl Default for Policy {
    /// docs/05 initial default for EmbeddingGemma 2 and the LLM.
    fn default() -> Self {
        Policy::IdleTimeout(Duration::from_secs(5 * 60))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelSnapshot {
    pub id: String,
    pub status: ModelStatus,
    /// Requests currently holding the model.
    pub active: u32,
    /// Time left before an idle unload, if one is scheduled.
    pub unload_in: Option<Duration>,
    pub pid: Option<u32>,
    pub last_error: Option<RuntimeError>,
    /// Completed loads since registration (for tests and diagnostics).
    pub loads: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnloadOutcome {
    Unloaded,
    /// Busy: will unload when the current request finishes.
    Deferred,
    /// A load in progress was cancelled.
    CancelledLoad,
    AlreadyUnloaded,
}

pub type Listener = Arc<dyn Fn(&ModelSnapshot) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Available,
    Loading,
    Loaded,
    Unloading,
    Error,
}

struct Slot {
    launcher: Arc<dyn Launcher>,
    policy: Policy,
    phase: Phase,
    epoch: u64,
    active: u32,
    last_used: Instant,
    worker: Option<Arc<dyn Worker>>,
    cancel: Arc<AtomicBool>,
    unload_when_idle: bool,
    last_error: Option<RuntimeError>,
    loads: u64,
}

impl Slot {
    fn deadline(&self) -> Option<Instant> {
        if self.phase != Phase::Loaded || self.active > 0 {
            return None;
        }
        if self.unload_when_idle {
            return Some(self.last_used);
        }
        match self.policy {
            Policy::IdleTimeout(d) => Some(self.last_used + d),
            Policy::UnloadAfterJob => Some(self.last_used),
            Policy::KeepLoaded => None,
        }
    }

    fn snapshot(&self, id: &str, now: Instant) -> ModelSnapshot {
        let status = match self.phase {
            Phase::Available => ModelStatus::Available,
            Phase::Loading => ModelStatus::Loading,
            Phase::Unloading => ModelStatus::Unloading,
            Phase::Error => ModelStatus::Error,
            Phase::Loaded if self.active > 0 => ModelStatus::Busy,
            Phase::Loaded if self.deadline().is_some() => ModelStatus::Idle,
            Phase::Loaded => ModelStatus::Ready,
        };
        ModelSnapshot {
            id: id.to_owned(),
            status,
            active: self.active,
            unload_in: self.deadline().map(|d| d.saturating_duration_since(now)),
            pid: self.worker.as_ref().and_then(|w| w.pid()),
            last_error: self.last_error.clone(),
            loads: self.loads,
        }
    }
}

#[derive(Default)]
struct State {
    slots: HashMap<String, Slot>,
    shutting_down: bool,
}

struct Inner {
    state: Mutex<State>,
    cv: Condvar,
    listener: Option<Listener>,
    /// Upper bound between crash checks.
    poll: Duration,
}

type Guard<'a> = MutexGuard<'a, State>;

impl Inner {
    fn lock(&self) -> Guard<'_> {
        // A poisoned lock means a panic mid-transition; the state is still
        // structurally valid, so keep serving rather than cascading panics.
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emit(&self, snaps: Vec<ModelSnapshot>) {
        if let Some(l) = &self.listener {
            for s in &snaps {
                l(s);
            }
        }
    }

    fn snap(st: &State, id: &str) -> Option<ModelSnapshot> {
        st.slots.get(id).map(|s| s.snapshot(id, Instant::now()))
    }

    /// Stop the worker of a Loaded slot. Releases the lock while waiting
    /// for the process to exit; returns the re-acquired guard.
    fn stop<'a>(&'a self, mut st: Guard<'a>, id: &str, out: &mut Vec<ModelSnapshot>) -> Guard<'a> {
        let Some(slot) = st.slots.get_mut(id) else {
            return st;
        };
        let Some(worker) = slot.worker.take() else {
            slot.phase = Phase::Available;
            return st;
        };
        slot.phase = Phase::Unloading;
        slot.unload_when_idle = false;
        slot.epoch += 1;
        let epoch = slot.epoch;
        out.extend(Self::snap(&st, id));
        drop(st);
        let result = worker.terminate();
        drop(worker);
        let mut st = self.lock();
        if let Some(slot) = st.slots.get_mut(id) {
            if slot.epoch == epoch && slot.phase == Phase::Unloading {
                match result {
                    Ok(()) => slot.phase = Phase::Available,
                    Err(e) => {
                        slot.phase = Phase::Error;
                        slot.last_error = Some(e);
                    }
                }
            }
        }
        out.extend(Self::snap(&st, id));
        self.cv.notify_all();
        st
    }

    /// One reaper pass: idle unloads and crash detection. Returns how long
    /// to sleep before the next pass.
    fn reap(&self) -> Duration {
        let mut out = Vec::new();
        let mut st = self.lock();
        let now = Instant::now();
        let mut next = self.poll;
        let ids: Vec<String> = st.slots.keys().cloned().collect();
        for id in ids {
            let Some(slot) = st.slots.get_mut(&id) else {
                continue;
            };
            if slot.phase != Phase::Loaded {
                continue;
            }
            if let Some(reason) = slot.worker.as_ref().and_then(|w| w.exit_reason()) {
                slot.worker = None;
                slot.phase = Phase::Error;
                slot.last_error = Some(RuntimeError::WorkerExited(reason));
                slot.epoch += 1;
                out.extend(Self::snap(&st, &id));
                self.cv.notify_all();
                continue;
            }
            match slot.deadline() {
                Some(d) if d <= now => st = self.stop(st, &id, &mut out),
                Some(d) => next = next.min(d - now),
                None => {}
            }
        }
        drop(st);
        self.emit(out);
        next
    }
}

/// Holds a loaded model for the duration of one request. While any lease
/// exists the model is BUSY and is never unloaded by TTL or manual unload.
pub struct Lease {
    inner: Arc<Inner>,
    id: String,
    worker: Arc<dyn Worker>,
}

impl Lease {
    pub fn endpoint(&self) -> &Endpoint {
        self.worker.endpoint()
    }
    pub fn pid(&self) -> Option<u32> {
        self.worker.pid()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let snap = {
            let mut st = self.inner.lock();
            if let Some(slot) = st.slots.get_mut(&self.id) {
                slot.active = slot.active.saturating_sub(1);
                slot.last_used = Instant::now();
            }
            Inner::snap(&st, &self.id)
        };
        // Wake the reaper: the idle deadline may have changed.
        self.inner.cv.notify_all();
        self.inner.emit(snap.into_iter().collect());
    }
}

pub struct ModelManager {
    inner: Arc<Inner>,
    reaper: Mutex<Option<JoinHandle<()>>>,
}

impl ModelManager {
    pub fn new(listener: Option<Listener>) -> Self {
        Self::with_poll(listener, Duration::from_secs(1))
    }

    pub fn with_poll(listener: Option<Listener>, poll: Duration) -> Self {
        let inner = Arc::new(Inner {
            state: Mutex::new(State::default()),
            cv: Condvar::new(),
            listener,
            poll,
        });
        let r = inner.clone();
        let reaper = std::thread::Builder::new()
            .name("tamgabul-model-reaper".into())
            .spawn(move || loop {
                let wait = r.reap();
                let st = r.lock();
                if st.shutting_down {
                    return;
                }
                let (st, _) =
                    r.cv.wait_timeout(st, wait)
                        .unwrap_or_else(|p| p.into_inner());
                if st.shutting_down {
                    return;
                }
            })
            .expect("spawn reaper thread");
        Self {
            inner,
            reaper: Mutex::new(Some(reaper)),
        }
    }

    /// Register or replace a model. Registration never starts a worker.
    /// Replacing stops the old worker first (its files may have changed).
    pub fn register(&self, id: &str, launcher: Arc<dyn Launcher>, policy: Policy) {
        let mut out = Vec::new();
        let mut st = self.inner.lock();
        if st.slots.contains_key(id) {
            st = self.cancel_or_stop(st, id, &mut out);
        }
        st.slots.insert(
            id.to_owned(),
            Slot {
                launcher,
                policy,
                phase: Phase::Available,
                epoch: 0,
                active: 0,
                last_used: Instant::now(),
                worker: None,
                cancel: Arc::new(AtomicBool::new(false)),
                unload_when_idle: false,
                last_error: None,
                loads: 0,
            },
        );
        out.extend(Inner::snap(&st, id));
        drop(st);
        self.inner.cv.notify_all();
        self.inner.emit(out);
    }

    /// Stop (if needed) and forget a model.
    pub fn remove(&self, id: &str) {
        let mut out = Vec::new();
        let st = self.inner.lock();
        let mut st = self.cancel_or_stop(st, id, &mut out);
        st.slots.remove(id);
        drop(st);
        self.inner.cv.notify_all();
        self.inner.emit(out);
    }

    fn cancel_or_stop<'a>(
        &'a self,
        mut st: Guard<'a>,
        id: &str,
        out: &mut Vec<ModelSnapshot>,
    ) -> Guard<'a> {
        if let Some(slot) = st.slots.get_mut(id) {
            match slot.phase {
                Phase::Loading => {
                    slot.cancel.store(true, Ordering::SeqCst);
                    slot.epoch += 1;
                    slot.phase = Phase::Available;
                }
                Phase::Loaded => st = self.inner.stop(st, id, out),
                _ => {}
            }
        }
        st
    }

    pub fn set_policy(&self, id: &str, policy: Policy) -> Result<(), RuntimeError> {
        let snap = {
            let mut st = self.inner.lock();
            let slot = st.slots.get_mut(id).ok_or(RuntimeError::UnknownModel)?;
            slot.policy = policy;
            Inner::snap(&st, id)
        };
        self.inner.cv.notify_all();
        self.inner.emit(snap.into_iter().collect());
        Ok(())
    }

    pub fn snapshot(&self, id: &str) -> Option<ModelSnapshot> {
        Inner::snap(&self.inner.lock(), id)
    }

    pub fn snapshots(&self) -> Vec<ModelSnapshot> {
        let st = self.inner.lock();
        let now = Instant::now();
        let mut v: Vec<_> = st.slots.iter().map(|(id, s)| s.snapshot(id, now)).collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// Get a loaded model for one request, starting it if necessary. Blocks
    /// while it loads. Concurrent callers share the same load.
    pub fn acquire(&self, id: &str) -> Result<Lease, RuntimeError> {
        let inner = &self.inner;
        let mut st = inner.lock();
        // Epoch of a load we waited on; if that load failed we report its
        // error instead of starting another one.
        let mut waited: Option<u64> = None;
        loop {
            if st.shutting_down {
                return Err(RuntimeError::ShuttingDown);
            }
            let slot = st.slots.get_mut(id).ok_or(RuntimeError::UnknownModel)?;
            match slot.phase {
                Phase::Loaded => {
                    if let Some(reason) = slot.worker.as_ref().and_then(|w| w.exit_reason()) {
                        // Died since the last reaper pass: restart it.
                        slot.worker = None;
                        slot.phase = Phase::Available;
                        slot.last_error = Some(RuntimeError::WorkerExited(reason));
                        slot.epoch += 1;
                        continue;
                    }
                    let worker = slot.worker.clone().expect("loaded slot has a worker");
                    slot.active += 1;
                    let snap = Inner::snap(&st, id);
                    drop(st);
                    inner.emit(snap.into_iter().collect());
                    return Ok(Lease {
                        inner: inner.clone(),
                        id: id.to_owned(),
                        worker,
                    });
                }
                Phase::Loading | Phase::Unloading => {
                    if slot.phase == Phase::Loading {
                        waited = Some(slot.epoch);
                    }
                    st = inner.cv.wait(st).unwrap_or_else(|p| p.into_inner());
                }
                Phase::Error if waited.is_some_and(|e| e == slot.epoch) => {
                    return Err(slot.last_error.clone().unwrap_or(RuntimeError::Cancelled));
                }
                Phase::Available | Phase::Error => {
                    slot.phase = Phase::Loading;
                    slot.epoch += 1;
                    slot.cancel = Arc::new(AtomicBool::new(false));
                    let (epoch, cancel, launcher) =
                        (slot.epoch, slot.cancel.clone(), slot.launcher.clone());
                    let snap = Inner::snap(&st, id);
                    drop(st);
                    inner.emit(snap.into_iter().collect());

                    let result = launcher.launch(&cancel);

                    st = inner.lock();
                    let shutting = st.shutting_down;
                    let current = st
                        .slots
                        .get_mut(id)
                        .filter(|s| s.epoch == epoch && s.phase == Phase::Loading);
                    match (current, result) {
                        (Some(slot), Ok(worker)) if !shutting => {
                            slot.worker = Some(worker);
                            slot.phase = Phase::Loaded;
                            slot.last_error = None;
                            slot.loads += 1;
                            slot.last_used = Instant::now();
                            inner.cv.notify_all();
                            // Loop: take the lease through the Loaded arm.
                        }
                        (Some(slot), Err(e)) => {
                            slot.phase = Phase::Error;
                            slot.last_error = Some(e.clone());
                            let snap = Inner::snap(&st, id);
                            drop(st);
                            inner.cv.notify_all();
                            inner.emit(snap.into_iter().collect());
                            return Err(e);
                        }
                        (_, Ok(worker)) => {
                            // Unloaded/cancelled/replaced while loading: never
                            // let this late worker become a stale READY.
                            drop(st);
                            let _ = worker.terminate();
                            inner.cv.notify_all();
                            return Err(RuntimeError::Cancelled);
                        }
                        (_, Err(_)) => {
                            drop(st);
                            inner.cv.notify_all();
                            return Err(RuntimeError::Cancelled);
                        }
                    }
                }
            }
        }
    }

    /// Manual unload (Models page, tray "Unload models").
    pub fn unload(&self, id: &str) -> Result<UnloadOutcome, RuntimeError> {
        let mut out = Vec::new();
        let mut st = self.inner.lock();
        let slot = st.slots.get_mut(id).ok_or(RuntimeError::UnknownModel)?;
        let outcome = match slot.phase {
            Phase::Loading => {
                slot.cancel.store(true, Ordering::SeqCst);
                slot.epoch += 1;
                slot.phase = Phase::Available;
                out.extend(Inner::snap(&st, id));
                UnloadOutcome::CancelledLoad
            }
            Phase::Loaded if slot.active > 0 => {
                slot.unload_when_idle = true;
                out.extend(Inner::snap(&st, id));
                UnloadOutcome::Deferred
            }
            Phase::Loaded => {
                st = self.inner.stop(st, id, &mut out);
                UnloadOutcome::Unloaded
            }
            Phase::Unloading | Phase::Available | Phase::Error => UnloadOutcome::AlreadyUnloaded,
        };
        drop(st);
        self.inner.cv.notify_all();
        self.inner.emit(out);
        Ok(outcome)
    }

    pub fn unload_all(&self) -> Vec<(String, UnloadOutcome)> {
        let ids: Vec<String> = self.inner.lock().slots.keys().cloned().collect();
        ids.into_iter()
            .filter_map(|id| self.unload(&id).ok().map(|o| (id, o)))
            .collect()
    }

    /// Quit: reject new work, cancel loads, stop every worker (even busy
    /// ones — the app is exiting) and wait until the processes are gone.
    /// Idempotent.
    pub fn shutdown(&self) {
        let mut out = Vec::new();
        let mut st = self.inner.lock();
        st.shutting_down = true;
        let ids: Vec<String> = st.slots.keys().cloned().collect();
        for id in &ids {
            st = self.cancel_or_stop(st, id, &mut out);
        }
        drop(st);
        self.inner.cv.notify_all();
        self.inner.emit(out);
        let handle = self.reaper.lock().ok().and_then(|mut h| h.take());
        if let Some(h) = handle {
            let _ = h.join();
        }
    }
}

impl Drop for ModelManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
