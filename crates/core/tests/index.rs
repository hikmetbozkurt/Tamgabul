//! P1 acceptance tests with temp folders and a test-only fake embedder:
//! scan, delta rescan, overlap + revoke choices, revoke during scan, purge
//! resume, and source files untouched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rusqlite::Connection;
use tamgabul_core::db::open_and_migrate;
use tamgabul_core::dto::RevokeMode;
use tamgabul_core::error::IpcError;
use tamgabul_core::index::job::{self, Embedder, JobInput, Outcome};
use tamgabul_core::index::store::{self, GenerationSpec, Scope, ScopeState};
use tamgabul_core::index::walk::{self, Limits};

struct FakeEmbedder {
    calls: usize,
    texts: usize,
    /// Called before each batch; lets a test revoke mid-scan.
    hook: Option<Box<dyn FnMut()>>,
}

impl FakeEmbedder {
    fn new() -> Self {
        Self {
            calls: 0,
            texts: 0,
            hook: None,
        }
    }
}

impl Embedder for FakeEmbedder {
    fn embed_documents(
        &mut self,
        _title: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, IpcError> {
        if let Some(h) = self.hook.as_mut() {
            h();
        }
        self.calls += 1;
        self.texts += texts.len();
        Ok(texts
            .iter()
            .map(|t| {
                let h = blake3::hash(t.as_bytes());
                (0..768)
                    .map(|i| h.as_bytes()[i % 32] as f32 / 255.0 + 0.01)
                    .collect()
            })
            .collect())
    }
}

fn spec(hash: &str) -> GenerationSpec {
    GenerationSpec {
        embedding_model_id: "m-test".into(),
        model_hash: hash.into(),
        model_revision: "rev".into(),
        adapter_version: "llama.cpp test".into(),
        preprocessing: "text-v1".into(),
        query_instruction: "task: search result | query: {q}".into(),
        document_instruction: "title: {t} | text: {d}".into(),
        dimension: 768,
        normalization: "l2".into(),
        modalities: vec!["text".into()],
    }
}

struct Env {
    _db: tempfile::TempDir,
    files: tempfile::TempDir,
    conn: Connection,
    generation: String,
}

fn env() -> Env {
    let db = tempfile::tempdir().unwrap();
    let mut conn = open_and_migrate(&db.path().join("t.db")).unwrap();
    // A registry row for the generation's FK.
    conn.execute(
        "INSERT INTO model_registry(model_id, category, display_name, path, format, runtime_adapter, adapter_version, is_active, status)
         VALUES ('m-test','embedding','EmbeddingGemma 2','x','gguf','llama.cpp','t',1,'AVAILABLE')",
        [],
    )
    .unwrap();
    let generation = store::ensure_current_generation(&mut conn, &spec("h1")).unwrap();
    let files = tempfile::tempdir().unwrap();
    let p = files.path();
    std::fs::create_dir_all(p.join("projects/auction")).unwrap();
    std::fs::create_dir_all(p.join("projects/private")).unwrap();
    std::fs::create_dir_all(p.join("node_modules/x")).unwrap();
    std::fs::write(
        p.join("projects/auction/timer.md"),
        "# Timer\nThe auction countdown timer restarts on late bids.\n",
    )
    .unwrap();
    std::fs::write(
        p.join("projects/notes.txt"),
        "Toplantı notları: sürüm cuma günü.\n",
    )
    .unwrap();
    std::fs::write(p.join("projects/private/diary.md"), "private thoughts\n").unwrap();
    std::fs::write(p.join("node_modules/x/readme.md"), "dependency readme\n").unwrap();
    std::fs::write(p.join("projects/picture.png"), [0x89, b'P', b'N', b'G']).unwrap();
    Env {
        _db: db,
        files,
        conn,
        generation,
    }
}

fn grant(e: &Env, rel: &str) -> Scope {
    let canonical = walk::canonical(&e.files.path().join(rel)).unwrap();
    let s = Scope {
        scope_id: store::new_id("s", rel),
        display_path: walk::display(&canonical),
        is_drive_root: walk::is_drive_root(&canonical),
        canonical_path: canonical,
        state: ScopeState::Active,
        file_count: 0,
        last_scan_at: None,
        error_summary: None,
        max_file_bytes: None,
    };
    store::insert_scope(&e.conn, &s).unwrap();
    s
}

fn scan(e: &mut Env, scope: &Scope, emb: &mut FakeEmbedder) -> (Outcome, job::Progress) {
    let generation = e.generation.clone();
    job::run(
        &mut e.conn,
        JobInput {
            scope_id: &scope.scope_id,
            generation: &generation,
            deny: &[],
            limits: Limits::default(),
            batch: 8,
        },
        emb,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap()
}

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn fts_hits(c: &Connection, q: &str) -> i64 {
    c.query_row(
        "SELECT COUNT(*) FROM chunk_fts WHERE chunk_fts MATCH ?1",
        [q],
        |r| r.get(0),
    )
    .unwrap()
}

fn hashes(root: &Path) -> HashMap<PathBuf, String> {
    let mut m = HashMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                m.insert(
                    p.clone(),
                    blake3::hash(&std::fs::read(&p).unwrap())
                        .to_hex()
                        .to_string(),
                );
            }
        }
    }
    m
}

