//! Model commands (P4, docs/04, docs/13): validate → register → load/unload.
//!
//! The registry lives in SQLite (`tamgabul_core::registry`); live status comes
//! from the runtime `ModelManager`. Workers start only when work needs them
//! and stop by their idle policy (ADR 019). Errors leaving here carry a code
//! and a short hint, never a path or a runtime log.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tamgabul_core::db;
use tamgabul_core::dto::{ModelEntry, RegisterModelRequest, StepState, TtlPolicy, ValidationStep};
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::health::{derive_health, AppHealth, HealthInputs, ModelSummary};
use tamgabul_core::model::{ModelCategory, ModelStatus};
use tamgabul_core::registry::{self, ModelRecord};
use tamgabul_runtime::worker::{Launcher, LlamaServerConfig, ProcessLauncher, Worker};
use tamgabul_runtime::{eg2, llm, locate, smoke, ModelManager, Policy, RuntimeError};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::AppState;

/// A passed validation waiting for "Save". Register only accepts what was
/// validated in this session, so the UI cannot skip the checks.
pub struct Validated {
    category: ModelCategory,
    path: String,
    record: ModelRecord,
    at: Instant,
}

#[derive(Default)]
pub struct ModelsState {
    pub validation_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub validated: Mutex<Option<Validated>>,
}

const VALIDATION_FRESH_FOR: Duration = Duration::from_secs(30 * 60);

fn lock_err() -> IpcError {
    IpcError::new(ErrorCode::Internal, "state lock poisoned")
}

/// Code plus a fixed hint. Runtime messages can contain local paths or log
/// lines, so they never cross the IPC boundary.
pub(crate) fn ipc(e: RuntimeError) -> IpcError {
    let hint = match &e {
        RuntimeError::ModelMissing => "model files not found",
        RuntimeError::InvalidModelFile(_) => "invalid model file",
        RuntimeError::Incompatible(_) => "model not supported",
        RuntimeError::RuntimeNotFound => "llama-server not found",
        RuntimeError::LoadFailed(_) => "worker failed to start",
        RuntimeError::LoadTimeout => "worker start timed out",
        RuntimeError::WorkerExited(_) => "worker exited",
        RuntimeError::Request(_) | RuntimeError::BadResponse(_) => "worker request failed",
        RuntimeError::Timeout => "worker request timed out",
        RuntimeError::UnknownModel => "unknown model",
        RuntimeError::ShuttingDown => "shutting down",
        RuntimeError::Cancelled => "cancelled",
        RuntimeError::Io(_) => "file access failed",
    };
    IpcError::new(e.code(), hint)
}

pub fn policy(ttl: &TtlPolicy) -> Policy {
    match ttl {
        TtlPolicy::Immediately => Policy::UnloadAfterJob,
        TtlPolicy::Minutes5 => Policy::IdleTimeout(Duration::from_secs(300)),
        TtlPolicy::Minutes10 => Policy::IdleTimeout(Duration::from_secs(600)),
        TtlPolicy::Minutes30 => Policy::IdleTimeout(Duration::from_secs(1800)),
        TtlPolicy::Hours1 => Policy::IdleTimeout(Duration::from_secs(3600)),
        TtlPolicy::Never => Policy::KeepLoaded,
    }
}

/// Starts an EG2 worker. Files and the runtime are resolved at launch time,
/// so a moved runtime or deleted file is reported when it matters.
struct Eg2Launcher {
    path: PathBuf,
}

impl Launcher for Eg2Launcher {
    fn launch(&self, cancel: &AtomicBool) -> Result<Arc<dyn Worker>, RuntimeError> {
        let files = eg2::locate(&self.path)?;
        let exe = locate::find_llama_server(None)?.path;
        let cfg = LlamaServerConfig::eg2(exe, files.model, files.mmproj);
        Ok(Arc::new(
            ProcessLauncher::llama_server(cfg).launch_process(cancel)?,
        ))
    }
}

/// Starts the search-plan LLM worker (text only, small context).
struct LlmLauncher {
    path: PathBuf,
}

impl Launcher for LlmLauncher {
    fn launch(&self, cancel: &AtomicBool) -> Result<Arc<dyn Worker>, RuntimeError> {
        let files = llm::locate(&self.path)?;
        let exe = locate::find_llama_server(None)?.path;
        let cfg = LlamaServerConfig::chat(exe, files.model, files.mmproj);
        Ok(Arc::new(
            ProcessLauncher::llama_server(cfg).launch_process(cancel)?,
        ))
    }
}

