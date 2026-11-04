//! P1 acceptance tests with temp folders and a test-only fake embedder:
//! scan, delta rescan, overlap + revoke choices, revoke during scan, purge
//! resume, and source files untouched.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rusqlite::Connection;
use tamgabul_core::db::open_and_migrate;
use tamgabul_core::describe::{Describer, NoDescriber};
use tamgabul_core::dto::RevokeMode;
use tamgabul_core::error::IpcError;
use tamgabul_core::index::job::{self, Embedder, JobInput, Outcome};
use tamgabul_core::index::media::{NoPlatform, Platform};
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
    fn supports_vision(&mut self) -> bool {
        true
    }
    fn supports_audio(&mut self) -> bool {
        true
    }
    fn embed_media(&mut self, bytes: &[u8]) -> Result<Vec<f32>, IpcError> {
        self.images += 1;
        Ok(vec_for(bytes))
    }
}

/// Test-only OS services: decodes "HEIC" to a PNG, renders PDF pages,
/// and "reads" fixed text from any image.
struct FakeOs;
impl Platform for FakeOs {
    fn decode_to_png(&self, _: &[u8], _: u32) -> Option<Vec<u8>> {
        Some(png_bytes(24, 16))
    }
    fn ocr(&self, _: &[u8]) -> Option<Result<String, String>> {
        Some(Ok("Fatura numarası 4521 ödeme bekliyor".into()))
    }
    fn ocr_available(&self) -> bool {
        true
    }
    fn render_pdf_pages(&self, _: &[u8], pages: &[u32], _: u32) -> Option<Vec<(u32, Vec<u8>)>> {
        Some(
            pages
                .iter()
                .filter(|p| **p == 0)
                .map(|p| (*p, png_bytes(30, 40)))
                .collect(),
        )
    }
}

fn png_bytes(w: u32, h: u32) -> Vec<u8> {
    let img =
        image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 9) as u8, (y * 7) as u8, 200]));
    let mut c = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut c, image::ImageFormat::Png)
        .unwrap();
    c.into_inner()
}

fn wav_tone(secs: u32) -> Vec<u8> {
    let rate = 16_000u32;
    let pcm: Vec<u8> = (0..rate * secs)
        .flat_map(|i| (((i as f32 * 0.07).sin() * 9000.0) as i16).to_le_bytes())
        .collect();
    let mut w = b"RIFF".to_vec();
    w.extend((36 + pcm.len() as u32).to_le_bytes());
    w.extend(b"WAVEfmt ");
    w.extend(16u32.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(1u16.to_le_bytes());
    w.extend(rate.to_le_bytes());
    w.extend((rate * 2).to_le_bytes());
    w.extend(2u16.to_le_bytes());
    w.extend(16u16.to_le_bytes());
    w.extend(b"data");
    w.extend((pcm.len() as u32).to_le_bytes());
    w.extend(pcm);
    w
}

fn docx(text: &str) -> Vec<u8> {
    let mut c = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut c);
        z.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(
            &mut z,
            format!(
                "<w:document><w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body></w:document>"
            )
            .as_bytes(),
        )
        .unwrap();
        z.finish().unwrap();
    }
    c.into_inner()
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
    std::fs::write(p.join("projects/picture.png"), png_bytes(20, 20)).unwrap();
    std::fs::write(
        p.join("projects/photo.heic"),
        b"\x00\x00\x00\x18ftypheic fake",
    )
    .unwrap();
    std::fs::write(p.join("projects/voice.wav"), wav_tone(35)).unwrap();
    std::fs::write(
        p.join("projects/legacy.doc"),
        b"\xd0\xcf\x11\xe0 old binary word",
    )
    .unwrap();
    std::fs::write(
        p.join("projects/report.pdf"),
        tiny_pdf("Quarterly budget review for the infrastructure team and finance"),
    )
    .unwrap();
    std::fs::write(p.join("projects/scan.pdf"), tiny_pdf("")).unwrap();
    std::fs::write(
        p.join("projects/contract.docx"),
        docx("Sözleşme taslağı imza bekliyor"),
    )
    .unwrap();
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
    scan_with(e, scope, emb, &mut NoDescriber)
}