#[test]
fn scan_indexes_text_with_vectors_and_rescans_only_changes() {
    let mut e = env();
    let before = hashes(e.files.path());
    let s = grant(&e, "projects");
    let mut emb = FakeEmbedder::new();
    let (out, p) = scan(&mut e, &s, &mut emb);
    assert_eq!(out, Outcome::Done);
    assert_eq!(p.total, 3, "png and node_modules are not candidates");
    assert_eq!(p.indexed, 3);
    assert_eq!(
        count(
            &e.conn,
            "SELECT COUNT(*) FROM documents WHERE parse_status='PARSED'"
        ),
        3
    );
    assert_eq!(
        count(&e.conn, "SELECT COUNT(*) FROM chunks"),
        count(
            &e.conn,
            "SELECT COUNT(*) FROM embedding_records WHERE dimension = 768"
        )
    );
    assert_eq!(fts_hits(&e.conn, "countdown"), 1);
    assert_eq!(fts_hits(&e.conn, "Toplantı"), 1, "Turkish text searchable");
    let line: (i64, i64) = e
        .conn
        .query_row(
            "SELECT line_start, line_end FROM chunks WHERE text LIKE '%countdown%'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(line, (1, 2));
    assert_eq!(
        store::get_scope(&e.conn, &s.scope_id)
            .unwrap()
            .unwrap()
            .file_count,
        3
    );

    // Rescan without changes: nothing is embedded again.
    let calls = emb.calls;
    let (_, p) = scan(&mut e, &s, &mut emb);
    assert_eq!(p.unchanged, 3);
    assert_eq!(emb.calls, calls);

    // Edit one file, delete another.
    std::fs::write(
        e.files.path().join("projects/notes.txt"),
        "Yeni not: sürüm pazartesi.\n",
    )
    .unwrap();
    std::fs::remove_file(e.files.path().join("projects/private/diary.md")).unwrap();
    let (_, p) = scan(&mut e, &s, &mut emb);
    assert_eq!(p.indexed, 1);
    assert_eq!(fts_hits(&e.conn, "pazartesi"), 1);
    assert_eq!(fts_hits(&e.conn, "cuma"), 0, "old text gone from FTS");
    assert_eq!(count(&e.conn, "SELECT COUNT(*) FROM documents"), 2);

    // Source files never written by the indexer (edits above were ours).
    let after = hashes(e.files.path());
    for (k, v) in &before {
        if k.ends_with("notes.txt") || k.ends_with("diary.md") {
            continue;
        }
        assert_eq!(after.get(k), Some(v), "{k:?} changed");
    }
}

#[test]
fn remove_and_purge_denies_first_and_leaves_nothing() {
    let mut e = env();
    let before = hashes(e.files.path());
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    let timer = walk::canonical(&e.files.path().join("projects/auction/timer.md")).unwrap();
    assert!(store::authorized(&e.conn, &timer).unwrap());

    let purge = store::begin_revoke(&mut e.conn, &s.scope_id, RevokeMode::RemoveAndPurge, false)
        .unwrap()
        .unwrap();
    // Denied before any purge work ran.
    assert!(!store::authorized(&e.conn, &timer).unwrap());
    assert_eq!(store::pending_purges(&e.conn).unwrap(), vec![purge.clone()]);

    store::run_purge(&mut e.conn, &purge).unwrap();
    for t in [
        "documents",
        "chunks",
        "embedding_records",
        "document_scopes",
    ] {
        assert_eq!(
            count(&e.conn, &format!("SELECT COUNT(*) FROM {t}")),
            0,
            "{t}"
        );
    }
    assert_eq!(fts_hits(&e.conn, "countdown"), 0);
    assert_eq!(
        store::get_scope(&e.conn, &s.scope_id)
            .unwrap()
            .unwrap()
            .state,
        ScopeState::Revoked
    );
    assert!(store::pending_purges(&e.conn).unwrap().is_empty());
    assert_eq!(hashes(e.files.path()), before, "source files untouched");
}

#[test]
fn child_revoke_under_active_parent_both_choices() {
    // Block: child content is erased and never collected again via the parent.
    let mut e = env();
    let parent = grant(&e, "projects");
    let child = grant(&e, "projects/private");
    scan(&mut e, &parent, &mut FakeEmbedder::new());
    scan(&mut e, &child, &mut FakeEmbedder::new());
    let all = store::visible_scopes(&e.conn).unwrap();
    assert_eq!(
        store::parent_of(&all, &child).map(|p| p.scope_id.clone()),
        Some(parent.scope_id.clone())
    );

    let p = store::begin_revoke(
        &mut e.conn,
        &child.scope_id,
        RevokeMode::BlockAndPurge,
        false,
    )
    .unwrap()
    .unwrap();
    store::run_purge(&mut e.conn, &p).unwrap();
    assert_eq!(fts_hits(&e.conn, "thoughts"), 0);
    assert_eq!(
        store::exclusion_count(&e.conn, &parent.scope_id).unwrap(),
        1
    );
    let diary = walk::canonical(&e.files.path().join("projects/private/diary.md")).unwrap();
    assert!(!store::authorized(&e.conn, &diary).unwrap());
    // Rescanning the parent does not re-collect it.
    scan(&mut e, &parent, &mut FakeEmbedder::new());
    assert_eq!(fts_hits(&e.conn, "thoughts"), 0);
    assert_eq!(fts_hits(&e.conn, "countdown"), 1, "rest of parent intact");

    // Grant only: stays searchable through the parent.
    let mut e = env();
    let parent = grant(&e, "projects");
    let child = grant(&e, "projects/private");
    scan(&mut e, &parent, &mut FakeEmbedder::new());
    scan(&mut e, &child, &mut FakeEmbedder::new());
    let p = store::begin_revoke(
        &mut e.conn,
        &child.scope_id,
        RevokeMode::RemoveGrantOnly,
        false,
    )
    .unwrap()
    .unwrap();
    store::run_purge(&mut e.conn, &p).unwrap();
    assert_eq!(fts_hits(&e.conn, "thoughts"), 1);
    let diary = walk::canonical(&e.files.path().join("projects/private/diary.md")).unwrap();
    assert!(store::authorized(&e.conn, &diary).unwrap());
}

#[test]
fn revoke_during_scan_stops_new_data() {
    let mut e = env();
    let s = grant(&e, "projects");
    // A second connection revokes while the first is scanning.
    let db_path: String = e
        .conn
        .query_row("PRAGMA database_list", [], |r| r.get(2))
        .unwrap();
    let id = s.scope_id.clone();
    let mut emb = FakeEmbedder::new();
    let mut fired = false;
    emb.hook = Some(Box::new(move || {
        if !fired {
            fired = true;
            let mut other = Connection::open(&db_path).unwrap();
            store::begin_revoke(&mut other, &id, RevokeMode::RemoveAndPurge, false).unwrap();
        }
    }));
    let (out, _) = scan(&mut e, &s, &mut emb);
    assert_eq!(out, Outcome::ScopeChanged);
    assert_eq!(count(&e.conn, "SELECT COUNT(*) FROM chunks"), 0);
    let p = store::pending_purges(&e.conn).unwrap();
    store::run_purge(&mut e.conn, &p[0]).unwrap();
    assert_eq!(count(&e.conn, "SELECT COUNT(*) FROM documents"), 0);
}

#[test]
fn interrupted_purge_resumes_and_verifies() {
    let mut e = env();
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    let p = store::begin_revoke(&mut e.conn, &s.scope_id, RevokeMode::RemoveAndPurge, true)
        .unwrap()
        .unwrap();
    // "Crash": purge never ran. Restart finds it and finishes it.
    let pending = store::pending_purges(&e.conn).unwrap();
    assert_eq!(pending, vec![p.clone()]);
    store::run_purge(&mut e.conn, &p).unwrap();
    store::run_purge(&mut e.conn, &p).unwrap(); // idempotent
    let verified: Option<String> = e
        .conn
        .query_row(
            "SELECT verified_at FROM purge_jobs WHERE purge_id = ?1",
            [&p],
            |r| r.get(0),
        )
        .unwrap();
    assert!(verified.is_some());
}

#[test]
fn stop_watching_keeps_the_index_and_cancel_is_clean() {
    let mut e = env();
    let s = grant(&e, "projects");
    let generation = e.generation.clone();
    let (out, p) = job::run(
        &mut e.conn,
        JobInput {
            scope_id: &s.scope_id,
            generation: &generation,
            deny: &[],
            limits: Limits::default(),
            batch: 8,
        },
        &mut FakeEmbedder::new(),
        &AtomicBool::new(true),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out, Outcome::Cancelled);
    assert_eq!(p.indexed, 0);

    scan(&mut e, &s, &mut FakeEmbedder::new());
    assert!(
        store::begin_revoke(&mut e.conn, &s.scope_id, RevokeMode::StopWatching, false)
            .unwrap()
            .is_none()
    );
    let timer = walk::canonical(&e.files.path().join("projects/auction/timer.md")).unwrap();
    assert!(
        store::authorized(&e.conn, &timer).unwrap(),
        "paused stays searchable"
    );
    assert_eq!(fts_hits(&e.conn, "countdown"), 1);
}

#[test]
fn new_embedding_identity_starts_a_new_generation() {
    let mut e = env();
    let g1 = e.generation.clone();
    assert_eq!(
        store::ensure_current_generation(&mut e.conn, &spec("h1")).unwrap(),
        g1
    );
    let g2 = store::ensure_current_generation(&mut e.conn, &spec("h2")).unwrap();
    assert_ne!(g1, g2);
    assert_eq!(store::current_generation(&e.conn).unwrap().unwrap().0, g2);
    // Switching back reactivates the old one instead of duplicating it.
    assert_eq!(
        store::ensure_current_generation(&mut e.conn, &spec("h1")).unwrap(),
        g1
    );
}