/// Make a registered model known to the manager (nothing starts).
pub(crate) fn attach(mgr: &ModelManager, rec: &ModelRecord) {
    let path = PathBuf::from(&rec.path);
    let launcher: Arc<dyn Launcher> = match rec.category {
        ModelCategory::Embedding => Arc::new(Eg2Launcher { path }),
        ModelCategory::Llm => Arc::new(LlmLauncher { path }),
        ModelCategory::Stt | ModelCategory::Tts => return,
    };
    mgr.register(&rec.model_id, launcher, policy(&rec.ttl));
}

/// Model categories that can run in this build.
fn runnable(c: ModelCategory) -> bool {
    matches!(c, ModelCategory::Embedding | ModelCategory::Llm)
}

/// App start: make the active models known to the manager. Nothing loads.
pub fn init(state: &AppState) {
    let Ok(guard) = state.db.lock() else { return };
    let Some(conn) = guard.as_ref() else { return };
    for rec in registry::active(conn).unwrap_or_default() {
        if rec.status.from_persisted_snapshot() == ModelStatus::Available {
            attach(&state.models, &rec);
        }
    }
}

fn parse_code(s: &str) -> Option<ErrorCode> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

fn entry(rec: &ModelRecord, mgr: &ModelManager) -> ModelEntry {
    let snap = mgr.snapshot(&rec.model_id);
    let files_ok = Path::new(&rec.path).exists();
    let mut error_key = None;
    let (status, error_code) = if !files_ok {
        (ModelStatus::Missing, Some(ErrorCode::ModelMissing))
    } else if let Some(s) = &snap {
        if s.last_error == Some(RuntimeError::RuntimeNotFound) {
            error_key = Some("model.err.runtime_missing".to_string());
        }
        (s.status, s.last_error.as_ref().map(RuntimeError::code))
    } else {
        (
            rec.status.from_persisted_snapshot(),
            rec.last_error_code.as_deref().and_then(parse_code),
        )
    };
    ModelEntry {
        model_id: rec.model_id.clone(),
        category: rec.category,
        display_name: rec.display_name.clone(),
        source_label: rec.provider.clone(),
        status,
        tone: status.tone(rec.category),
        required: rec.category.is_required(),
        runtime: Some(format!("{} {}", rec.runtime_adapter, rec.adapter_version)),
        dimension: rec.embedding_dim,
        // Measured from the OS while the worker runs; never estimated.
        memory_mb: snap
            .as_ref()
            .filter(|s| s.status.is_loaded())
            .and_then(|s| s.pid)
            .and_then(tamgabul_platform::usage::process_memory_mb)
            .map(|m| m as f64),
        validated_at: rec.validated_at.clone(),
        ttl: rec.ttl.clone(),
        error_code,
        error_key,
    }
}

fn entries(state: &AppState) -> Result<Vec<ModelEntry>, IpcError> {
    let guard = state.db.lock().map_err(|_| lock_err())?;
    let Some(conn) = guard.as_ref() else {
        return Err(IpcError::new(
            ErrorCode::DbMigrationFailed,
            "database unavailable",
        ));
    };
    Ok(registry::active(conn)?
        .iter()
        .map(|r| entry(r, &state.models))
        .collect())
}

/// Health from the registry plus live worker state.
pub fn health(state: &AppState) -> Result<AppHealth, IpcError> {
    let guard = state.db.lock().map_err(|_| lock_err())?;
    let Some(conn) = guard.as_ref() else {
        return Ok(db::read_health(None));
    };
    let list: Vec<ModelEntry> = registry::active(conn)?
        .iter()
        .map(|r| entry(r, &state.models))
        .collect();
    let summary = |c: ModelCategory| match list.iter().find(|m| m.category == c) {
        Some(m) => ModelSummary::new(c, m.status, Some(m.display_name.clone())),
        None => ModelSummary::new(c, ModelStatus::Missing, None),
    };
    Ok(derive_health(HealthInputs {
        db_ok: true,
        active_scope_count: db::active_scope_count(conn)?,
        embedding: summary(ModelCategory::Embedding),
        llm: summary(ModelCategory::Llm),
        stt: summary(ModelCategory::Stt),
        tts: summary(ModelCategory::Tts),
    }))
}

#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelEntry>, IpcError> {
    entries(&state)
}

// ---------------------------------------------------------------- validation