fn scan_with(
    e: &mut Env,
    scope: &Scope,
    emb: &mut FakeEmbedder,
    describer: &mut dyn Describer,
) -> (Outcome, job::Progress) {
    let generation = e.generation.clone();
    job::run(
        &mut e.conn,
        JobInput {
            scope_id: &scope.scope_id,
            generation: &generation,
            deny: &[],
            limits: Limits::default(),
            batch: 8,
            platform: &FakeOs,
        },
        emb,
        describer,
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
fn scan_indexes_every_kind_with_vectors_and_rescans_only_changes() {
    let mut e = env();
    let before = hashes(e.files.path());
    let s = grant(&e, "projects");
    let mut emb = FakeEmbedder::new();
    let (out, p) = scan(&mut e, &s, &mut emb);
    assert_eq!(out, Outcome::Done);
    // md, txt, md, png, heic, wav, pdf, pdf, docx, doc — not node_modules.
    // The fake .doc is not a real Word 97 file: skipped as corrupt.
    assert_eq!(p.total, 10);
    assert_eq!((p.indexed, p.skipped, p.errors), (9, 1, 0));
    assert_eq!(
        count(
            &e.conn,
            "SELECT COUNT(*) FROM documents WHERE parse_status='PARSED'"
        ),
        9
    );
    // Media: two images (PNG + OS-decoded HEIC) and two audio segments (30 s + 5 s).
    assert_eq!(emb.images, 4);
    assert_eq!(
        count(
            &e.conn,
            "SELECT COUNT(*) FROM chunks WHERE modality='image' AND text IS NULL"
        ),
        2
    );
    assert_eq!(
        count(&e.conn, "SELECT COUNT(*) FROM chunks WHERE modality='audio' AND time_start_ms IN (0, 30000) AND time_end_ms IN (30000, 35000)"),
        2
    );
    // OCR: both images and the scanned PDF page.
    assert_eq!(
        count(&e.conn, "SELECT COUNT(*) FROM chunks WHERE modality='ocr'"),
        3
    );
    assert_eq!(
        count(
            &e.conn,
            "SELECT COUNT(*) FROM chunks WHERE modality='ocr' AND page = 1"
        ),
        1
    );
    assert_eq!(fts_hits(&e.conn, "fatura"), 3);
    // Text layer of a normal PDF, a DOCX, and accent/case-insensitive words.
    assert_eq!(
        count(
            &e.conn,
            "SELECT COUNT(*) FROM chunks WHERE page = 1 AND text LIKE '%Quarterly%'"
        ),
        1
    );
    assert_eq!(fts_hits(&e.conn, "budget"), 1);
    assert_eq!(
        fts_hits(&e.conn, "sozlesme"),
        1,
        "docx text, typed without accents"
    );
    assert_eq!(
        fts_hits(&e.conn, "toplanti"),
        1,
        "Toplantı found as toplanti"
    );
    assert_eq!(fts_hits(&e.conn, "countdown"), 1);
    assert_eq!(
        count(&e.conn, "SELECT COUNT(*) FROM chunks"),
        count(
            &e.conn,
            "SELECT COUNT(*) FROM embedding_records WHERE dimension = 768"
        )
    );
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
        9
    );

    // Rescan without changes: nothing is read or embedded again.
    let (calls, media) = (emb.calls, emb.images);
    let (_, p) = scan(&mut e, &s, &mut emb);
    assert_eq!(p.unchanged, 9);
    assert_eq!((emb.calls, emb.images), (calls, media));

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
        8
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
fn without_os_services_scans_and_heic_are_skipped_with_reasons() {
    let mut e = env();
    let s = grant(&e, "projects");
    let generation = e.generation.clone();
    let (_, p) = job::run(
        &mut e.conn,
        JobInput {
            scope_id: &s.scope_id,
            generation: &generation,
            deny: &[],
            limits: Limits::default(),
            batch: 8,
            platform: &NoPlatform,
        },
        &mut FakeEmbedder::new(),
        &mut NoDescriber,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!((p.indexed, p.skipped), (7, 3));
    let reasons: Vec<String> = {
        let mut st = e
            .conn
            .prepare("SELECT parse_error FROM documents WHERE parse_status='SKIPPED' ORDER BY parse_error")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    assert_eq!(
        reasons,
        ["corrupt", "pdf_no_text_no_ocr", "unsupported_codec"]
    );
    assert_eq!(
        count(&e.conn, "SELECT COUNT(*) FROM chunks WHERE modality='ocr'"),
        0
    );
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
            platform: &NoPlatform,
        },
        &mut FakeEmbedder::new(),
        &mut NoDescriber,
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
use tamgabul_core::history::{self, Answer, ShownResult};
use tamgabul_core::search::{self, Acl, DocFilter, Reason};
use tamgabul_core::understand::{self, DateFilter, FileTypeFilter};

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

    // An image vector ranks images first, in their own modality band, even
    // though text↔text similarities would be higher.
    let img: Vec<u8> = e
        .conn
        .query_row("SELECT e.vector FROM embedding_records e JOIN chunks c ON c.chunk_id = e.chunk_id WHERE c.modality = 'image' LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let hits = search::search(
        &e.conn,
        Some(&g),
        Some(&store::vector_from_bytes(&img)),
        "zzz",
        10,
    )
    .unwrap();
    assert_eq!(hits[0].reason, Reason::Image);
    // Words read by OCR find screenshots and the scanned PDF.
    let hits = search::search(&e.conn, Some(&g), None, "fatura numarasi", 10).unwrap();
    let names: Vec<String> = hits
        .iter()
        .map(|h| title_of(&e.conn, &h.document_id))
        .collect();
    for n in ["picture.png", "photo.heic", "scan.pdf"] {
        assert!(names.contains(&n.to_string()), "{names:?}");
    }
    assert!(hits.iter().all(|h| h.reason == Reason::Ocr));

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
    let plan = understand::plan("budget review", None, true);
    let direct = tamgabul_core::dto::DirectAnswer {
        text: "Quarterly budget review.".into(),
        result_ref: search::result_ref(&hits[0].document_id, hits[0].chunk_id),
    };
    history::record_search(
        &mut e.conn,
        &session,
        "budget review",
        &Answer {
            kind: SearchKind::Results,
            results: &shown,
            plan: &plan,
            choices: &[],
            direct: Some(&direct),
            summary: None,
            picked_by_llm: true,
            notice: None,
        },
        Some(&g),
    )
    .unwrap();
    history::mark_opened(&e.conn, &hits[0].document_id).unwrap();
    let turns = history::session_turns(&e.conn, &session).unwrap();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].text.as_deref(), Some("budget review"));
    assert_eq!(turns[1].results[0].title, "report.pdf");
    assert!(turns[1].opened);
    assert!(turns[1].picked_by_llm);
    assert_eq!(turns[1].answer.as_ref(), Some(&direct));
    let list = history::list_sessions(&e.conn).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].opened_count, 1);

    // Revoke: gone from search at once, and from the old conversation too.
    store::begin_revoke(&mut e.conn, &s.scope_id, RevokeMode::RemoveAndPurge, false).unwrap();
    assert!(search::search(&e.conn, Some(&g), Some(&q), "budget", 10)
        .unwrap()
        .is_empty());
    let after = &history::session_turns(&e.conn, &session).unwrap()[1];
    assert!(after.results.is_empty());
    assert!(
        after.answer.is_none(),
        "answer hidden with its revoked source"
    );

    history::clear_all(&e.conn).unwrap();
    assert!(history::list_sessions(&e.conn).unwrap().is_empty());
}

