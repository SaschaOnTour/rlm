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
/// hashed only when their on-disk mtime is newer than the DB's `indexed_at`,
/// which matches the subset of files the user actually touched since the last
/// index. For clean projects the overhead is O(files) stats (~tens of ms even
/// on large repos). For a typical edit session, only a handful of files are
/// hashed per call.
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
/// `mtime_secs` to the DB's `indexed_at_secs`. Hash only files that were
/// touched after their last index. This keeps the per-call cost ≈ stat-per-
/// file on clean projects, while still catching content edits via hash
/// verification when mtime bumps.
// qual:allow(iosp) reason: "partitioning the three change categories requires orchestration"
fn detect_changes(db: &Database, config: &Config) -> Result<ChangeSet> {
    let indexed = db.get_indexed_files_meta()?;
    let scanner = Scanner::with_max_file_size(
        &config.project_root,
        config.settings.indexing.max_file_size_mb,
    );
    let walked = scanner.walk()?;

    let indexed_by_path: HashMap<&str, &IndexedFileMeta> =
        indexed.iter().map(|f| (f.path.as_str(), f)).collect();
    let walked_paths: HashSet<&str> = walked.iter().map(|f| f.relative_path.as_str()).collect();

    let mut modified = Vec::new();
    let mut added = Vec::new();
    for file in &walked {
        match indexed_by_path.get(file.relative_path.as_str()) {
            None => added.push(file.relative_path.clone()),
            Some(meta) => {
                // Fast path: mtime is strictly older than indexed_at → trust.
                // SQLite's CURRENT_TIMESTAMP is second-precision, so equal
                // seconds are ambiguous within a 1s window — hash to be safe.
                if file.mtime_secs < meta.indexed_at_secs {
                    continue;
                }
                // Suspect: hash to confirm a real content change
                // (mtime bumps from `touch` / git checkout / editor save-
                // without-change should not count as modifications).
                match hasher::hash_file(&file.abs_path) {
                    Ok(fresh_hash) if fresh_hash != meta.hash => {
                        modified.push(file.relative_path.clone());
                    }
                    _ => {} // hash error → skip; hash matches → genuinely unchanged
                }
            }
        }
    }

    let deleted_ids: Vec<i64> = indexed
        .iter()
        .filter(|f| !walked_paths.contains(f.path.as_str()))
        .map(|f| f.id)
        .collect();

    Ok(ChangeSet {
        modified,
        added,
        deleted_ids,
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
// qual:allow(iosp) reason: "sequential application of three category groups"
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
    use crate::indexer::run_index;
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
    fn ensure_fresh_ignores_mtime_bump_without_content_change() {
        // Mtime-first optimization: touching a file (bumping mtime without
        // changing content — e.g. `git checkout`, editor save-without-change)
        // must not cause a reindex. The hash verification on suspect files
        // catches this correctly.
        //
        // Sleep past a second boundary so the rewrite guarantees
        // `mtime_secs > indexed_at_secs` (SQLite timestamps are 1s-precise).
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