struct Steps {
    steps: Vec<ValidationStep>,
    channel: Channel<Vec<ValidationStep>>,
}

impl Steps {
    fn new(channel: Channel<Vec<ValidationStep>>, ids: &[(&str, &str)]) -> Self {
        let steps = ids
            .iter()
            .map(|(id, key)| ValidationStep {
                id: (*id).into(),
                label_key: (*key).into(),
                state: StepState::Pending,
                detail: None,
            })
            .collect();
        let s = Self { steps, channel };
        s.send();
        s
    }
    fn send(&self) {
        let _ = self.channel.send(self.steps.clone());
    }
    fn set(&mut self, id: &str, state: StepState, detail: Option<String>) {
        if let Some(s) = self.steps.iter_mut().find(|s| s.id == id) {
            s.state = state;
            s.detail = detail;
        }
        self.send();
    }
    fn run(&mut self, id: &str) {
        self.set(id, StepState::Running, None);
    }
    fn ok(&mut self, id: &str, detail: impl Into<Option<String>>) {
        self.set(id, StepState::Ok, detail.into());
    }
    fn fail(&mut self, id: &str, detail_key: &str) -> Result<bool, IpcError> {
        self.set(id, StepState::Failed, Some(detail_key.into()));
        Ok(false)
    }
}

fn cancelled(flag: &AtomicBool) -> Result<(), IpcError> {
    if flag.load(Ordering::SeqCst) {
        Err(IpcError::new(ErrorCode::UserCancelled, "cancelled"))
    } else {
        Ok(())
    }
}

fn new_model_id() -> String {
    let t = tamgabul_runtime::worker::random_token().unwrap_or_else(|_| {
        format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        )
    });
    format!("m-{}", &t[..t.len().min(24)])
}

fn validate_embedding(
    path: &str,
    steps: &mut Steps,
    cancel: &AtomicBool,
) -> Result<Option<ModelRecord>, IpcError> {
    // 1. readable
    steps.run("readable");
    let p = PathBuf::from(path);
    if !p.exists() {
        steps.fail("readable", "addModel.fail.notFound")?;
        return Ok(None);
    }
    steps.ok("readable", None);
    cancelled(cancel)?;

    // 2. format (GGUF headers only)
    steps.run("format");
    let (files, probe) = match eg2::locate(&p).and_then(|f| eg2::probe(&f).map(|pr| (f, pr))) {
        Ok(x) => x,
        Err(_) => {
            steps.fail("format", "addModel.fail.format.embedding")?;
            return Ok(None);
        }
    };
    let mut kinds = vec!["text"];
    if probe.vision {
        kinds.push("image");
    }
    if probe.audio {
        kinds.push("audio");
    }
    steps.ok(
        "format",
        format!(
            "{}d · {} · {}",
            probe.output_dim,
            probe.precision,
            kinds.join(" + ")
        ),
    );
    cancelled(cancel)?;

    // 3. checksum
    steps.run("checksum");
    let mut pinned = true;
    let mut model_hash = None;
    for f in std::iter::once(&files.model).chain(files.mmproj.iter()) {
        cancelled(cancel)?;
        match eg2::sha256_file(f, |_, _| {}) {
            Ok(h) => {
                pinned &= eg2::is_pinned(&h);
                if model_hash.is_none() {
                    model_hash = Some(h);
                }
            }
            Err(_) => {
                steps.fail("checksum", "addModel.fail.readable.embedding")?;
                return Ok(None);
            }
        }
    }
    steps.ok(
        "checksum",
        Some(
            if pinned {
                "addModel.detail.pinned"
            } else {
                "addModel.detail.unpinned"
            }
            .to_string(),
        ),
    );
    cancelled(cancel)?;

    // 4. runtime
    steps.run("runtime");
    let Ok(found) = locate::find_llama_server(None) else {
        steps.fail("runtime", "addModel.fail.runtime")?;
        return Ok(None);
    };
    let version = locate::version(&found.path).unwrap_or_else(|_| "unknown".into());
    steps.ok("runtime", format!("llama.cpp {version}"));
    cancelled(cancel)?;

    // 5. smoke test with a private worker (stopped afterwards)
    steps.run("smoke");
    let cfg = LlamaServerConfig::eg2(found.path, files.model.clone(), files.mmproj.clone());
    match smoke::eg2_smoke(cfg, cancel) {
        Ok(r) => {
            let mark = |o: Option<bool>| match o {
                Some(true) => "✓",
                Some(false) => "✗",
                None => "–",
            };
            steps.ok(
                "smoke",
                format!(
                    "{}d · image {} · audio {} · {:.1} s",
                    r.dim,
                    mark(r.image),
                    mark(r.audio),
                    r.cold_load.as_secs_f64()
                ),
            );
        }
        Err(RuntimeError::Cancelled) => {
            return Err(IpcError::new(ErrorCode::UserCancelled, "cancelled"))
        }
        Err(_) => {
            steps.fail("smoke", "addModel.fail.smoke")?;
            return Ok(None);
        }
    }

    let mut caps: Vec<String> = kinds.iter().map(|s| s.to_string()).collect();
    caps.dedup();
    Ok(Some(ModelRecord {
        model_id: new_model_id(),
        category: ModelCategory::Embedding,
        display_name: "EmbeddingGemma 2".into(),
        provider: Some(if pinned {
            format!("ggml-org/embeddinggemma-2-GGUF · {}", probe.precision)
        } else {
            format!("GGUF · {}", probe.precision)
        }),
        path: path.to_string(),
        format: "gguf".into(),
        architecture: Some(eg2::ARCH.into()),
        weights_sha256: model_hash,
        model_revision: pinned.then(|| eg2::PINNED_REVISION.to_string()),
        license_name: pinned.then(|| "apache-2.0".to_string()),
        runtime_adapter: "llama.cpp".into(),
        adapter_version: version,
        capabilities: caps,
        embedding_dim: Some(probe.output_dim as u32),
        status: ModelStatus::Available,
        last_error_code: None,
        validated_at: None,
        ttl: TtlPolicy::Minutes5,
    }))
}

