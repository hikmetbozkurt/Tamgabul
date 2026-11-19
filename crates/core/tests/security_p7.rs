//! Deterministic security, boundary, and release-gate integration tests (P7):
//! Windows junctions, drive-root OS exclusions across Windows & macOS,
//! locked/unreadable files without source mutation, zero-model fresh-profile
//! health, and schema non-downgrade enforcement.

use std::sync::atomic::AtomicBool;

use tamgabul_core::db::{self, open_and_migrate};
use tamgabul_core::describe::NoDescriber;
use tamgabul_core::error::{ErrorCode, IpcError};
use tamgabul_core::health::HealthLevel;
use tamgabul_core::index::job::{self, Embedder, JobInput, Outcome};
use tamgabul_core::index::media::NoPlatform;
use tamgabul_core::index::store::{self, GenerationSpec, Scope, ScopeState};
use tamgabul_core::index::walk::{self, Limits};
use tamgabul_core::model::{ModelCategory, ModelStatus, StatusTone};
use tamgabul_core::search;

struct FakeEmbedder;

impl Embedder for FakeEmbedder {
    fn embed_documents(&mut self, _t: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, IpcError> {
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

#[cfg(windows)]
#[test]
fn windows_junctions_never_escape_root_or_loop() {
    use std::process::Command;

    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.md");
    std::fs::write(&secret, "top secret outside grant\n").unwrap();

    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("inside.md"), "authorized note\n").unwrap();

    let escape_junction = root.path().join("escape_junction");
    let loop_junction = root.path().join("loop_junction");

    let mk = |link: &std::path::Path, target: &std::path::Path| {
        let st = Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.to_string_lossy(),
                &target.to_string_lossy(),
            ])
            .status()
            .unwrap();
        assert!(st.success(), "mklink /J failed for {}", link.display());
    };
    mk(&escape_junction, outside.path());
    mk(&loop_junction, root.path());

    let canonical_root = walk::canonical(root.path()).unwrap();
    let report = walk::walk(
        &canonical_root,
        &[],
        Limits::default(),
        &AtomicBool::new(false),
    );
    assert!(report.complete);
    assert!(
        report.skipped_links >= 2,
        "expected both junctions skipped, got {}",
        report.skipped_links
    );
    let names: Vec<String> = report
        .files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["inside.md"]);

    // Verify Acl denies the outside file even by direct path.
    let db = tempfile::tempdir().unwrap();
    let conn = open_and_migrate(&db.path().join("acl.db")).unwrap();
    store::insert_scope(
        &conn,
        &Scope {
            scope_id: "s-junc".into(),
            display_path: walk::display(&canonical_root),
            is_drive_root: false,
            canonical_path: canonical_root,
            state: ScopeState::Active,
            file_count: 0,
            last_scan_at: None,
            error_summary: None,
            max_file_bytes: None,
        },
    )
    .unwrap();
    let acl = search::Acl::load(&conn).unwrap();
    assert!(!acl.allows(&walk::canonical(&secret).unwrap()));

    // Cleanly unlink junctions before TempDir drops.
    let _ = std::fs::remove_dir(&escape_junction);
    let _ = std::fs::remove_dir(&loop_junction);
}

#[test]
fn drive_root_exclusions_cover_windows_and_macos_protected_dirs() {
    for d in [
        "Windows",
        "Program Files",
        "Program Files (x86)",
        "ProgramData",
        "$Recycle.Bin",
        "System",
        "Library",
        "private",
        "Volumes",
        "cores",
        "usr",
        "etc",
        "proc",
    ] {
        assert!(
            walk::is_excluded_dir_name(d, true),
            "drive root must exclude {d}"
        );
    }

    for global in [".git", "NODE_MODULES", "target", "AppData", "Build"] {
        assert!(
            walk::is_excluded_dir_name(global, false),
            "must exclude {global} anywhere"
        );
        assert!(walk::is_excluded_dir_name(global, true));
    }

    for normal in ["Documents", "Projects", "Belgeler", "Desktop"] {
        assert!(
            !walk::is_excluded_dir_name(normal, true),
            "must allow {normal}"
        );
        assert!(!walk::is_excluded_dir_name(normal, false));
    }
    // Drive-root-only OS names are allowed inside a regular project folder.
    assert!(!walk::is_excluded_dir_name("Library", false));
    assert!(!walk::is_excluded_dir_name("Windows", false));
}

