//! Tests for `staleness.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "staleness_tests.rs"] mod tests;`.

use super::super::fixtures::setup_indexed;
use super::{apply_changes, ensure_index_fresh, ChangeReport, ChangeSet};
use crate::application::index::run_index;
use crate::config::Config;
use crate::db::Database;
use std::fs;
use tempfile::TempDir;

#[test]
fn ensure_fresh_is_clean_on_unchanged_project() {
    let (_tmp, config, db) = setup_indexed(&[("main.rs", "fn main() {}")]);
    let report = ensure_index_fresh(&db, &config).unwrap();
    assert!(report.is_clean(), "no changes expected, got {report:?}");
}

#[test]
fn ensure_fresh_does_not_rehash_file_indexed_in_same_second_as_edit() {
    // Regression: previously the staleness check compared file.mtime_nanos
    // against indexed_at (second-precision), so a file edited and indexed
    // in the SAME second would always hit the "suspect" path on every
    // subsequent call, hashing it repeatedly despite being unchanged.
    //
    // With the stored file-mtime approach, a clean re-check after index
    // reports zero modifications regardless of whether index+edit fell
    // inside the same wall-clock second.
    let (_tmp, config, db) = setup_indexed(&[("main.rs", "fn main() {}")]);

    // First call: should be clean (no changes since index).
    let report1 = ensure_index_fresh(&db, &config).unwrap();
    assert!(report1.is_clean(), "first call clean, got {report1:?}");

    // Second call back-to-back (within the same second): still clean.
    let report2 = ensure_index_fresh(&db, &config).unwrap();
    assert!(
        report2.is_clean(),
        "same-second repeat call must stay clean, got {report2:?}"
    );
}

#[test]
fn ensure_fresh_refreshes_stored_mtime_after_touch_without_content_change() {
    // Regression: after a touch (mtime bump, content identical), staleness
    // must UPDATE the stored mtime_nanos so subsequent calls hit the fast
    // path instead of re-hashing the file forever. Legacy rows with
    // mtime_nanos=0 follow the same self-healing path.
    const SECOND_BOUNDARY_MS: u64 = 1100;

    let (tmp, config, db) = setup_indexed(&[("main.rs", "fn main() {}")]);

    // Read the stored mtime after indexing.
    let stored_before = db
        .get_file_by_path("src/main.rs")
        .unwrap()
        .expect("indexed")
        .mtime_nanos;

    // Bump mtime by rewriting the same content after the second boundary.
    std::thread::sleep(std::time::Duration::from_millis(SECOND_BOUNDARY_MS));
    let main = tmp.path().join("src/main.rs");
    let content = fs::read(&main).unwrap();
    fs::write(&main, &content).unwrap();

    ensure_index_fresh(&db, &config).unwrap();

    let stored_after = db
        .get_file_by_path("src/main.rs")
        .unwrap()
        .expect("indexed")
        .mtime_nanos;

    assert_ne!(
        stored_before, stored_after,
        "mtime must be refreshed after touch-without-change"
    );
    // Fast path should now be clean without re-hashing.
    let report2 = ensure_index_fresh(&db, &config).unwrap();
    assert!(
        report2.is_clean(),
        "next call must trust the refreshed mtime, got {report2:?}"
    );
}

#[test]
fn ensure_fresh_ignores_mtime_bump_without_content_change() {
    // Mtime-first optimization: touching a file (bumping mtime without
    // changing content — e.g. `git checkout`, editor save-without-change)
    // must not cause a reindex. The hash verification on suspect files
    // catches this correctly.
    //
    // Sleep past a second boundary so the rewrite guarantees
    // `mtime_nanos > indexed_at_secs` (SQLite timestamps are 1s-precise).
    const SECOND_BOUNDARY_MS: u64 = 1100;

    let (tmp, config, db) = setup_indexed(&[("main.rs", "fn main() {}")]);

    std::thread::sleep(std::time::Duration::from_millis(SECOND_BOUNDARY_MS));

    // Rewrite with identical bytes → mtime bumps, content unchanged.
    let main = tmp.path().join("src/main.rs");
    let content = fs::read(&main).unwrap();
    fs::write(&main, &content).unwrap();

    let report = ensure_index_fresh(&db, &config).unwrap();
    assert_eq!(
        report.reindexed, 0,
        "mtime bump without content change must not trigger reindex, got {report:?}"
    );
    assert_eq!(report.added, 0);
    assert_eq!(report.deleted, 0);
}

#[test]
fn ensure_fresh_detects_modified_file() {
    let (tmp, config, db) = setup_indexed(&[("main.rs", "fn main() {}")]);
    fs::write(
        tmp.path().join("src/main.rs"),
        "fn main() {\n    println!(\"changed\");\n}",
    )
    .unwrap();
    let report = ensure_index_fresh(&db, &config).unwrap();
    assert_eq!(report.reindexed, 1, "got {report:?}");
    assert_eq!(report.added, 0);
    assert_eq!(report.deleted, 0);
}

#[test]
fn ensure_fresh_detects_added_file() {
    let (tmp, config, db) = setup_indexed(&[("main.rs", "fn main() {}")]);
    fs::write(tmp.path().join("src/helper.rs"), "fn helper() {}").unwrap();
    let report = ensure_index_fresh(&db, &config).unwrap();
    assert_eq!(report.added, 1, "got {report:?}");
    assert_eq!(report.reindexed, 0);
    assert_eq!(report.deleted, 0);
}

#[test]
fn ensure_fresh_detects_deleted_file() {
    let (tmp, config, db) =
        setup_indexed(&[("main.rs", "fn main() {}"), ("helper.rs", "fn helper() {}")]);
    fs::remove_file(tmp.path().join("src/helper.rs")).unwrap();
    let report = ensure_index_fresh(&db, &config).unwrap();
    assert_eq!(report.deleted, 1, "got {report:?}");
    assert_eq!(report.reindexed, 0);
    assert_eq!(report.added, 0);
}

