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
        classify_scanned_file(
            file,
            indexed_by_path.get(file.relative_path.as_str()).copied(),
            &mut modified,
            &mut added,
            &mut mtime_refreshes,
        );
    }

    let deleted_ids = detect_deleted_ids(&indexed, &discovered_paths, walk.walk_had_errors);

    Ok(ChangeSet {
        modified,
        added,
        deleted_ids,
        mtime_refreshes,
    })
}

/// Classify one scanned file against its indexed counterpart (if any),
/// appending to the appropriate change list. Extracted from `detect_changes`
/// so the orchestrator stays below the function-length threshold.
fn classify_scanned_file(
    file: &crate::ingest::scanner::WalkedFile,
    meta: Option<&IndexedFileMeta>,
    modified: &mut Vec<String>,
    added: &mut Vec<String>,
    mtime_refreshes: &mut Vec<(i64, i64)>,
) {
    let Some(meta) = meta else {
        added.push(file.relative_path.clone());
        return;
    };
    // Fast path: on-disk mtime matches the mtime captured when we
    // last indexed this file → the file hasn't been touched. Only
    // valid when mtime is a real value (non-zero sentinel) on both
    // sides; 0 means "unknown", always force a hash verification.
    if file.mtime_nanos != 0 && meta.mtime_nanos != 0 && file.mtime_nanos == meta.mtime_nanos {
        return;
    }
    // Suspect: mtime moved or one side is unknown — hash to confirm
    // a real content change. Mtime bumps from `touch` / `git checkout`
    // / editor save-without-change produce a matching hash and are not flagged.
    match hasher::hash_file(&file.abs_path) {
        Ok(fresh_hash) if fresh_hash != meta.hash => {
            modified.push(file.relative_path.clone());
        }
        Ok(_) => {
            // Hash matches: content is unchanged. Update the stored mtime so
            // the fast-path can trust it next call (prevents forever-rehashing
            // after `touch` / legacy-migrated rows with mtime_nanos=0).
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

/// Compute which indexed files are no longer on disk, suppressing deletion
/// entirely when the walk hit errors (can't tell absence from unreadability).
fn detect_deleted_ids(
    indexed: &[IndexedFileMeta],
    discovered_paths: &HashSet<&str>,
    walk_had_errors: bool,
) -> Vec<i64> {
    if walk_had_errors {
        eprintln!(
            "rlm: staleness walk hit errors; skipping deletion phase to preserve indexed entries"
        );
        return Vec::new();
    }
    indexed
        .iter()
        .filter(|f| !discovered_paths.contains(f.path.as_str()))
        .map(|f| f.id)
        .collect()
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
#[path = "staleness_tests.rs"]
mod tests;
