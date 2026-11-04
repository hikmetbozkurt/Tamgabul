//! Folder watching (Folders → "Watch for changes", ADR 016).
//!
//! The OS reports changes under watched folders; after a quiet period the
//! folder gets a normal delta scan (unchanged files are skipped by size +
//! modification time, so a rescan only reads what changed). Nothing is
//! read in the event handler itself, and no model starts unless a changed
//! file really needs embedding.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use tamgabul_core::db;
use tamgabul_core::index::store;
use tamgabul_core::index::walk::is_within;
use tauri::{AppHandle, Manager};

use crate::AppState;

/// Wait this long after the last change before rescanning (saves, copies
/// and downloads come in bursts).
const QUIET: Duration = Duration::from_secs(10);
const TICK: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct WatchState {
    watcher: Mutex<Option<RecommendedWatcher>>,
    /// scope_id → watched root.
    roots: Mutex<HashMap<String, PathBuf>>,
    /// scope_id → time of the last change seen.
    dirty: Mutex<HashMap<String, Instant>>,
    /// Paths whose changes are ignored (app data: the index itself).
    ignore: Mutex<Vec<PathBuf>>,
}

fn on_event(app: &AppHandle, ev: notify::Result<notify::Event>) {
    let Ok(ev) = ev else { return };
    if matches!(ev.kind, EventKind::Access(_)) {
        return;
    }
    let state = app.state::<AppState>();
    let w = &state.watch;
    let ignore = w.ignore.lock().map(|v| v.clone()).unwrap_or_default();
    let Ok(roots) = w.roots.lock() else { return };
    let Ok(mut dirty) = w.dirty.lock() else {
        return;
    };
    for p in &ev.paths {
        if ignore.iter().any(|i| is_within(p, i)) {
            continue;
        }
        for (id, root) in roots.iter() {
            if is_within(p, root) {
                dirty.insert(id.clone(), Instant::now());
            }
        }
    }
}

/// Watch exactly the folders that have watching on (call after any
/// change to folders or their watching flag).
pub fn sync(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(path) = state.db_path.clone() else {
        return;
    };
    let Ok(conn) = db::open_worker(&path) else {
        return;
    };
    let want: HashMap<String, PathBuf> = store::watched_scopes(&conn)
        .unwrap_or_default()
        .into_iter()
        .filter(|s| s.canonical_path.is_dir())
        .map(|s| (s.scope_id, s.canonical_path))
        .collect();
    let w = &state.watch;
    if let Ok(mut ig) = w.ignore.lock() {
        *ig = state.app_data_dir.iter().cloned().collect();
    }
    let Ok(mut guard) = w.watcher.lock() else {
        return;
    };
    if guard.is_none() {
        let handle = app.clone();
        match notify::recommended_watcher(move |ev| on_event(&handle, ev)) {
            Ok(x) => *guard = Some(x),
            Err(e) => {
                eprintln!("folder watching unavailable: {e}");
                return;
            }
        }
    }
    let Some(watcher) = guard.as_mut() else {
        return;
    };
    let Ok(mut roots) = w.roots.lock() else {
        return;
    };
    for (id, root) in roots.clone() {
        if want.get(&id) != Some(&root) {
            let _ = watcher.unwatch(&root);
            roots.remove(&id);
            if let Ok(mut d) = w.dirty.lock() {
                d.remove(&id);
            }
        }
    }
    for (id, root) in want {
        if roots.contains_key(&id) {
            continue;
        }
        if watcher.watch(&root, RecursiveMode::Recursive).is_ok() {
            roots.insert(id, root);
        }
    }
}

/// Background loop: rescan folders that have been quiet for `QUIET`.
pub fn start(app: &AppHandle) {
    sync(app);
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("tamgabul-watch".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            let state = app.state::<AppState>();
            let due: Vec<String> = match state.watch.dirty.lock() {
                Ok(d) => d
                    .iter()
                    .filter(|(_, t)| t.elapsed() >= QUIET)
                    .map(|(id, _)| id.clone())
                    .collect(),
                Err(_) => continue,
            };
            for id in due {
                if crate::scopes::is_scanning(&state, &id) {
                    // Picked up again after the running scan.
                    continue;
                }
                if let Ok(mut d) = state.watch.dirty.lock() {
                    d.remove(&id);
                }
                let _ = crate::scopes::start_job(&app, &id);
            }
        });
}