fn names(e: &Env, hits: &[search::Hit]) -> Vec<String> {
    let mut v: Vec<String> = hits
        .iter()
        .map(|h| title_of(&e.conn, &h.document_id))
        .collect();
    v.sort();
    v
}

fn set_mtime(c: &Connection, name: &str, secs: i64) {
    c.execute(
        "UPDATE documents SET mtime = ?1 WHERE canonical_path LIKE ?2",
        rusqlite::params![format!("{secs}.0"), format!("%{name}")],
    )
    .unwrap();
}

#[test]
fn filters_narrow_search_and_browse_lists_by_type_and_date() {
    let mut e = env();
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    let g = e.generation.clone();
    let now = 1_793_700_000; // 2026-11-03 (UTC)
    let day = 86_400;
    set_mtime(&e.conn, "picture.png", now - 3600);
    set_mtime(&e.conn, "photo.heic", now - 40 * day);
    set_mtime(&e.conn, "scan.pdf", now - day);
    set_mtime(&e.conn, "report.pdf", now - day);

    // OCR words match two images and a PDF; a type filter keeps one kind.
    let words = search::terms("fatura numarasi");
    let images = understand::Filters {
        file_type: FileTypeFilter::Image,
        ..Default::default()
    };
    let hits = search::search_terms(
        &e.conn,
        Some(&g),
        None,
        &words,
        &DocFilter::new(&images, now, 0),
        10,
    )
    .unwrap();
    assert_eq!(names(&e, &hits), ["photo.heic", "picture.png"]);

    // …and a date window drops the old photo.
    let today_images = understand::Filters {
        file_type: FileTypeFilter::Image,
        date: DateFilter::Today,
        exclude: vec![],
    };
    let hits = search::search_terms(
        &e.conn,
        Some(&g),
        None,
        &words,
        &DocFilter::new(&today_images, now, 0),
        10,
    )
    .unwrap();
    assert_eq!(names(&e, &hits), ["picture.png"]);

    // Excluded words drop documents by name or matched text.
    let no_scan = understand::Filters {
        exclude: vec!["SCAN".into()],
        ..Default::default()
    };
    let hits = search::search_terms(
        &e.conn,
        Some(&g),
        None,
        &words,
        &DocFilter::new(&no_scan, now, 0),
        10,
    )
    .unwrap();
    assert!(!names(&e, &hits).contains(&"scan.pdf".to_string()));

    // No words, only filters: newest first.
    let p = understand::plan("dünkü pdf'ler", None, true);
    assert!(p.is_browse());
    let hits = search::browse(&e.conn, &DocFilter::new(&p.filters, now, 0), 10).unwrap();
    assert_eq!(names(&e, &hits), ["report.pdf", "scan.pdf"]);
    assert!(hits.iter().all(|h| h.reason == Reason::Filter));

    // Revoked folders never list.
    store::begin_revoke(&mut e.conn, &s.scope_id, RevokeMode::RemoveAndPurge, false).unwrap();
    assert!(search::browse(&e.conn, &DocFilter::default(), 10)
        .unwrap()
        .is_empty());
}

