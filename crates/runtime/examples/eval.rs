//! Search quality on the bilingual evaluation set (`examples/eval/`) with
//! the real models, through the same engine as the app
//! (`tamgabul_core::engine`). Developer tool, not shipped. Prints measured
//! numbers only and changes no files: the corpus is indexed read-only into
//! a temporary database.
//!
//!   cargo run -p tamgabul-runtime --release --example eval -- `
//!       --embedding C:\AI\models\embeddinggemma-2 `
//!       --llm C:\AI\models\gemma-4-e2b-it
//!
//! Options:
//!   --llm <folder>        optional; without it the rules-only pipeline runs
//!   --corpus <folder>     default examples/eval/corpus
//!   --queries <file>      default examples/eval/queries.jsonl
//!   --json <file>         write per-query results
//!   --server <exe>        llama-server (default: PATH / winget)
//!   --cpu                 force CPU

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tamgabul_core::describe::NoDescriber;
use tamgabul_core::dto::TtlPolicy;
use tamgabul_core::engine::{self, Query, QueryModels};
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::history::SessionContext;
use tamgabul_core::index::job::{self, Embedder, JobInput};
use tamgabul_core::index::media::NoPlatform;
use tamgabul_core::index::store::{self, GenerationSpec, Scope, ScopeState};
use tamgabul_core::index::walk::{self, Limits};
use tamgabul_core::model::{ModelCategory, ModelStatus};
use tamgabul_core::registry::{self, ModelRecord};
use tamgabul_runtime::client::{ChatMessage, Client};
use tamgabul_runtime::worker::{
    GpuLayers, LlamaServerConfig, ProcessLauncher, ProcessWorker, Worker,
};
use tamgabul_runtime::{eg2, llm, locate};

fn die(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(2)
}

struct Args {
    embedding: PathBuf,
    llm: Option<PathBuf>,
    corpus: PathBuf,
    queries: PathBuf,
    json: Option<PathBuf>,
    server: Option<PathBuf>,
    cpu: bool,
}

fn args() -> Args {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/eval");
    let mut a = Args {
        embedding: PathBuf::new(),
        llm: None,
        corpus: here.join("corpus"),
        queries: here.join("queries.jsonl"),
        json: None,
        server: None,
        cpu: false,
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
            "--embedding" => a.embedding = v(),
            "--llm" => a.llm = Some(v()),
            "--corpus" => a.corpus = v(),
            "--queries" => a.queries = v(),
            "--json" => a.json = Some(v()),
            "--server" => a.server = Some(v()),
            "--cpu" => a.cpu = true,
            other => die(&format!("unknown option {other}")),
        }
    }
    if a.embedding.as_os_str().is_empty() {
        die("--embedding <EmbeddingGemma 2 folder> is required");
    }
    a
}

fn start(cfg: LlamaServerConfig) -> (ProcessWorker, Client) {
    let launcher = ProcessLauncher::llama_server(cfg);
    let w = launcher
        .launch_process(&AtomicBool::new(false))
        .unwrap_or_else(|e| {
            die(&format!(
                "worker: {}",
                e.to_string().lines().next().unwrap_or("")
            ))
        });
    let c = Client::new(w.endpoint().clone(), Duration::from_secs(120));
    (w, c)
}

struct Docs<'a>(&'a Client);

impl Embedder for Docs<'_> {
    fn embed_documents(
        &mut self,
        title: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, IpcError> {
        let inputs: Vec<String> = texts
            .iter()
            .map(|t| eg2::document_prompt(Some(title), t))
            .collect();
        let mut v = self
            .0
            .embed_texts(&inputs)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        v.iter_mut().for_each(|x| eg2::normalize(x));
        Ok(v)
    }
    fn supports_vision(&mut self) -> bool {
        false
    }
    fn supports_audio(&mut self) -> bool {
        false
    }
    fn embed_media(&mut self, _b: &[u8]) -> Result<Vec<f32>, IpcError> {
        Err(IpcError::new(ErrorCode::ModelIncompatible, "text only"))
    }
}

struct Models<'a> {
    emb: &'a Client,
    llm: Option<&'a Client>,
}

