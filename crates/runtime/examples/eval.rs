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
//! With `--llm`, each LLM job is measured on its own against the
//! rules-only pipeline, so a job that does not help can be turned off
//! (Settings → Models → LLM jobs):
//!
//!   rules   no LLM
//!   plan    LLM understands the request only
//!   pick    LLM picks / answers from the results only
//!   all     both
//!
//! Images (`examples/eval/images/`) are indexed twice, without and with
//! LLM image descriptions, to measure the scan-time "caption" job. OCR runs
//! on Windows only; elsewhere the no-caption images have no text.
//!
//! Options:
//!   --llm <folder>        optional; without it only "rules" runs
//!   --modes <list>        e.g. rules,pick (default: all four with --llm)
//!   --corpus <folder>     default examples/eval/corpus
//!   --queries <file>      default examples/eval/queries.jsonl
//!   --images <folder>     default examples/eval/images ("none" to skip)
//!   --image-queries <f>   default examples/eval/images.jsonl
//!   --json <file>         write per-query results
//!   --server <exe>        llama-server (default: PATH / winget)
//!   --cpu                 force CPU

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tamgabul_core::describe::{self, Describer, NoDescriber};
use tamgabul_core::dto::TtlPolicy;
use tamgabul_core::engine::{self, Query, QueryModels, Roles};
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::history::SessionContext;
use tamgabul_core::index::job::{self, Embedder, JobInput};
use tamgabul_core::index::media::Platform;
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
    modes: Option<Vec<Mode>>,
    corpus: PathBuf,
    queries: PathBuf,
    images: Option<PathBuf>,
    image_queries: PathBuf,
    json: Option<PathBuf>,
    server: Option<PathBuf>,
    cpu: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Rules,
    Plan,
    Pick,
    All,
}

impl Mode {
    fn parse(s: &str) -> Mode {
        match s.trim() {
            "rules" => Mode::Rules,
            "plan" => Mode::Plan,
            "pick" => Mode::Pick,
            "all" => Mode::All,
            other => die(&format!("unknown mode {other} (rules, plan, pick, all)")),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Mode::Rules => "rules",
            Mode::Plan => "plan",
            Mode::Pick => "pick",
            Mode::All => "all",
        }
    }
    fn roles(self) -> Roles {
        Roles {
            plan: matches!(self, Mode::Plan | Mode::All),
            pick: matches!(self, Mode::Pick | Mode::All),
        }
    }
}

fn args() -> Args {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/eval");
    let mut a = Args {
        embedding: PathBuf::new(),
        llm: None,
        modes: None,
        corpus: here.join("corpus"),
        queries: here.join("queries.jsonl"),
        images: Some(here.join("images")),
        image_queries: here.join("images.jsonl"),
        json: None,
        server: None,
        cpu: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        let mut v = || {
            it.next()
                .unwrap_or_else(|| die(&format!("{k} needs a value")))
        };
        match k.as_str() {
            "--embedding" => a.embedding = v().into(),
            "--llm" => a.llm = Some(v().into()),
            "--modes" => a.modes = Some(v().split(',').map(Mode::parse).collect()),
            "--corpus" => a.corpus = v().into(),
            "--queries" => a.queries = v().into(),
            "--images" => {
                let p = v();
                a.images = (p != "none").then(|| p.into());
            }
            "--image-queries" => a.image_queries = v().into(),
            "--json" => a.json = Some(v().into()),
            "--server" => a.server = Some(v().into()),
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

/// Image descriptions with the LLM, as the app writes them while scanning.
struct Captions<'a>(&'a Client);

impl Describer for Captions<'_> {
    fn available(&mut self) -> bool {
        true
    }
    fn describe_image(&mut self, png: &[u8]) -> Result<Option<String>, IpcError> {
        let v = self
            .0
            .chat_json(
                &[
                    ChatMessage::system(describe::CAPTION_SYSTEM),
                    ChatMessage::user_with_png(describe::CAPTION_USER, png.to_vec()),
                ],
                &describe::caption_schema(),
                400,
            )
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(describe::parse_caption(&v))
    }
}

/// The OS services the app uses (OCR, image decoding; Windows only).
struct OsMedia;

impl Platform for OsMedia {
    fn decode_to_png(&self, bytes: &[u8], max_side: u32) -> Option<Vec<u8>> {
        tamgabul_platform::media::decode_to_png(bytes, max_side).ok()
    }
    fn ocr(&self, png: &[u8]) -> Option<Result<String, String>> {
        match tamgabul_platform::media::ocr(png) {
            Ok(t) => Some(Ok(t.text())),
            Err(tamgabul_platform::media::MediaError::Unsupported) => None,
            Err(e) => Some(Err(e.to_string())),
        }
    }
    fn ocr_available(&self) -> bool {
        tamgabul_platform::media::ocr_supported()
    }
    fn render_pdf_pages(&self, _: &[u8], _: &[u32], _: u32) -> Option<Vec<(u32, Vec<u8>)>> {
        None
    }
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

struct Index {
    _dir: tempfile::TempDir,
    conn: rusqlite::Connection,
    generation: String,
    files: u32,
    secs: f64,
}

/// Index one folder read-only into a throwaway database.
fn index(folder: &Path, ec: &Client, describer: &mut dyn Describer) -> Index {
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
    let canonical = walk::canonical(folder).unwrap_or_else(|_| die("corpus folder not found"));
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
            platform: &OsMedia,
        },
        &mut Docs(ec),
        describer,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap_or_else(|e| die(&e.detail));
    Index {
        _dir: tmp,
        conn,
        generation,
        files: p.indexed,
        secs: t.elapsed().as_secs_f64(),
    }
}

struct Run {
    label: String,
    by_kind: BTreeMap<String, Tally>,
    all: Tally,
    none_n: u32,
    median_ms: u128,
    max_ms: u128,
    report: Vec<Value>,
}

fn read_queries(p: &Path) -> Vec<Value> {
    std::fs::read_to_string(p)
        .unwrap_or_else(|_| die("queries file not found"))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|_| die("bad queries line")))
        .collect()
}