fn validate_llm(
    path: &str,
    steps: &mut Steps,
    cancel: &AtomicBool,
) -> Result<Option<ModelRecord>, IpcError> {
    // 1. readable
    steps.run("readable");
    let p = PathBuf::from(path);
    if !p.exists() {
        steps.fail("readable", "addModel.fail.notFound")?;
        return Ok(None);
    }
    steps.ok("readable", None);
    cancelled(cancel)?;

    // 2. format: GGUF instruction model with a chat template
    steps.run("format");
    let (files, probe) = match llm::locate(&p).and_then(|f| llm::probe(&f.model).map(|pr| (f, pr)))
    {
        Ok(x) => x,
        Err(_) => {
            steps.fail("format", "addModel.fail.format.llm")?;
            return Ok(None);
        }
    };
    steps.ok(
        "format",
        format!(
            "{} · {} · {}{}",
            probe.architecture,
            probe.size_label.as_deref().unwrap_or("?"),
            probe.precision,
            if files.mmproj.is_some() {
                " + image"
            } else {
                ""
            }
        ),
    );
    cancelled(cancel)?;

    // 3. checksum (model, then projector)
    steps.run("checksum");
    let mut pinned = true;
    let mut hash = String::new();
    for f in std::iter::once(&files.model).chain(files.mmproj.iter()) {
        cancelled(cancel)?;
        match eg2::sha256_file(f, |_, _| {}) {
            Ok(h) => {
                pinned &= llm::is_pinned(&h);
                if hash.is_empty() {
                    hash = h;
                }
            }
            Err(_) => {
                steps.fail("checksum", "addModel.fail.readable.llm")?;
                return Ok(None);
            }
        }
    }
    steps.ok(
        "checksum",
        Some(
            if pinned {
                "addModel.detail.pinned"
            } else {
                "addModel.detail.unpinned"
            }
            .to_string(),
        ),
    );
    cancelled(cancel)?;

    // 4. runtime
    steps.run("runtime");
    let Ok(found) = locate::find_llama_server(None) else {
        steps.fail("runtime", "addModel.fail.runtime")?;
        return Ok(None);
    };
    let version = locate::version(&found.path).unwrap_or_else(|_| "unknown".into());
    steps.ok("runtime", format!("llama.cpp {version}"));
    cancelled(cancel)?;

    // 5. smoke: one schema-constrained answer from a private worker
    steps.run("smoke");
    let cfg = LlamaServerConfig::chat(found.path, files.model.clone(), files.mmproj.clone());
    let vision = match smoke::llm_smoke(cfg, cancel) {
        Ok(r) => {
            steps.ok(
                "smoke",
                format!(
                    "JSON ✓ · image {} · {:.1} s load · {:.1} s answer",
                    match r.vision {
                        Some(true) => "✓",
                        Some(false) => "✗",
                        None => "–",
                    },
                    r.cold_load.as_secs_f64(),
                    r.answer.as_secs_f64()
                ),
            );
            r.vision == Some(true)
        }
        Err(RuntimeError::Cancelled) => {
            return Err(IpcError::new(ErrorCode::UserCancelled, "cancelled"))
        }
        Err(_) => {
            steps.fail("smoke", "addModel.fail.smoke.llm")?;
            return Ok(None);
        }
    };

    let display = probe
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "LLM".into());
    Ok(Some(ModelRecord {
        model_id: new_model_id(),
        category: ModelCategory::Llm,
        display_name: display,
        provider: Some(if pinned {
            format!("{} · {}", llm::PINNED_REPO, probe.precision)
        } else {
            format!("GGUF · {}", probe.precision)
        }),
        path: path.to_string(),
        format: "gguf".into(),
        architecture: Some(probe.architecture.clone()),
        weights_sha256: Some(hash),
        model_revision: pinned.then(|| llm::PINNED_REPO.to_string()),
        license_name: probe.license.clone(),
        runtime_adapter: "llama.cpp".into(),
        adapter_version: version,
        capabilities: if vision {
            vec![
                "search_plan".into(),
                "result_pick".into(),
                "image_caption".into(),
            ]
        } else {
            vec!["search_plan".into(), "result_pick".into()]
        },
        embedding_dim: None,
        status: ModelStatus::Available,
        last_error_code: None,
        validated_at: None,
        ttl: TtlPolicy::Minutes5,
    }))
}

