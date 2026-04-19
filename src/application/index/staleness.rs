//! Self-healing index: detect filesystem changes and reindex on demand.
//!
//! Invoked at the canonical DB-open seam so every CLI/MCP tool call sees
//! an up-to-date index regardless of who modified the files (rlm itself,
//! Claude Code's native tools, vim, `git pull`, etc.).
//!
//! Set `RLM_SKIP_REFRESH=1` to skip the check (useful for performance-
//! sensitive scripts that already know the index is fresh).

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::config::Config;
use crate::db::queries::IndexedFileMeta;
use crate::db::Database;
use crate::error::Result;
use crate::ingest::hasher;
use crate::ingest::scanner::Scanner;

use super::reindex_single_file;

/// Environment variable to skip staleness checks.
const SKIP_ENV: &str = "RLM_SKIP_REFRESH";

/// Summary of what `ensure_index_fresh` did for the calling tool invocation.
#[derive(Debug, Default, Clone, Copy)]
pub struct ChangeReport {
    /// Files whose content hash changed since last index.
    pub reindexed: usize,
    /// Files present on disk but missing from the index.
    pub added: usize,
    /// Files removed from the index because they no longer exist on disk.
    pub deleted: usize,
    /// Wall-clock time for the detection + application phase, in milliseconds.
    pub elapsed_ms: u64,
}

impl ChangeReport {
    /// Returns true if no files were reindexed, added, or deleted.
    /// Useful for callers that want to log or branch only on actual changes.
    // qual:api
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.reindexed == 0 && self.added == 0 && self.deleted == 0
    }
}

/// Partition of on-disk / indexed file paths into the change categories.
struct ChangeSet {
    /// Relative paths whose DB hash differs from the scanner's fresh hash.
    modified: Vec<String>,
    /// Relative paths present on disk but not yet in the index.
    added: Vec<String>,
    /// File IDs in the DB whose path no longer exists on disk.
    deleted_ids: Vec<i64>,
    /// `(file_id, new_mtime_nanos)` pairs where hash matched despite an mtime
    /// bump (e.g. `touch`, `git checkout`). Applying refreshes the stored
    /// mtime so the fast-path trusts the file next time instead of re-hashing
    /// it on every call (important for post-`git-pull` and legacy migrated rows).
    mtime_refreshes: Vec<(i64, i64)>,
}

/// Ensure the DB's index reflects the current state of the filesystem.
///
/// Returns immediately with a clean `ChangeReport` if `RLM_SKIP_REFRESH` is
/// set (any non-empty value). On internal errors (permission issues, I/O
/// hiccups) logs to stderr and returns a clean report — a slightly stale
/// index is better than a blocked tool call.
///
/// # Performance
/// Mtime-first: the common path is one stat per file, no hashing. Files are
/// hashed only when their on-disk mtime (nanoseconds) differs from the
/// per-file `files.mtime_nanos` stored at the last index — i.e., the subset
/// of files that were actually touched since their last verification. After
/// a hash-verified no-op (e.g. `touch`, `git checkout`), the stored mtime is
/// refreshed so the fast-path stays effective. For clean projects the
/// overhead is O(files) stats (~tens of ms even on large repos). For a
/// typical edit session, only a handful of files are hashed per call.
///
/// Nanosecond precision avoids the same-second false negative that a
/// second-precision comparison would suffer from — on modern filesystems
/// (ext4 / NTFS / APFS / btrfs), rapid edit sequences produce distinct
/// mtimes and are correctly detected.
///
/// The `RLM_SKIP_REFRESH=1` env var remains available as an escape hatch for
/// cases where even stat-per-file is unwanted (e.g., batch scripts over huge
/// trees that explicitly manage their own indexing).
///
/// # Errors
/// Returns `Ok(ChangeReport)` in all non-catastrophic cases. Errors that would
/// propagate here have already been caught and logged; callers can treat this
/// as infallible for practical purposes.
// qual:allow(iosp) reason: "integration: orchestrates skip-check / measure / error-handling"
pub fn ensure_index_fresh(db: &Database, config: &Config) -> Result<ChangeReport> {
    if skip_requested() {
        return Ok(ChangeReport::default());
    }
    let start = Instant::now();
    match detect_and_apply(db, config) {
        Ok(mut report) => {
            report.elapsed_ms = elapsed_ms(start);
            Ok(report)
        }
        Err(e) => {
            eprintln!("rlm: staleness check failed ({e}); continuing with existing index");
            Ok(ChangeReport::default())
        }
    }
}

fn skip_requested() -> bool {
    std::env::var(SKIP_ENV).is_ok_and(|v| !v.is_empty())
}