#[cfg(unix)]
#[test]
fn ensure_fresh_skips_deletion_when_walk_hits_errors() {
    // Regression: permission-denied on a subdirectory causes walker errors,
    // which leave `discovered` incomplete. Staleness must NOT delete
    // indexed files that could still exist inside the unreadable subdir.
    use std::os::unix::fs::PermissionsExt;
    const DENY: u32 = 0o000;
    const RESTORE: u32 = 0o755;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let private = src.join("private");
    fs::create_dir_all(&private).unwrap();
    fs::write(src.join("main.rs"), "fn main() {}").unwrap();
    fs::write(private.join("secret.rs"), "fn secret() {}").unwrap();

    let config = Config::new(tmp.path());
    run_index(&config, None).unwrap();
    let db = Database::open(&config.db_path).unwrap();

    // Revoke access so walker errors on reading the private subdir.
    fs::set_permissions(&private, fs::Permissions::from_mode(DENY)).unwrap();

    // Root-bypass check: skip if permissions are ineffective.
    if std::fs::read_dir(&private).is_ok() {
        let _ = fs::set_permissions(&private, fs::Permissions::from_mode(RESTORE));
        eprintln!("skipping: effective UID bypasses file permissions (root?)");
        return;
    }

    let report = ensure_index_fresh(&db, &config).unwrap();

    // Restore before asserting so TempDir cleanup works.
    let _ = fs::set_permissions(&private, fs::Permissions::from_mode(RESTORE));

    // secret.rs is still on disk but invisible to the walk; must NOT be
    // classified as deleted.
    assert_eq!(
        report.deleted, 0,
        "walker errors must prevent deletion of files in unreadable subdirs; got {report:?}"
    );
    let files = db.get_all_files().unwrap();
    assert!(
        files.iter().any(|f| f.path.contains("secret")),
        "secret.rs must remain indexed after a walk with errors"
    );
}

#[test]
fn ensure_fresh_preserves_index_for_files_exceeding_size_limit() {
    // Regression: a file that's been indexed and subsequently grows past
    // the `max_file_size_mb` limit must NOT be deleted from the index.
    // `walk()` drops it from the stat'd list, but keeps it in `discovered`
    // so staleness can distinguish "too big now" from "truly gone".
    const ONE_MB_BYTES: usize = 1024 * 1024;

    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    let big = src.join("big.rs");
    fs::write(&big, "fn main() {}").unwrap();

    let mut config = Config::new(tmp.path());
    run_index(&config, None).unwrap();

    // Grow the file past 1 MB, then tighten the limit so walk() drops it.
    let large = format!("fn main() {{}}\n{}", "x".repeat(2 * ONE_MB_BYTES));
    fs::write(&big, large).unwrap();
    config.settings.indexing.max_file_size_mb = 1;

    let db = Database::open(&config.db_path).unwrap();
    let report = ensure_index_fresh(&db, &config).unwrap();
    assert_eq!(
        report.deleted, 0,
        "oversized file must stay indexed, not be deleted; got {report:?}"
    );
}

#[test]
fn apply_changes_continues_on_per_file_failure() {
    // Regression: if one file's reindex fails (e.g. deleted between walk
    // and reindex, or corrupt content), the batch must continue with the
    // remaining files rather than bailing. Only successful files count
    // toward the report; the failure is logged to stderr.
    let (_tmp, config, db) = setup_indexed(&[("main.rs", "fn main() {}")]);

    // Craft a ChangeSet that mixes a real file (nonexistent relative path
    // → reindex_single_file fails on file read) with a no-op deletion.
    let changes = ChangeSet {
        modified: vec![],
        added: vec!["does_not_exist.rs".to_string()],
        deleted_ids: vec![],
        mtime_refreshes: vec![],
    };
    let report = apply_changes(&db, &config, changes)
        .expect("apply_changes must not propagate per-file errors");

    assert_eq!(
        report.added, 0,
        "failed file must not be counted as successfully added"
    );
    assert_eq!(report.deleted, 0);
    assert_eq!(report.reindexed, 0);
}

#[test]
fn ensure_fresh_handles_mixed_changes() {
    let (tmp, config, db) = setup_indexed(&[
        ("a.rs", "fn a() {}"),
        ("b.rs", "fn b() {}"),
        ("c.rs", "fn c() {}"),
    ]);
    // Modify a, delete b, add d
    fs::write(tmp.path().join("src/a.rs"), "fn a_changed() {}").unwrap();
    fs::remove_file(tmp.path().join("src/b.rs")).unwrap();
    fs::write(tmp.path().join("src/d.rs"), "fn d() {}").unwrap();

    let report = ensure_index_fresh(&db, &config).unwrap();
    assert_eq!(report.reindexed, 1, "expected 1 modified, got {report:?}");
    assert_eq!(report.deleted, 1, "expected 1 deleted, got {report:?}");
    assert_eq!(report.added, 1, "expected 1 added, got {report:?}");
}

// `RLM_SKIP_REFRESH` env var end-to-end behavior is exercised by the
// integration test `cli_respects_skip_refresh_env` in tests/staleness_tests.rs,
// which runs each test in its own process (no parallel env-var races).

#[test]
fn change_report_is_clean_when_all_zero() {
    let report = ChangeReport::default();
    assert!(report.is_clean());
}

#[test]
fn change_report_not_clean_when_any_change() {
    let report = ChangeReport {
        reindexed: 1,
        ..Default::default()
    };
    assert!(!report.is_clean());
}