fn run_validation(
    app: &AppHandle,
    req: RegisterModelRequest,
    channel: Channel<Vec<ValidationStep>>,
    cancel: Arc<AtomicBool>,
) -> Result<bool, IpcError> {
    let path = req.path.trim().to_string();
    let fmt_key = format!("addModel.step.format.{}", req.category.as_db_str());
    if !runnable(req.category) {
        let mut steps = Steps::new(
            channel,
            &[
                ("readable", "addModel.step.readable"),
                ("format", fmt_key.as_str()),
            ],
        );
        steps.run("readable");
        if !Path::new(&path).exists() {
            return steps.fail("readable", "addModel.fail.notFound");
        }
        steps.ok("readable", None);
        return steps.fail("format", "addModel.fail.notYet");
    }
    let mut steps = Steps::new(
        channel,
        &[
            ("readable", "addModel.step.readable"),
            ("format", fmt_key.as_str()),
            ("checksum", "addModel.step.checksum"),
            ("runtime", "addModel.step.runtime"),
            ("smoke", "addModel.step.smoke"),
        ],
    );
    let record = match req.category {
        ModelCategory::Llm => validate_llm(&path, &mut steps, &cancel)?,
        _ => validate_embedding(&path, &mut steps, &cancel)?,
    };
    let Some(record) = record else {
        return Ok(false);
    };
    let state = app.state::<AppState>();
    *state.model_ops.validated.lock().map_err(|_| lock_err())? = Some(Validated {
        category: req.category,
        path,
        record,
        at: Instant::now(),
    });
    Ok(true)
}

#[tauri::command]
pub async fn validate_model(
    app: AppHandle,
    req: RegisterModelRequest,
    on_step: Channel<Vec<ValidationStep>>,
) -> Result<bool, IpcError> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let state = app.state::<AppState>();
        let mut slot = state
            .model_ops
            .validation_cancel
            .lock()
            .map_err(|_| lock_err())?;
        // One validation at a time: a new one cancels the previous.
        if let Some(prev) = slot.replace(cancel.clone()) {
            prev.store(true, Ordering::SeqCst);
        }
    }
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || run_validation(&app2, req, on_step, cancel))
        .await
        .map_err(|_| IpcError::new(ErrorCode::Internal, "validation task failed"))?
}

#[tauri::command]
pub fn cancel_validation(state: State<'_, AppState>) -> Result<(), IpcError> {
    if let Some(c) = state
        .model_ops
        .validation_cancel
        .lock()
        .map_err(|_| lock_err())?
        .take()
    {
        c.store(true, Ordering::SeqCst);
    }
    Ok(())
}

// ---------------------------------------------------------------- register / lifecycle

