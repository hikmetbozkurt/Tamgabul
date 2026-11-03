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
    images: usize,
    /// Called before each batch; lets a test revoke mid-scan.
    hook: Option<Box<dyn FnMut()>>,
}

impl FakeEmbedder {
    fn new() -> Self {
        Self {
            calls: 0,
            texts: 0,
            images: 0,
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
        Ok(texts.iter().map(|t| vec_for(t.as_bytes())).collect())
    }
    fn supports_images(&mut self) -> bool {
        true
    }
    fn embed_image(&mut self, bytes: &[u8]) -> Result<Vec<f32>, IpcError> {
        self.images += 1;
        Ok(vec_for(bytes))
    }
}

fn vec_for(b: &[u8]) -> Vec<f32> {
    let h = blake3::hash(b);
    (0..768)
        .map(|i| h.as_bytes()[i % 32] as f32 / 255.0 + 0.01)
        .collect()
}

/// Minimal one-page PDF with a text layer.
fn tiny_pdf(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 24 Tf 72 720 Td ({text}) Tj ET");
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
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
    std::fs::write(
        p.join("projects/report.pdf"),
        tiny_pdf("Quarterly budget review"),
    )
    .unwrap();
    std::fs::write(p.join("projects/scan.pdf"), tiny_pdf("")).unwrap();
    std::fs::write(p.join("projects/table.docx"), "not indexed").unwrap();
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
    assert_eq!(p.total, 6, "docx and node_modules are not candidates");
    assert_eq!(p.indexed, 5);
    assert_eq!(p.skipped, 1, "PDF without a text layer");
    assert_eq!(emb.images, 1);
    let q = |sql: &str| count(&e.conn, sql);
    assert_eq!(
        q("SELECT COUNT(*) FROM documents WHERE parse_status='PARSED'"),
        5
    );
    assert_eq!(
        q("SELECT COUNT(*) FROM documents WHERE parse_error='pdf_no_text'"),
        1
    );
    assert_eq!(
        q("SELECT COUNT(*) FROM chunks WHERE modality='image' AND text IS NULL"),
        1
    );
    assert_eq!(
        q("SELECT COUNT(*) FROM chunks WHERE page = 1 AND text LIKE '%Quarterly%'"),
        1
    );
    assert_eq!(fts_hits(&e.conn, "budget"), 1);
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
        5
    );

    // Rescan without changes: nothing is embedded again.
    let (calls, images) = (emb.calls, emb.images);
    let (_, p) = scan(&mut e, &s, &mut emb);
    assert_eq!(p.unchanged, 6, "permanent skips are not retried");
    assert_eq!((emb.calls, emb.images), (calls, images));

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
    assert_eq!(
        count(
            &e.conn,
            "SELECT COUNT(*) FROM documents WHERE parse_status='PARSED'"
        ),
        4
    );

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

// ---------------------------------------------------------------- P2/P3

use tamgabul_core::dto::SearchKind;
use tamgabul_core::history::{self, ShownResult};
use tamgabul_core::search::{self, Acl, Reason};

fn chunk_vector(c: &Connection, like: &str) -> Vec<f32> {
    let b: Vec<u8> = c
        .query_row(
            "SELECT e.vector FROM embedding_records e JOIN chunks c ON c.chunk_id = e.chunk_id WHERE c.text LIKE ?1",
            [like],
            |r| r.get(0),
        )
        .unwrap();
    store::vector_from_bytes(&b)
}

fn title_of(c: &Connection, doc: &str) -> String {
    let p: String = c
        .query_row(
            "SELECT canonical_path FROM documents WHERE document_id = ?1",
            [doc],
            |r| r.get(0),
        )
        .unwrap();
    Path::new(&p)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn hybrid_search_finds_by_meaning_words_and_name_and_respects_revoke() {
    let mut e = env();
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    let g = e.generation.clone();

    // Meaning: a query vector equal to the timer chunk's vector ranks it first.
    let q = chunk_vector(&e.conn, "%countdown%");
    let hits = search::search(&e.conn, Some(&g), Some(&q), "zzz", 10).unwrap();
    assert_eq!(title_of(&e.conn, &hits[0].document_id), "timer.md");
    assert_eq!(hits[0].reason, Reason::Semantic);

    // Words (PDF text) and file names (the image) without any vector.
    let hits = search::search(&e.conn, Some(&g), None, "budget review", 10).unwrap();
    assert_eq!(title_of(&e.conn, &hits[0].document_id), "report.pdf");
    assert_eq!(hits[0].reason, Reason::Keyword);
    let hits = search::search(&e.conn, Some(&g), None, "picture", 10).unwrap();
    assert_eq!(title_of(&e.conn, &hits[0].document_id), "picture.png");
    assert_eq!(hits[0].reason, Reason::Filename);

    // Card + history round trip.
    let acl = Acl::load(&e.conn).unwrap();
    let terms = search::terms("budget review");
    let hits = search::search(&e.conn, Some(&g), None, "budget review", 10).unwrap();
    let card = search::resolve(
        &e.conn,
        &acl,
        &hits[0].document_id,
        hits[0].chunk_id,
        "keyword",
        &terms,
    )
    .unwrap()
    .unwrap();
    assert_eq!(card.card.title, "report.pdf");
    assert!(card.card.snippet.as_deref().unwrap().contains("budget"));
    assert_eq!(card.card.highlights, ["budget", "review"]);
    let session = history::ensure_session(&e.conn, None, true).unwrap();
    let shown: Vec<ShownResult> = hits
        .iter()
        .map(|h| ShownResult {
            hit: h,
            scope_id: Some(s.scope_id.clone()),
            title: title_of(&e.conn, &h.document_id),
        })
        .collect();
    history::record_search(
        &mut e.conn,
        &session,
        "budget review",
        SearchKind::Results,
        &shown,
        Some(&g),
    )
    .unwrap();
    history::mark_opened(&e.conn, &hits[0].document_id).unwrap();
    let turns = history::session_turns(&e.conn, &session).unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].text.as_deref(), Some("budget review"));
    assert_eq!(turns[1].results[0].title, "report.pdf");
    assert!(turns[1].opened);
    let list = history::list_sessions(&e.conn).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].opened_count, 1);

    // Revoke: gone from search at once, and from the old conversation too.
    store::begin_revoke(&mut e.conn, &s.scope_id, RevokeMode::RemoveAndPurge, false).unwrap();
    assert!(search::search(&e.conn, Some(&g), Some(&q), "budget", 10)
        .unwrap()
        .is_empty());
    assert!(history::session_turns(&e.conn, &session).unwrap()[1]
        .results
        .is_empty());

    history::clear_all(&e.conn).unwrap();
    assert!(history::list_sessions(&e.conn).unwrap().is_empty());
}