#[test]
fn clarify_turns_and_context_are_kept_in_history() {
    let mut e = env();
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    let g = e.generation.clone();
    let session = history::ensure_session(&e.conn, None, true).unwrap();

    // 1. A search with a PDF filter.
    let p1 = understand::plan("bütçe pdf", None, true);
    let hits = search::search(&e.conn, Some(&g), None, "budget", 10).unwrap();
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
        "bütçe pdf",
        &Answer {
            kind: SearchKind::Results,
            results: &shown,
            plan: &p1,
            choices: &[],
            direct: None,
            summary: None,
            picked_by_llm: false,
            notice: None,
        },
        Some(&g),
    )
    .unwrap();

    // 2. A clarifying question from the LLM.
    let l = understand::parse_llm(&serde_json::json!({
        "search_text": "", "keywords": [], "exclude": [], "file_type": "any", "date": "any",
        "intent": "clarify", "clarify_question": "Hangi proje?", "clarify_options": ["20coin", "satranç"]
    }))
    .unwrap();
    let p2 = understand::plan("şunu bul", Some(&l), true);
    let choices: Vec<tamgabul_core::dto::Choice> = p2
        .options
        .iter()
        .enumerate()
        .map(|(i, o)| tamgabul_core::dto::Choice {
            id: format!("opt:{i}"),
            label: o.clone(),
            count: 0,
        })
        .collect();
    history::record_search(
        &mut e.conn,
        &session,
        "şunu bul",
        &Answer {
            kind: SearchKind::Clarify,
            results: &[],
            plan: &p2,
            choices: &choices,
            direct: None,
            summary: None,
            picked_by_llm: false,
            notice: None,
        },
        Some(&g),
    )
    .unwrap();

    let ctx = history::context(&e.conn, Some(&session)).unwrap();
    assert_eq!(ctx.prior.len(), 2);
    assert_eq!(ctx.prior[0].user, "bütçe pdf");
    assert_eq!(ctx.prior[0].shown, ["report.pdf"]);
    assert_eq!(
        ctx.active.file_type,
        FileTypeFilter::Pdf,
        "kept from the search, not the question"
    );
    assert!(ctx.last_was_clarify);
    assert_eq!(ctx.last_user.as_deref(), Some("şunu bul"));

    let turns = history::session_turns(&e.conn, &session).unwrap();
    assert_eq!(
        turns[1].understood.as_ref().unwrap().file_type,
        FileTypeFilter::Pdf
    );
    assert_eq!(turns[3].kind, Some(SearchKind::Clarify));
    assert_eq!(turns[3].question.as_deref(), Some("Hangi proje?"));
    assert_eq!(turns[3].choices[1].label, "satranç");
    let intent: String = e
        .conn
        .query_row(
            "SELECT intent FROM turns WHERE session_id = ?1 AND ordinal = 2",
            [&session],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(intent, "clarify");
    assert!(history::context(&e.conn, None).unwrap().prior.is_empty());
}