#[tauri::command]
pub fn register_model(
    state: State<'_, AppState>,
    req: RegisterModelRequest,
) -> Result<ModelEntry, IpcError> {
    let v = state
        .model_ops
        .validated
        .lock()
        .map_err(|_| lock_err())?
        .take()
        .filter(|v| {
            v.category == req.category
                && v.path == req.path.trim()
                && v.at.elapsed() < VALIDATION_FRESH_FOR
        })
        .ok_or_else(|| IpcError::new(ErrorCode::ModelIncompatible, "validate the model first"))?;

    let previous: Vec<String> = {
        let mut guard = state.db.lock().map_err(|_| lock_err())?;
        let conn = guard
            .as_mut()
            .ok_or_else(|| IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable"))?;
        let prev = registry::active(conn)?
            .into_iter()
            .filter(|r| r.category == v.category)
            .map(|r| r.model_id)
            .collect();
        registry::activate(conn, &v.record)?;
        prev
    };
    // The old model's worker (if any) stops before the new one is known.
    for id in previous {
        state.models.remove(&id);
    }
    let rec = {
        let guard = state.db.lock().map_err(|_| lock_err())?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable"))?;
        registry::get(conn, &v.record.model_id)?
            .ok_or_else(|| IpcError::new(ErrorCode::Internal, "saved model not found"))?
    };
    attach(&state.models, &rec);
    Ok(entry(&rec, &state.models))
}

fn record(state: &AppState, id: &str) -> Result<ModelRecord, IpcError> {
    let guard = state.db.lock().map_err(|_| lock_err())?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable"))?;
    registry::get(conn, id)?.ok_or_else(|| IpcError::new(ErrorCode::ModelMissing, "unknown model"))
}

/// Manual "Load": start the worker now; it then follows its idle policy.
#[tauri::command]
pub async fn load_model(app: AppHandle, model_id: String) -> Result<(), IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let rec = record(&state, &model_id)?;
        if !runnable(rec.category) {
            return Err(IpcError::new(
                ErrorCode::NotImplemented,
                "model type arrives later",
            ));
        }
        if state.models.snapshot(&model_id).is_none() {
            attach(&state.models, &rec);
        }
        let lease = state.models.acquire(&model_id).map_err(ipc)?;
        drop(lease);
        Ok(())
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "load task failed"))?
}

#[tauri::command]
pub async fn unload_model(app: AppHandle, model_id: String) -> Result<(), IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        match state.models.unload(&model_id) {
            Ok(_) | Err(RuntimeError::UnknownModel) => Ok(()),
            Err(e) => Err(ipc(e)),
        }
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "unload task failed"))?
}

#[tauri::command]
pub async fn unload_all_models(app: AppHandle) -> Result<(), IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>().models.unload_all();
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "unload task failed"))
}

#[tauri::command]
pub fn set_model_ttl(
    state: State<'_, AppState>,
    model_id: String,
    ttl: TtlPolicy,
) -> Result<(), IpcError> {
    {
        let guard = state.db.lock().map_err(|_| lock_err())?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable"))?;
        registry::set_ttl(conn, &model_id, &ttl)?;
    }
    match state.models.set_policy(&model_id, policy(&ttl)) {
        Ok(()) | Err(RuntimeError::UnknownModel) => Ok(()),
        Err(e) => Err(ipc(e)),
    }
}

/// Forget a model; its files stay where they are.
#[tauri::command]
pub async fn remove_model(app: AppHandle, model_id: String) -> Result<(), IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        state.models.remove(&model_id);
        let guard = state.db.lock().map_err(|_| lock_err())?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| IpcError::new(ErrorCode::DbMigrationFailed, "database unavailable"))?;
        registry::remove(conn, &model_id)?;
        Ok(())
    })
    .await
    .map_err(|_| IpcError::new(ErrorCode::Internal, "remove task failed"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_maps_to_runtime_policy() {
        assert_eq!(policy(&TtlPolicy::Immediately), Policy::UnloadAfterJob);
        assert_eq!(policy(&TtlPolicy::Never), Policy::KeepLoaded);
        assert_eq!(
            policy(&TtlPolicy::Minutes5),
            Policy::IdleTimeout(Duration::from_secs(300))
        );
    }

    #[test]
    fn ipc_errors_never_carry_runtime_text() {
        let e = ipc(RuntimeError::WorkerExited(
            "exit 1\nloading C:\\Users\\me\\private\\model.gguf".into(),
        ));
        assert_eq!(e.code, ErrorCode::RuntimeLoadFailed);
        assert!(!e.detail.contains("Users"));
    }
}
