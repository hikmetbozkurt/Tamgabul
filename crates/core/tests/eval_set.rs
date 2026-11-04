//! The bilingual evaluation set (`examples/eval/`) is well formed, and the
//! word + file-name part of search finds every `keyword` request in the
//! top 3 with no model at all. Meaning-based kinds need the real models:
//! `cargo run -p tamgabul-runtime --release --example eval`.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use serde_json::Value;
use tamgabul_core::db::open_and_migrate;
use tamgabul_core::describe::NoDescriber;
use tamgabul_core::engine::{self, Query, QueryModels};
use tamgabul_core::error::IpcError;
use tamgabul_core::history::SessionContext;
use tamgabul_core::index::job::{self, Embedder, JobInput, Outcome};
use tamgabul_core::index::media::NoPlatform;
use tamgabul_core::index::store::{self, GenerationSpec, Scope, ScopeState};
use tamgabul_core::index::walk::{self, Limits};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/eval")
}

fn queries() -> Vec<Value> {
    std::fs::read_to_string(root().join("queries.jsonl"))
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// Test-only: vectors exist but carry no meaning (search runs on words).
struct NoMeaning;
impl Embedder for NoMeaning {
    fn embed_documents(&mut self, _t: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, IpcError> {
        Ok(texts.iter().map(|_| vec![0.03; 768]).collect())
    }
    fn supports_vision(&mut self) -> bool {
        false
    }
    fn supports_audio(&mut self) -> bool {
        false
    }
    fn embed_media(&mut self, _b: &[u8]) -> Result<Vec<f32>, IpcError> {
        unreachable!()
    }
}

struct NoModels;
impl QueryModels for NoModels {
    fn embed_query(&self, _t: &str) -> Option<Vec<f32>> {
        None
    }
    fn has_llm(&self) -> bool {
        false
    }
    fn llm_json(&self, _s: &str, _u: &str, _sc: &Value, _m: u32) -> Option<Value> {
        None
    }
}

#[test]
fn set_is_well_formed() {
    let q = queries();
    assert!(q.len() >= 40);
    let mut ids = std::collections::HashSet::new();
    for x in &q {
        assert!(
            ids.insert(x["id"].as_str().unwrap().to_string()),
            "duplicate id"
        );
        let kind = x["kind"].as_str().unwrap();
        assert!(
            [
                "keyword",
                "paraphrase",
                "cross_lingual",
                "question",
                "typo",
                "none"
            ]
            .contains(&kind),
            "{kind}"
        );
        let expect = x["expect"].as_array().unwrap();
        assert_eq!(expect.is_empty(), kind == "none", "{}", x["id"]);
        for f in expect {
            let p = root().join("corpus").join(f.as_str().unwrap());
            assert!(p.is_file(), "missing {}", p.display());
        }
    }
}

#[test]
fn image_set_is_well_formed() {
    let text = std::fs::read_to_string(root().join("images.jsonl")).unwrap();
    let mut n = 0;
    for l in text.lines().filter(|l| !l.trim().is_empty()) {
        let x: Value = serde_json::from_str(l).unwrap();
        let kind = x["kind"].as_str().unwrap();
        assert!(["image", "image_text", "none"].contains(&kind), "{kind}");
        let expect = x["expect"].as_array().unwrap();
        assert_eq!(expect.is_empty(), kind == "none", "{}", x["id"]);
        for f in expect {
            let name = f.as_str().unwrap();
            assert!(root().join("images").join(name).is_file(), "missing {name}");
        }
        n += 1;
    }
    assert!(n >= 12);
}

#[test]
fn keyword_requests_are_found_without_models() {
    let db = tempfile::tempdir().unwrap();
    let mut conn = open_and_migrate(&db.path().join("e.db")).unwrap();
    conn.execute(
        "INSERT INTO model_registry(model_id, category, display_name, path, format, runtime_adapter, adapter_version, is_active, status)
         VALUES ('m','embedding','E','x','gguf','l','t',1,'AVAILABLE')",
        [],
    )
    .unwrap();
    let generation = store::ensure_current_generation(
        &mut conn,
        &GenerationSpec {
            embedding_model_id: "m".into(),
            model_hash: "h".into(),
            model_revision: "r".into(),
            adapter_version: "t".into(),
            preprocessing: "eval".into(),
            query_instruction: "q".into(),
            document_instruction: "d".into(),
            dimension: 768,
            normalization: "l2".into(),
            modalities: vec!["text".into()],
        },
    )
    .unwrap();
    let canonical = walk::canonical(&root().join("corpus")).unwrap();
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
    store::insert_scope(&conn, &scope).unwrap();
    let (out, p) = job::run(
        &mut conn,
        JobInput {
            scope_id: &scope.scope_id,
            generation: &generation,
            deny: &[],
            limits: Limits::default(),
            batch: 8,
            platform: &NoPlatform,
        },
        &mut NoMeaning,
        &mut NoDescriber,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out, Outcome::Done);
    assert_eq!(p.indexed, 24);

    let mut misses = Vec::new();
    for x in queries().iter().filter(|x| x["kind"] == "keyword") {
        let ctx = SessionContext::default();
        let r = engine::run(
            &conn,
            Some(&generation),
            &NoModels,
            &Query {
                text: x["query"].as_str().unwrap(),
                ctx: &ctx,
                allow_clarify: true,
                now: 1_793_700_000,
                utc_offset: 0,
                roles: Default::default(),
            },
        )
        .unwrap();
        let top: Vec<String> = r
            .found
            .iter()
            .take(3)
            .map(|f| f.resolved.card.title.clone())
            .collect();
        let want: Vec<&str> = x["expect"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        if !want.iter().any(|w| top.iter().any(|t| t == w)) {
            misses.push(format!("{} {:?} → {:?}", x["id"], x["query"], top));
        }
    }
    assert!(misses.is_empty(), "{misses:#?}");
}