/// Test-only stand-in for the vision LLM: one fixed description per image.
struct FakeDescriber {
    calls: usize,
    fail: bool,
}

impl Describer for FakeDescriber {
    fn available(&mut self) -> bool {
        true
    }
    fn describe_image(&mut self, _png: &[u8]) -> Result<Option<String>, IpcError> {
        self.calls += 1;
        if self.fail {
            return Err(IpcError::new(
                tamgabul_core::error::ErrorCode::RuntimeLoadFailed,
                "x",
            ));
        }
        Ok(Some(
            "UK visa page inside a passport. Keywords: passport, pasaport, vize".into(),
        ))
    }
}

#[test]
fn images_get_descriptions_that_words_can_find() {
    let mut e = env();
    let s = grant(&e, "projects");
    // First scan without a vision model: no captions.
    scan(&mut e, &s, &mut FakeEmbedder::new());
    assert_eq!(
        count(
            &e.conn,
            "SELECT COUNT(*) FROM chunks WHERE modality = 'caption'"
        ),
        0
    );
    // Adding the model later: unchanged images are described on the next
    // scan, other files are not touched.
    let mut d = FakeDescriber {
        calls: 0,
        fail: false,
    };
    let (_, p) = scan_with(&mut e, &s, &mut FakeEmbedder::new(), &mut d);
    let images = count(
        &e.conn,
        "SELECT COUNT(*) FROM documents WHERE file_type = 'image' AND parse_status = 'PARSED'",
    );
    assert!(images >= 2);
    assert_eq!(d.calls as i64, images);
    assert_eq!(p.indexed as i64, images);
    let g = e.generation.clone();
    let hits = search::search(&e.conn, Some(&g), None, "pasaport vize", 10).unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.reason == Reason::Caption));
    // The card shows the description.
    let acl = Acl::load(&e.conn).unwrap();
    let r = search::resolve(
        &e.conn,
        &acl,
        &hits[0].document_id,
        hits[0].chunk_id,
        "caption",
        &[],
    )
    .unwrap()
    .unwrap();
    assert!(r.card.snippet.unwrap().starts_with("UK visa page"));
    // Already described: a third scan does no model work.
    let (_, p) = scan_with(&mut e, &s, &mut FakeEmbedder::new(), &mut d);
    assert_eq!(p.indexed, 0);
    assert_eq!(d.calls as i64, images);
}

#[test]
fn a_failing_vision_model_does_not_stop_the_scan() {
    let mut e = env();
    let s = grant(&e, "projects");
    let mut d = FakeDescriber {
        calls: 0,
        fail: true,
    };
    let (out, p) = scan_with(&mut e, &s, &mut FakeEmbedder::new(), &mut d);
    assert_eq!(out, Outcome::Done);
    assert_eq!(d.calls, 1, "tried once, then the scan went on without it");
    assert!(p.indexed > 5);
}