impl QueryModels for Models<'_> {
    fn embed_query(&self, text: &str) -> Option<Vec<f32>> {
        let mut v = self
            .emb
            .embed_texts(&[eg2::query_prompt(eg2::Task::SearchResult, text)])
            .ok()?
            .pop()?;
        eg2::is_valid_vector(&v).then(|| {
            eg2::normalize(&mut v);
            v
        })
    }
    fn has_llm(&self) -> bool {
        self.llm.is_some()
    }
    fn llm_json(&self, system: &str, user: &str, schema: &Value, max: u32) -> Option<Value> {
        self.llm?
            .chat_json(
                &[ChatMessage::system(system), ChatMessage::user(user)],
                schema,
                max,
            )
            .ok()
    }
}

#[derive(Default)]
struct Tally {
    n: u32,
    hit1: u32,
    hit3: u32,
    rr: f64,
    empty: u32,
}

fn pct(a: u32, n: u32) -> String {
    if n == 0 {
        "-".into()
    } else {
        format!("{:.0}%", 100.0 * a as f64 / n as f64)
    }
}

fn main() {
    let a = args();
    let exe = locate::find_llama_server(a.server.as_deref())
        .unwrap_or_else(|_| die("llama-server not found"))
        .path;
    println!("== Tamgabul search evaluation ==");
    println!(
        "  llama-server   {}",
        locate::version(&exe).unwrap_or_else(|_| "unknown".into())
    );

    let eg = eg2::locate(&a.embedding).unwrap_or_else(|e| die(&format!("embedding: {e}")));
    let mut ecfg = LlamaServerConfig::eg2(exe.clone(), eg.model.clone(), None);
    if a.cpu {
        ecfg.gpu_layers = GpuLayers::None;
    }
    let (ew, ec) = start(ecfg);
    let llm_worker = a.llm.as_ref().map(|p| {
        let f = llm::locate(p).unwrap_or_else(|e| die(&format!("llm: {e}")));
        let mut c = LlamaServerConfig::chat(exe.clone(), f.model, None);
        if a.cpu {
            c.gpu_layers = GpuLayers::None;
        }
        start(c)
    });
    println!(
        "  models         EmbeddingGemma 2{}",
        if llm_worker.is_some() {
            " + LLM"
        } else {
            " (rules only, no LLM)"
        }
    );

    // Index the corpus into a throwaway database.
    let tmp = tempfile::tempdir().unwrap_or_else(|e| die(&e.to_string()));
    let mut conn = tamgabul_core::db::open_and_migrate(&tmp.path().join("eval.db"))
        .unwrap_or_else(|e| die(&e.detail));
    registry::activate(
        &mut conn,
        &ModelRecord {
            model_id: "m-eval".into(),
            category: ModelCategory::Embedding,
            display_name: "EmbeddingGemma 2".into(),
            provider: None,
            path: String::new(),
            format: "gguf".into(),
            architecture: Some(eg2::ARCH.into()),
            weights_sha256: None,
            model_revision: None,
            license_name: None,
            runtime_adapter: "llama.cpp".into(),
            adapter_version: "eval".into(),
            capabilities: vec!["text".into()],
            embedding_dim: Some(eg2::NATIVE_DIM as u32),
            status: ModelStatus::Available,
            last_error_code: None,
            validated_at: None,
            ttl: TtlPolicy::Never,
        },
    )
    .unwrap_or_else(|e| die(&e.detail));
    let generation = store::ensure_current_generation(
        &mut conn,
        &GenerationSpec {
            embedding_model_id: "m-eval".into(),
            model_hash: "eval".into(),
            model_revision: "eval".into(),
            adapter_version: "eval".into(),
            preprocessing: "eval".into(),
            query_instruction: eg2::query_prompt(eg2::Task::SearchResult, "{query}"),
            document_instruction: eg2::document_prompt(Some("{title}"), "{content}"),
            dimension: eg2::NATIVE_DIM as u32,
            normalization: "l2".into(),
            modalities: vec!["text".into()],
        },
    )
    .unwrap_or_else(|e| die(&e.detail));
    let canonical = walk::canonical(&a.corpus).unwrap_or_else(|_| die("corpus folder not found"));
    let scope = Scope {
        scope_id: "s-eval".into(),
        display_path: walk::display(&canonical),
        is_drive_root: false,
        canonical_path: canonical,
        state: ScopeState::Active,
        file_count: 0,
        last_scan_at: None,
        error_summary: None,
        max_file_bytes: None,
    };
    store::insert_scope(&conn, &scope).unwrap_or_else(|e| die(&e.detail));
    let t = Instant::now();
    let (_, p) = job::run(
        &mut conn,
        JobInput {
            scope_id: &scope.scope_id,
            generation: &generation,
            deny: &[],
            limits: Limits::default(),
            batch: 8,
            platform: &NoPlatform,
        },
        &mut Docs(&ec),
        &mut NoDescriber,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap_or_else(|e| die(&e.detail));
    println!(
        "  indexed        {} files in {:.1} s",
        p.indexed,
        t.elapsed().as_secs_f64()
    );

    let models = Models {
        emb: &ec,
        llm: llm_worker.as_ref().map(|(_, c)| c),
    };
    let text =
        std::fs::read_to_string(&a.queries).unwrap_or_else(|_| die("queries file not found"));
    let mut by_kind: BTreeMap<String, Tally> = BTreeMap::new();
    let mut all = Tally::default();
    let mut times = Vec::new();
    let mut report = Vec::new();
    println!("\n  misses:");
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let q: Value = serde_json::from_str(line).unwrap_or_else(|_| die("bad queries line"));
        let (id, query, kind) = (
            q["id"].as_str().unwrap_or("?"),
            q["query"].as_str().unwrap_or(""),
            q["kind"].as_str().unwrap_or("?"),
        );
        let want: Vec<&str> = q["expect"]
            .as_array()
            .map(|v| v.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let ctx = SessionContext::default();
        let t = Instant::now();
        let out = engine::run(
            &conn,
            Some(&generation),
            &models,
            &Query {
                text: query,
                ctx: &ctx,
                allow_clarify: false,
                now: 1_793_700_000,
                utc_offset: 0,
            },
        )
        .unwrap_or_else(|e| die(&e.detail));
        let took = t.elapsed();
        times.push(took);
        let titles: Vec<String> = out
            .found
            .iter()
            .map(|f| f.resolved.card.title.clone())
            .collect();
        let rank = titles
            .iter()
            .position(|t| want.contains(&t.as_str()))
            .map(|r| r + 1);
        let tally = by_kind.entry(kind.to_string()).or_default();
        for t in [&mut *tally, &mut all] {
            t.n += 1;
            if want.is_empty() {
                t.empty += titles.is_empty() as u32;
            } else if let Some(r) = rank {
                t.hit1 += (r == 1) as u32;
                t.hit3 += (r <= 3) as u32;
                t.rr += 1.0 / r as f64;
            }
        }
        let ok = if want.is_empty() {
            titles.is_empty()
        } else {
            rank == Some(1)
        };
        if !ok {
            println!(
                "    {id} {query:?} → {:?}",
                titles.iter().take(3).collect::<Vec<_>>()
            );
        }
        report.push(json!({
            "id": id, "kind": kind, "query": query, "expect": want, "rank": rank,
            "shown": titles, "picked_by_llm": out.picked_by_llm,
            "answer": out.direct.map(|d| d.text), "summary": out.summary.map(|s| s.text),
            "ms": took.as_millis() as u64,
        }));
    }

    let none_n = by_kind.get("none").map(|x| x.n).unwrap_or(0);
    println!(
        "\n  {:<14} {:>4} {:>7} {:>7} {:>6} {:>10}",
        "kind", "n", "hit@1", "hit@3", "MRR", "not found"
    );
    for (k, t) in by_kind
        .iter()
        .chain(std::iter::once((&"ALL".to_string(), &all)))
    {
        // Requests with an expected file vs. "nothing relevant exists".
        let scored = match k.as_str() {
            "none" => 0,
            "ALL" => t.n - none_n,
            _ => t.n,
        };
        let dash = || "-".to_string();
        println!(
            "  {:<14} {:>4} {:>7} {:>7} {:>6} {:>10}",
            k,
            t.n,
            if scored == 0 {
                dash()
            } else {
                pct(t.hit1, scored)
            },
            if scored == 0 {
                dash()
            } else {
                pct(t.hit3, scored)
            },
            if scored == 0 {
                dash()
            } else {
                format!("{:.2}", t.rr / scored as f64)
            },
            if k == "none" || k == "ALL" {
                pct(t.empty, none_n)
            } else {
                dash()
            },
        );
    }
    times.sort();
    println!(
        "\n  latency        median {} ms · max {} ms",
        times[times.len() / 2].as_millis(),
        times.last().map(|d| d.as_millis()).unwrap_or(0)
    );
    if let Some(p) = &a.json {
        std::fs::write(p, serde_json::to_string_pretty(&report).unwrap_or_default())
            .unwrap_or_else(|_| die("could not write the report"));
        println!(
            "  report         {}",
            Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        );
    }
    let _ = ew.terminate();
    if let Some((w, _)) = llm_worker {
        let _ = w.terminate();
    }
}