#[test]
fn locked_or_unreadable_file_does_not_abort_scan_or_mutate_sources() {
    let db = tempfile::tempdir().unwrap();
    let mut conn = open_and_migrate(&db.path().join("lock.db")).unwrap();
    conn.execute(
        "INSERT INTO model_registry(model_id, category, display_name, path, format, runtime_adapter, adapter_version, is_active, status)
         VALUES ('m','embedding','EmbeddingGemma 2','x','gguf','llama.cpp','b1',1,'AVAILABLE')",
        [],
    )
    .unwrap();
    let generation = store::ensure_current_generation(
        &mut conn,
        &GenerationSpec {
            embedding_model_id: "m".into(),
            model_hash: "h".into(),
            model_revision: "r".into(),
            adapter_version: "b1".into(),
            preprocessing: "p".into(),
            query_instruction: "q".into(),
            document_instruction: "d".into(),
            dimension: 768,
            normalization: "l2".into(),
            modalities: vec!["text".into()],
        },
    )
    .unwrap();

    let files = tempfile::tempdir().unwrap();
    let good = files.path().join("good.md");
    let locked = files.path().join("locked.txt");
    let good_bytes = b"# Healthy Note\nIndexed reliably despite locked neighbors.\n";
    let locked_bytes = b"locked content must stay untouched";
    std::fs::write(&good, good_bytes).unwrap();
    std::fs::write(&locked, locked_bytes).unwrap();

    let canonical = walk::canonical(files.path()).unwrap();
    let scope = Scope {
        scope_id: "s-lock".into(),
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

    #[cfg(windows)]
    let _guard = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .unwrap()
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    }

    let (outcome, progress) = job::run(
        &mut conn,
        JobInput {
            scope_id: &scope.scope_id,
            generation: &generation,
            deny: &[],
            limits: Limits::default(),
            batch: 8,
            platform: &NoPlatform,
        },
        &mut FakeEmbedder,
        &mut NoDescriber,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(outcome, Outcome::Done);
    assert!(progress.indexed >= 1);

    let hits = search::search(&conn, Some(&generation), None, "reliably", 10).unwrap();
    assert_eq!(hits.len(), 1);

    #[cfg(windows)]
    drop(_guard);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    }

    assert_eq!(std::fs::read(&good).unwrap(), good_bytes);
    assert_eq!(std::fs::read(&locked).unwrap(), locked_bytes);
}

#[test]
fn fresh_profile_zero_models_and_schema_downgrade_refused() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("fresh.db");
    {
        let conn = open_and_migrate(&db_path).unwrap();
        let h = db::read_health(Some(&conn));
        assert!(h.db_ok);
        assert_eq!(h.level, HealthLevel::Blocked);
        assert!(!h.semantic_search_enabled);
        assert_eq!(h.active_scope_count, 0);
        assert_eq!(h.models.len(), 4);
        assert_eq!(h.models[0].category, ModelCategory::Embedding);
        assert_eq!(h.models[0].status, ModelStatus::Missing);
        assert_eq!(h.models[0].tone, StatusTone::Red);
        for opt in &h.models[1..] {
            assert_eq!(opt.status, ModelStatus::Missing);
            assert_eq!(opt.tone, StatusTone::Neutral);
        }

        // Simulate a database created by a newer schema version.
        conn.execute(
            "INSERT INTO schema_migrations(version, name) VALUES (999, '999_future')",
            [],
        )
        .unwrap();
    }

    let err = open_and_migrate(&db_path).unwrap_err();
    assert_eq!(err.code, ErrorCode::DbMigrationFailed);
}