#[test]
fn retention_removes_only_old_conversations() {
    use tamgabul_core::settings::HistoryRetention;
    let e = env();
    let old = history::ensure_session(&e.conn, None, true).unwrap();
    let fresh = history::ensure_session(&e.conn, None, true).unwrap();
    e.conn
        .execute(
            "UPDATE sessions SET last_active_at = strftime('%Y-%m-%dT%H:%M:%fZ','now','-45 days') WHERE session_id = ?1",
            [&old],
        )
        .unwrap();
    assert_eq!(
        history::apply_retention(&e.conn, HistoryRetention::Forever).unwrap(),
        0
    );
    assert_eq!(
        history::apply_retention(&e.conn, HistoryRetention::Days90).unwrap(),
        0
    );
    assert_eq!(
        history::apply_retention(&e.conn, HistoryRetention::Days30).unwrap(),
        1
    );
    let left: Vec<String> = e
        .conn
        .prepare("SELECT session_id FROM sessions")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(left, [fresh]);
}

// ---------------------------------------------------------------- engine

use tamgabul_core::engine::{self, Query, QueryModels};

/// Test-only models: no embedding; the LLM answers from a script keyed by
/// which prompt it got (plan vs pick).
struct FakeModels {
    llm: bool,
    pick: serde_json::Value,
}

impl QueryModels for FakeModels {
    fn embed_query(&self, _text: &str) -> Option<Vec<f32>> {
        None
    }
    fn has_llm(&self) -> bool {
        self.llm
    }
    fn llm_json(
        &self,
        system: &str,
        _user: &str,
        _schema: &serde_json::Value,
        _max: u32,
    ) -> Option<serde_json::Value> {
        if system.contains("search plan") {
            Some(serde_json::json!({
                "search_text": "", "keywords": [], "exclude": [], "file_type": "any",
                "date": "any", "intent": "search", "clarify_question": "", "clarify_options": []
            }))
        } else {
            Some(self.pick.clone())
        }
    }
}

fn ask(e: &Env, models: &dyn QueryModels, text: &str, session: Option<&str>) -> engine::Outcome {
    let ctx = history::context(&e.conn, session).unwrap();
    engine::run(
        &e.conn,
        Some(&e.generation),
        models,
        &Query {
            text,
            ctx: &ctx,
            allow_clarify: true,
            now: 1_793_700_000,
            utc_offset: 0,
        },
    )
    .unwrap()
}

fn record(e: &mut Env, session: &str, text: &str, out: &engine::Outcome, scope: &str) {
    let shown: Vec<ShownResult> = out
        .found
        .iter()
        .map(|f| ShownResult {
            hit: &f.hit,
            scope_id: Some(scope.to_string()),
            title: f.resolved.card.title.clone(),
        })
        .collect();
    history::record_search(
        &mut e.conn,
        session,
        text,
        &Answer {
            kind: out.kind.clone(),
            results: &shown,
            plan: &out.plan,
            choices: &out.choices,
            direct: out.direct.as_ref(),
            summary: out.summary.as_ref(),
            picked_by_llm: out.picked_by_llm,
            notice: out.notice.as_deref(),
        },
        None,
    )
    .unwrap();
}

#[test]
fn earlier_results_are_found_from_history_and_hidden_after_revoke() {
    let mut e = env();
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    let none = FakeModels {
        llm: false,
        pick: serde_json::Value::Null,
    };
    let session = history::ensure_session(&e.conn, None, true).unwrap();
    let first = ask(&e, &none, "budget review", Some(&session));
    assert_eq!(first.found[0].resolved.card.title, "report.pdf");
    record(&mut e, &session, "budget review", &first, &s.scope_id);

    // A new conversation: "you found me the budget file".
    let again = ask(&e, &none, "geçen bana budget dosyasını bulmuştun", None);
    assert_eq!(
        again.plan.intent,
        tamgabul_core::understand::PlanIntent::LookupPrior
    );
    assert_eq!(again.found[0].resolved.card.title, "report.pdf");
    assert_eq!(again.found[0].hit.reason, Reason::Previous);
    assert_eq!(again.found[0].resolved.card.match_reason, "previous_result");

    // After revoke: nothing revealed, the user is told why.
    store::begin_revoke(&mut e.conn, &s.scope_id, RevokeMode::RemoveAndPurge, false).unwrap();
    while !store::pending_purges(&e.conn).unwrap().is_empty() {
        for p in store::pending_purges(&e.conn).unwrap() {
            store::run_purge(&mut e.conn, &p).unwrap();
        }
    }
    let gone = ask(&e, &none, "geçen bana budget dosyasını bulmuştun", None);
    assert!(gone.found.is_empty());
    assert_eq!(gone.notice.as_deref(), Some("prior_not_accessible"));
}