fn run_set(label: &str, ix: &Index, models: &Models<'_>, roles: Roles, queries: &[Value]) -> Run {
    let mut by_kind: BTreeMap<String, Tally> = BTreeMap::new();
    let mut all = Tally::default();
    let mut times = Vec::new();
    let mut report = Vec::new();
    let mut misses = Vec::new();
    for q in queries {
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
            &ix.conn,
            Some(&ix.generation),
            models,
            &Query {
                text: query,
                ctx: &ctx,
                allow_clarify: false,
                now: 1_793_700_000,
                utc_offset: 0,
                roles,
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
        // "Not found" accuracy: nothing shown, or (with picks) nothing picked.
        let empty = titles.is_empty() || out.llm_unsure;
        let tally = by_kind.entry(kind.to_string()).or_default();
        for t in [&mut *tally, &mut all] {
            t.n += 1;
            if want.is_empty() {
                t.empty += empty as u32;
            } else if let Some(r) = rank {
                t.hit1 += (r == 1) as u32;
                t.hit3 += (r <= 3) as u32;
                t.rr += 1.0 / r as f64;
            }
        }
        let ok = if want.is_empty() {
            empty
        } else {
            rank == Some(1)
        };
        if !ok {
            misses.push(format!(
                "    {id} {query:?} → {:?}",
                titles.iter().take(3).collect::<Vec<_>>()
            ));
        }
        report.push(json!({
            "run": label, "id": id, "kind": kind, "query": query, "expect": want, "rank": rank,
            "shown": titles, "picked": out.picked_count, "notice": out.notice,
            "answer": out.direct.map(|d| d.text), "summary": out.summary.map(|s| s.text),
            "ms": took.as_millis() as u64,
        }));
    }
    println!("\n  [{label}] misses:");
    for m in &misses {
        println!("{m}");
    }
    if misses.is_empty() {
        println!("    none");
    }
    times.sort();
    let none_n = by_kind.get("none").map(|x| x.n).unwrap_or(0);
    let run = Run {
        label: label.into(),
        by_kind,
        all,
        none_n,
        median_ms: times
            .get(times.len() / 2)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        max_ms: times.last().map(|d| d.as_millis()).unwrap_or(0),
        report,
    };
    print_table(&run);
    run
}

fn scored(t: &Tally, k: &str, none_n: u32) -> u32 {
    match k {
        "none" => 0,
        "ALL" => t.n - none_n,
        _ => t.n,
    }
}

fn print_table(r: &Run) {
    println!(
        "  {:<14} {:>4} {:>7} {:>7} {:>6} {:>10}",
        "kind", "n", "hit@1", "hit@3", "MRR", "not found"
    );
    let all = "ALL".to_string();
    for (k, t) in r.by_kind.iter().chain(std::iter::once((&all, &r.all))) {
        let n = scored(t, k, r.none_n);
        let dash = || "-".to_string();
        println!(
            "  {:<14} {:>4} {:>7} {:>7} {:>6} {:>10}",
            k,
            t.n,
            if n == 0 { dash() } else { pct(t.hit1, n) },
            if n == 0 { dash() } else { pct(t.hit3, n) },
            if n == 0 {
                dash()
            } else {
                format!("{:.2}", t.rr / n as f64)
            },
            if k == "none" || k == "ALL" {
                pct(t.empty, r.none_n)
            } else {
                dash()
            },
        );
    }
    println!(
        "  latency        median {} ms · max {} ms",
        r.median_ms, r.max_ms
    );
}

/// One line per run: overall numbers and the change against the first run.
fn compare(title: &str, runs: &[Run]) {
    let Some(base) = runs.first() else { return };
    println!("\n== {title} ==");
    println!(
        "  {:<18} {:>7} {:>7} {:>6} {:>10} {:>9}   vs {}",
        "run", "hit@1", "hit@3", "MRR", "not found", "median", base.label
    );
    let f = |r: &Run| {
        let n = scored(&r.all, "ALL", r.none_n).max(1) as f64;
        (
            100.0 * r.all.hit1 as f64 / n,
            100.0 * r.all.hit3 as f64 / n,
            r.all.rr / n,
            if r.none_n == 0 {
                0.0
            } else {
                100.0 * r.all.empty as f64 / r.none_n as f64
            },
        )
    };
    let b = f(base);
    for r in runs {
        let x = f(r);
        let delta = if std::ptr::eq(r, base) {
            String::new()
        } else {
            format!(
                "hit@1 {:+.0} · MRR {:+.2} · not found {:+.0}",
                x.0 - b.0,
                x.2 - b.2,
                x.3 - b.3
            )
        };
        println!(
            "  {:<18} {:>6.0}% {:>6.0}% {:>6.2} {:>9.0}% {:>6} ms   {}",
            r.label, x.0, x.1, x.2, x.3, r.median_ms, delta
        );
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
    let mut vision = false;
    let llm_worker = a.llm.as_ref().map(|p| {
        let f = llm::locate(p).unwrap_or_else(|e| die(&format!("llm: {e}")));
        vision = f.mmproj.is_some();
        let mut c = LlamaServerConfig::chat(exe.clone(), f.model, f.mmproj);
        if a.cpu {
            c.gpu_layers = GpuLayers::None;
        }
        start(c)
    });
    let llm_client = llm_worker.as_ref().map(|(_, c)| c);
    println!(
        "  models         EmbeddingGemma 2{}",
        match (llm_client.is_some(), vision) {
            (true, true) => " + LLM (with vision)",
            (true, false) => " + LLM (no vision projector: captions not measured)",
            _ => " (rules only, no LLM)",
        }
    );
    println!(
        "  OCR            {}",
        if tamgabul_platform::media::ocr_supported() {
            "available"
        } else {
            "not available on this system"
        }
    );
    let modes = a.modes.clone().unwrap_or_else(|| {
        if llm_client.is_some() {
            vec![Mode::Rules, Mode::Plan, Mode::Pick, Mode::All]
        } else {
            vec![Mode::Rules]
        }
    });
    if llm_client.is_none() && modes.iter().any(|m| *m != Mode::Rules) {
        die("modes other than rules need --llm");
    }
    let models_for = |m: Mode| Models {
        emb: &ec,
        llm: if m == Mode::Rules { None } else { llm_client },
    };

    // Text corpus: each mode on the same index.
    let ix = index(&a.corpus, &ec, &mut NoDescriber);
    println!("  text corpus    {} files in {:.1} s", ix.files, ix.secs);
    let queries = read_queries(&a.queries);
    let mut report = Vec::new();
    let mut runs = Vec::new();
    for m in &modes {
        let r = run_set(m.name(), &ix, &models_for(*m), m.roles(), &queries);
        report.extend(r.report.iter().cloned());
        runs.push(r);
    }
    compare("text: LLM jobs on/off", &runs);

    // Images: without and with descriptions written at scan time.
    let mut img_runs = Vec::new();
    if let Some(folder) = &a.images {
        let iq = read_queries(&a.image_queries);
        // The app default (pick only) when it is measured, else the last mode.
        let last = if modes.contains(&Mode::Pick) {
            Mode::Pick
        } else {
            *modes.last().unwrap_or(&Mode::Rules)
        };
        let plain = index(folder, &ec, &mut NoDescriber);
        println!(
            "\n  images         {} files in {:.1} s (no captions)",
            plain.files, plain.secs
        );
        let r = run_set(
            &format!("images/{}", last.name()),
            &plain,
            &models_for(last),
            last.roles(),
            &iq,
        );
        report.extend(r.report.iter().cloned());
        img_runs.push(r);
        if let (Some(c), true) = (llm_client, vision) {
            let cap = index(folder, &ec, &mut Captions(c));
            println!(
                "\n  images         {} files in {:.1} s (with captions)",
                cap.files, cap.secs
            );
            for m in [Mode::Rules, last] {
                let r = run_set(
                    &format!("images+caption/{}", m.name()),
                    &cap,
                    &models_for(m),
                    m.roles(),
                    &iq,
                );
                report.extend(r.report.iter().cloned());
                img_runs.push(r);
                if m == last {
                    break;
                }
            }
        }
        compare("images: captions off/on", &img_runs);
    }

    if let Some(p) = &a.json {
        std::fs::write(p, serde_json::to_string_pretty(&report).unwrap_or_default())
            .unwrap_or_else(|_| die("could not write the report"));
        println!(
            "\n  report         {}",
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