#[test]
fn user_exclusion_immediately_denies_purges_and_blocks_rescan_without_mutating_sources() {
    use tamgabul_core::dto::ExclusionReason;

    let db = tempfile::tempdir().unwrap();
    let mut conn = open_and_migrate(&db.path().join("excl.db")).unwrap();
    conn.execute(
        "INSERT INTO model_registry(model_id, category, display_name, path, format, runtime_adapter, adapter_version, is_active, status)
         VALUES ('m','embedding','EmbeddingGemma 2','x','gguf','llama.cpp','b1',1,'AVAILABLE')",
        [],
    )
    .unwrap();
    let generation = store::ensure_current_generation(
        &mut conn,
        &GenerationSpec {
            embedding_model_id: "m".into(),
            model_hash: "h".into(),
            model_revision: "r".into(),
            adapter_version: "b1".into(),
            preprocessing: "p".into(),
            query_instruction: "q".into(),
            document_instruction: "d".into(),
            dimension: 768,
            normalization: "l2".into(),
            modalities: vec!["text".into()],
        },
    )
    .unwrap();

    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(outside.path().join("other")).unwrap();

    let files = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(files.path().join("public")).unwrap();
    std::fs::create_dir_all(files.path().join("secret")).unwrap();
    std::fs::create_dir_all(files.path().join("secret2")).unwrap();

    let pub_file = files.path().join("public/visible.md");
    let sec_file = files.path().join("secret/private.md");
    let sec2_file = files.path().join("secret2/vault.txt");
    let sec_bytes = b"confidential payroll secret alpha";
    let sec2_bytes = b"private ssh vault tokens beta";
    std::fs::write(&pub_file, b"public quarterly roadmap overview").unwrap();
    std::fs::write(&sec_file, sec_bytes).unwrap();
    std::fs::write(&sec2_file, sec2_bytes).unwrap();

    let canonical = walk::canonical(files.path()).unwrap();
    let scope = Scope {
        scope_id: "s-excl".into(),
        display_path: walk::display(&canonical),
        is_drive_root: false,
        canonical_path: canonical.clone(),
        state: ScopeState::Active,
        file_count: 0,
        last_scan_at: None,
        error_summary: None,
        max_file_bytes: None,
    };
    store::insert_scope(&conn, &scope).unwrap();

    let run_scan = |conn: &mut rusqlite::Connection| {
        job::run(
            conn,
            JobInput {
                scope_id: &scope.scope_id,
                generation: &generation,
                deny: &[],
                limits: Limits::default(),
                batch: 8,
                platform: &NoPlatform,
            },
            &mut FakeEmbedder,
            &mut NoDescriber,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap()
    };

    let (out, prog) = run_scan(&mut conn);
    assert_eq!(out, Outcome::Done);
    assert_eq!(prog.indexed, 3);
    assert_eq!(
        search::search(&conn, Some(&generation), None, "payroll", 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        search::search(&conn, Some(&generation), None, "vault", 10)
            .unwrap()
            .len(),
        1
    );

    // Exclude one subfolder relatively and another by absolute canonical path.
    let ex1 = store::add_exclusion(&mut conn, &scope.scope_id, "secret").unwrap();
    assert_eq!(ex1.reason, ExclusionReason::UserExclusion);
    let sec2_dir = walk::canonical(&files.path().join("secret2")).unwrap();
    let ex2 =
        store::add_exclusion(&mut conn, &scope.scope_id, &sec2_dir.to_string_lossy()).unwrap();
    assert_eq!(ex2.reason, ExclusionReason::UserExclusion);

    // Idempotent duplicate add keeps same record without duplicating.
    let dup = store::add_exclusion(&mut conn, &scope.scope_id, "secret").unwrap();
    assert_eq!(dup.exclusion_id, ex1.exclusion_id);
    assert_eq!(
        store::list_exclusions(&conn, &scope.scope_id)
            .unwrap()
            .len(),
        2
    );

    // Immediate ACL deny, immediate purge of derived docs/FTS, and accurate file_count.
    let acl = search::Acl::load(&conn).unwrap();
    assert!(acl.allows(&walk::canonical(&pub_file).unwrap()));
    assert!(!acl.allows(&walk::canonical(&sec_file).unwrap()));
    assert!(!acl.allows(&walk::canonical(&sec2_file).unwrap()));
    assert!(
        search::search(&conn, Some(&generation), None, "payroll", 10)
            .unwrap()
            .is_empty()
    );
    assert!(search::search(&conn, Some(&generation), None, "vault", 10)
        .unwrap()
        .is_empty());
    assert_eq!(
        search::search(&conn, Some(&generation), None, "roadmap", 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store::get_scope(&conn, &scope.scope_id)
            .unwrap()
            .unwrap()
            .file_count,
        1
    );

    // Outside-scope and scope-root exclusions are rejected.
    let outside_err = store::add_exclusion(
        &mut conn,
        &scope.scope_id,
        &outside.path().join("other").to_string_lossy(),
    )
    .unwrap_err();
    assert_eq!(outside_err.code, ErrorCode::ScopeNotAuthorized);
    assert_eq!(
        store::add_exclusion(&mut conn, &scope.scope_id, ".")
            .unwrap_err()
            .code,
        ErrorCode::ScopeNotAuthorized
    );

    // Rescan skips both excluded folders cleanly.
    let (out2, prog2) = run_scan(&mut conn);
    assert_eq!(out2, Outcome::Done);
    assert_eq!(prog2.indexed, 0);
    assert_eq!(prog2.unchanged, 1);
    assert!(
        search::search(&conn, Some(&generation), None, "payroll", 10)
            .unwrap()
            .is_empty()
    );

    // Source files remain untouched.
    assert_eq!(std::fs::read(&sec_file).unwrap(), sec_bytes);
    assert_eq!(std::fs::read(&sec2_file).unwrap(), sec2_bytes);

    // Removing the first exclusion restores indexing on next scan.
    store::remove_exclusion(&conn, &scope.scope_id, &ex1.exclusion_id).unwrap();
    let (out3, prog3) = run_scan(&mut conn);
    assert_eq!(out3, Outcome::Done);
    assert_eq!(prog3.indexed, 1);
    assert_eq!(
        search::search(&conn, Some(&generation), None, "payroll", 10)
            .unwrap()
            .len(),
        1
    );

    // Protected exclusions cannot be removed by the user.
    conn.execute(
        "INSERT INTO scope_exclusions(exclusion_id, parent_scope_id, canonical_path, reason)
         VALUES ('x-prot', ?1, ?2, 'protected_path')",
        rusqlite::params![scope.scope_id, sec2_dir.to_string_lossy()],
    )
    .unwrap();
    let err_prot = store::remove_exclusion(&conn, &scope.scope_id, "x-prot").unwrap_err();
    assert_eq!(err_prot.code, ErrorCode::ScopeNotAuthorized);
}

#[test]
fn commit_document_and_record_skipped_refuse_excluded_paths() {
    use tamgabul_core::index::store::{DocMeta, NewChunk};
    use tamgabul_core::index::walk::SourceKind;

    let db = tempfile::tempdir().unwrap();
    let mut conn = open_and_migrate(&db.path().join("race.db")).unwrap();
    conn.execute(
        "INSERT INTO model_registry(model_id, category, display_name, path, format, runtime_adapter, adapter_version, is_active, status)
         VALUES ('m','embedding','EmbeddingGemma 2','x','gguf','llama.cpp','b1',1,'AVAILABLE')",
        [],
    )
    .unwrap();
    let generation = store::ensure_current_generation(
        &mut conn,
        &GenerationSpec {
            embedding_model_id: "m".into(),
            model_hash: "h".into(),
            model_revision: "r".into(),
            adapter_version: "b1".into(),
            preprocessing: "p".into(),
            query_instruction: "q".into(),
            document_instruction: "d".into(),
            dimension: 768,
            normalization: "l2".into(),
            modalities: vec!["text".into()],
        },
    )
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("blocked")).unwrap();
    let sneaky = dir.path().join("blocked/sneaky.md");
    std::fs::write(&sneaky, "should never commit").unwrap();

    let canonical = walk::canonical(dir.path()).unwrap();
    let scope = Scope {
        scope_id: "s-race".into(),
        display_path: walk::display(&canonical),
        is_drive_root: false,
        canonical_path: canonical.clone(),
        state: ScopeState::Active,
        file_count: 0,
        last_scan_at: None,
        error_summary: None,
        max_file_bytes: None,
    };
    store::insert_scope(&conn, &scope).unwrap();
    store::add_exclusion(&mut conn, &scope.scope_id, "blocked").unwrap();

    let sneaky_canonical = walk::canonical(&sneaky).unwrap();
    let meta = DocMeta {
        path: &sneaky_canonical,
        kind: SourceKind::Markdown,
        size: 19,
        mtime: "1.0",
        content_hash: Some("abc"),
    };
    let chunk = NewChunk {
        ordinal: 0,
        modality: "text",
        text: Some("should never commit".into()),
        page: None,
        line_start: Some(1),
        line_end: Some(1),
        time_ms: None,
        vector: vec![0.1; 768],
    };

    assert!(
        !store::commit_document(&mut conn, &scope.scope_id, &generation, &meta, &[chunk]).unwrap()
    );
    assert!(!store::record_skipped(&mut conn, &scope.scope_id, &meta, "SKIPPED", "test").unwrap());
    assert_eq!(store::indexed_file_count(&conn).unwrap(), 0);
}