#[test]
fn llm_pick_keeps_matches_and_cites_them_in_a_summary() {
    let mut e = env();
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    // "fatura numarasi" matches several OCR'd files; the LLM keeps #2.
    let pick = serde_json::json!({
        "relevant": ["2"], "answer": "", "answer_from": "none",
        "summary": "Fatura numarası görselde yazıyor [2]."
    });
    let m = FakeModels { llm: true, pick };
    let out = ask(&e, &m, "fatura numarasi", None);
    assert!(out.picked_by_llm);
    assert_eq!(out.found.len(), 1);
    let sum = out.summary.as_ref().unwrap();
    assert_eq!(sum.text, "Fatura numarası görselde yazıyor [1].");
    assert_eq!(sum.sources, [out.found[0].resolved.card.result_ref.clone()]);

    // Stored and rebuilt from history.
    let session = history::ensure_session(&e.conn, None, true).unwrap();
    record(&mut e, &session, "fatura numarasi", &out, &s.scope_id);
    let turns = history::session_turns(&e.conn, &session).unwrap();
    assert_eq!(turns[1].summary.as_ref(), Some(sum));

    // A summary citing nothing is dropped; the pick still applies.
    let m = FakeModels {
        llm: true,
        pick: serde_json::json!({"relevant": ["1"], "answer": "", "answer_from": "none", "summary": "Uncited."}),
    };
    let out = ask(&e, &m, "fatura numarasi", None);
    assert!(out.summary.is_none());
    assert_eq!(out.found.len(), 1);
}

#[test]
fn without_llm_weak_tail_hits_are_cut() {
    let mut e = env();
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    let none = FakeModels {
        llm: false,
        pick: serde_json::Value::Null,
    };
    let out = ask(&e, &none, "budget review", None);
    let best = out.found[0].hit.score;
    assert!(out
        .found
        .iter()
        .all(|f| f.hit.score >= best * engine::RELATIVE_FLOOR));
    assert!(!out.picked_by_llm);
}

// ---------------------------------------------------------------- legacy, video, relink, watching

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn legacy_office_files_and_video_audio_tracks_are_indexed() {
    let mut e = env();
    let dir = e.files.path().join("old");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kira.doc"), fixture("legacy.doc")).unwrap();
    std::fs::write(dir.join("stok.xls"), fixture("stock.xls")).unwrap();
    std::fs::write(dir.join("clip.mp4"), fixture("clip.mp4")).unwrap();
    std::fs::write(dir.join("broken.doc"), b"not an office file").unwrap();
    let before = hashes(&dir);
    let s = grant(&e, "old");
    let mut emb = FakeEmbedder::new();
    let (_, p) = scan(&mut e, &s, &mut emb);
    assert_eq!(p.indexed, 3, "{p:?}");
    assert_eq!(p.skipped, 1);
    let g = e.generation.clone();
    let hits = search::search(&e.conn, Some(&g), None, "artışı", 10).unwrap();
    assert_eq!(title_of(&e.conn, &hits[0].document_id), "kira.doc");
    let hits = search::search(&e.conn, Some(&g), None, "Somun", 10).unwrap();
    assert_eq!(title_of(&e.conn, &hits[0].document_id), "stok.xls");
    // The video's audio track became audio segments (no OS frame here).
    let audio: i64 = count(
        &e.conn,
        "SELECT COUNT(*) FROM chunks c JOIN documents d ON d.document_id = c.document_id
         WHERE d.file_type = 'video' AND c.modality = 'audio'",
    );
    assert!(audio >= 1);
    assert_eq!(
        search::file_kind("video"),
        tamgabul_core::dto::FileKind::Video
    );
    assert_eq!(
        search::file_kind("doc"),
        tamgabul_core::dto::FileKind::Document
    );
    // A rescan does not decode the unchanged video again.
    let images_before = emb.images;
    let (_, p) = scan(&mut e, &s, &mut emb);
    assert_eq!(p.indexed, 0);
    assert_eq!(emb.images, images_before);
    assert_eq!(before, hashes(&dir), "sources untouched");
}