fn elapsed_ms(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Full detect + apply cycle. Runs only when staleness check is enabled.
// qual:allow(iosp) reason: "integration: detect + apply are both sub-steps of the cycle"
fn detect_and_apply(db: &Database, config: &Config) -> Result<ChangeReport> {
    let changes = detect_changes(db, config)?;
    apply_changes(db, config, changes)
}

/// Compare the DB state against the on-disk state and classify each file.
///
/// Mtime-first strategy: walk (stat only, no hashing), compare each file's
/// on-disk `mtime_nanos` to the per-file `mtime_nanos` stored in `files` from
/// the last index. Hash only files whose mtime changed since the last
/// recorded scan. This keeps the per-call cost ≈ stat-per-file on clean
/// projects, while still catching content edits via hash verification when
/// mtime bumps. After a hash-verified no-op (touch, git checkout) the
/// stored mtime gets refreshed so the fast-path trusts the file next time.
// qual:allow(iosp) reason: "partitioning the three change categories requires orchestration"
fn detect_changes(db: &Database, config: &Config) -> Result<ChangeSet> {
    let indexed = db.get_indexed_files_meta()?;
    let scanner = Scanner::with_max_file_size(
        &config.project_root,
        config.settings.indexing.max_file_size_mb,
    );
    let walk = scanner.walk()?;

    let indexed_by_path: HashMap<&str, &IndexedFileMeta> =
        indexed.iter().map(|f| (f.path.as_str(), f)).collect();
    // Existence check uses `discovered` (the full pre-filtering list), not
    // `files`. Otherwise transient metadata-read errors would drop a file
    // from the walk output and wrongly mark an indexed entry as deleted.
    let discovered_paths: HashSet<&str> = walk.discovered.iter().map(String::as_str).collect();

    let mut modified = Vec::new();
    let mut added = Vec::new();
    let mut mtime_refreshes = Vec::new();
    for file in &walk.files {
        match indexed_by_path.get(file.relative_path.as_str()) {
            None => added.push(file.relative_path.clone()),
            Some(meta) => {
                // Fast path: on-disk mtime matches the mtime captured when we
                // last indexed this file → the file hasn't been touched. Only
                // valid when mtime is a real value (non-zero sentinel) on both
                // sides; 0 means "unknown", always force a hash verification.
                if file.mtime_nanos != 0
                    && meta.mtime_nanos != 0
                    && file.mtime_nanos == meta.mtime_nanos
                {
                    continue;
                }
                // Suspect: mtime moved or one side is unknown — hash to confirm
                // a real content change. Mtime bumps from `touch` /
                // `git checkout` / editor save-without-change produce a
                // matching hash and are not flagged.
                match hasher::hash_file(&file.abs_path) {
                    Ok(fresh_hash) if fresh_hash != meta.hash => {
                        modified.push(file.relative_path.clone());
                    }
                    Ok(_) => {
                        // Hash matches: content is unchanged. Update the
                        // stored mtime so the fast-path can trust it next
                        // call (prevents forever-rehashing after `touch` /
                        // legacy-migrated rows with mtime_nanos=0).
                        if meta.mtime_nanos != file.mtime_nanos {
                            mtime_refreshes.push((meta.id, file.mtime_nanos));
                        }
                    }
                    Err(e) => {
                        // Don't silently drift: surface the path that failed.
                        // Next tool call re-attempts (eventual consistency).
                        eprintln!("rlm: staleness hash failed for {}: {e}", file.relative_path);
                    }
                }
            }
        }
    }

    // Only compute deletions when the walk was complete. If the walker hit
    // errors (permission / IO on a subdirectory), `discovered_paths` is
    // known-incomplete and a "missing" indexed file might still exist inside
    // the unreadable branch — classifying it as deleted would drop real data.
    let deleted_ids: Vec<i64> = if walk.walk_had_errors {
        eprintln!(
            "rlm: staleness walk hit errors; skipping deletion phase to preserve indexed entries"
        );
        Vec::new()
    } else {
        indexed
            .iter()
            .filter(|f| !discovered_paths.contains(f.path.as_str()))
            .map(|f| f.id)
            .collect()
    };

    Ok(ChangeSet {
        modified,
        added,
        deleted_ids,
        mtime_refreshes,
    })
}

/// Apply a `ChangeSet` to the index: delete removed files, reindex changed/new.
///
/// Per-file atomicity comes from `reindex_single_file` (which wraps each file
/// in its own SQLite transaction). Wrapping the whole batch in one outer
/// transaction would require nested `BEGIN` which SQLite doesn't support.
///
/// Continues on error rather than bailing early: a broken file must not block
/// reconciliation of others. Failures are logged to stderr with the file path
/// so silent drift is visible; failed files stay flagged as drifted and get
/// retried on the next tool call (eventual consistency).
// qual:allow(iosp) reason: "sequential application of four category groups (delete / reindex-modified / reindex-added / mtime-refresh)"
fn apply_changes(db: &Database, config: &Config, changes: ChangeSet) -> Result<ChangeReport> {
    let mut report = ChangeReport::default();
    let mut failures: Vec<String> = Vec::new();

    for id in changes.deleted_ids {
        match db.delete_file(id) {
            Ok(()) => report.deleted += 1,
            Err(e) => failures.push(format!("delete file id={id}: {e}")),
        }
    }

    for path in changes.modified {
        match reindex_single_file(db, config, &path) {
            Ok(_) => report.reindexed += 1,
            Err(e) => failures.push(format!("reindex modified {path}: {e}")),
        }
    }

    for path in changes.added {
        match reindex_single_file(db, config, &path) {
            Ok(_) => report.added += 1,
            Err(e) => failures.push(format!("reindex added {path}: {e}")),
        }
    }

    // Refresh stored mtimes for files that had mtime bumps but identical
    // content — ensures the fast-path trusts them on the next invocation.
    for (id, mtime_nanos) in changes.mtime_refreshes {
        if let Err(e) = db.update_file_mtime(id, mtime_nanos) {
            failures.push(format!("refresh mtime id={id}: {e}"));
        }
    }

    if !failures.is_empty() {
        let succeeded = report.reindexed + report.added + report.deleted;
        eprintln!(
            "rlm: staleness check partially succeeded ({} applied, {} failed)",
            succeeded,
            failures.len()
        );
        for msg in &failures {
            eprintln!("  - {msg}");
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::index::run_index;
    use std::fs;
    use tempfile::TempDir;

    fn setup_indexed(files: &[(&str, &str)]) -> (TempDir, Config, Database) {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        for (name, content) in files {
            fs::write(src.join(name), content).unwrap();
        }
        let config = Config::new(tmp.path());
        run_index(&config, None).unwrap();
        let db = Database::open(&config.db_path).unwrap();
        (tmp, config, db)
    }

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
}