#[test]
fn a_moved_folder_is_relinked_without_reindexing() {
    let mut e = env();
    let s = grant(&e, "projects");
    let mut emb = FakeEmbedder::new();
    scan(&mut e, &s, &mut emb);
    let calls = emb.calls;
    let old = e.files.path().join("projects");
    let new = e.files.path().join("moved-projects");
    std::fs::rename(&old, &new).unwrap();
    let new = walk::canonical(&new).unwrap();

    // Not the same folder: refused.
    let other = walk::canonical(&e.files.path().join("node_modules")).unwrap();
    assert!(!store::relink_check(&e.conn, &s, &other).unwrap().accepted());
    // The moved folder: accepted, index kept.
    let check = store::relink_check(&e.conn, &s, &new).unwrap();
    assert!(check.accepted(), "{check:?}");
    let moved = store::relink_scope(&mut e.conn, &s.scope_id, &new, "moved").unwrap();
    assert!(moved >= 5);
    let s2 = store::get_scope(&e.conn, &s.scope_id).unwrap().unwrap();
    assert_eq!(s2.canonical_path, new);

    // Rescan: nothing is embedded again.
    let (_, p) = scan(&mut e, &s2, &mut emb);
    assert_eq!(p.indexed, 0, "{p:?}");
    assert_eq!(emb.calls, calls);
    let g = e.generation.clone();
    let hits = search::search(&e.conn, Some(&g), None, "budget review", 10).unwrap();
    let path: String = e
        .conn
        .query_row(
            "SELECT canonical_path FROM documents WHERE document_id = ?1",
            [&hits[0].document_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(Path::new(&path).starts_with(&new), "{path}");
}

#[test]
fn watching_is_stored_per_folder() {
    let e = env();
    let s = grant(&e, "projects");
    assert!(!store::is_watching(&e.conn, &s.scope_id).unwrap());
    store::set_watching(&e.conn, &s.scope_id, true).unwrap();
    assert!(store::is_watching(&e.conn, &s.scope_id).unwrap());
    assert_eq!(
        store::watched_scopes(&e.conn).unwrap()[0].scope_id,
        s.scope_id
    );
    store::set_watching(&e.conn, &s.scope_id, false).unwrap();
    assert!(store::watched_scopes(&e.conn).unwrap().is_empty());
    assert!(store::set_watching(&e.conn, "s-nope", true).is_err());
}

#[test]
fn images_without_text_get_one_sideways_ocr_retry() {
    let mut e = env();
    let s = grant(&e, "projects");
    // First scan without OS services: images have no OCR text.
    let generation = e.generation.clone();
    job::run(
        &mut e.conn,
        JobInput {
            scope_id: &s.scope_id,
            generation: &generation,
            deny: &[],
            limits: Limits::default(),
            batch: 8,
            platform: &NoPlatform,
        },
        &mut FakeEmbedder::new(),
        &mut NoDescriber,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    // With OCR available, those images are read once more…
    let (_, p) = scan(&mut e, &s, &mut FakeEmbedder::new());
    assert!(p.indexed >= 1, "{p:?}");
    // …and only once.
    let (_, p) = scan(&mut e, &s, &mut FakeEmbedder::new());
    assert_eq!(p.indexed, 0, "{p:?}");
}

#[test]
fn a_filter_that_hides_everything_is_relaxed_and_said_so() {
    let mut e = env();
    let s = grant(&e, "projects");
    scan(&mut e, &s, &mut FakeEmbedder::new());
    let none = FakeModels {
        llm: false,
        pick: serde_json::Value::Null,
    };
    // "toplantı" is only in a .txt file; the user asked for a PDF.
    let out = ask(&e, &none, "toplantı notları pdf", None);
    assert_eq!(out.found[0].resolved.card.title, "notes.txt");
    assert_eq!(out.notice.as_deref(), Some("filters_relaxed"));
    // When the filter matches, nothing is relaxed.
    let out = ask(&e, &none, "budget pdf", None);
    assert_eq!(out.found[0].resolved.card.title, "report.pdf");
    assert!(out.notice.is_none());
}
