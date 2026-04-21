use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::Serialize;

use crate::error::Result;
use crate::ingest::hasher;

/// Bytes per megabyte (1024 * 1024).
const BYTES_PER_MB: u64 = 1024 * 1024;

/// Reason why a file was skipped during scanning/indexing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// File extension is not supported for indexing.
    UnsupportedExtension,
    /// File exceeds the configured `max_file_size_mb` limit.
    TooLarge,
    /// File content is not valid UTF-8.
    NonUtf8,
    /// IO error while reading the file.
    IoError,
    /// Language parser doesn't support this file type.
    UnsupportedLanguage,
    /// File hash unchanged (incremental indexing).
    Unchanged,
}

impl SkipReason {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            SkipReason::UnsupportedExtension => "unsupported_extension",
            SkipReason::TooLarge => "too_large",
            SkipReason::NonUtf8 => "non_utf8",
            SkipReason::IoError => "io_error",
            SkipReason::UnsupportedLanguage => "unsupported_language",
            SkipReason::Unchanged => "unchanged",
        }
    }
}

/// Discovered file with metadata (for indexing - only supported files).
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub hash: String,
    pub size: u64,
    pub extension: String,
    /// File mtime at scan time, in nanoseconds since the Unix epoch.
    /// Persisted to `files.mtime_nanos` so staleness can trust-skip hashing
    /// when the on-disk mtime is unchanged. Nanosecond precision avoids the
    /// same-second ambiguity that second-precision timestamps would suffer.
    pub mtime_nanos: i64,
}

/// Stat-only file metadata from `Scanner::walk` — no hash.
///
/// Used by `staleness::detect_changes` to skip SHA-256 on files whose
/// on-disk `mtime_nanos` still exactly matches the per-file
/// `files.mtime_nanos` stored in the DB (i.e., unchanged since the last
/// indexed version of that file). Much cheaper than `ScannedFile` for the
/// clean-project common case.
#[derive(Debug, Clone)]
pub struct WalkedFile {
    pub abs_path: PathBuf,
    pub relative_path: String,
    pub extension: String,
    pub size: u64,
    /// Last-modified time, nanoseconds since the Unix epoch (matching
    /// `files.mtime_nanos` in the DB for exact equality comparisons).
    pub mtime_nanos: i64,
}

/// Result of `Scanner::walk`: successfully-stat'd files AND the full list of
/// discovered relative paths (including entries whose metadata read failed or
/// that exceed the size limit). Splitting lets staleness detect deletions
/// from the discovered set without losing index entries to transient I/O errors.
#[derive(Debug, Clone)]
pub struct WalkResult {
    /// Files that were successfully stat'd within the size limit.
    pub files: Vec<WalkedFile>,
    /// Relative paths of every supported file the walker confirmed exists,
    /// even if its metadata couldn't be read. Use this to distinguish
    /// "file truly deleted" from "file still on disk but transiently
    /// unreadable / over the configured size limit".
    pub discovered: Vec<String>,
    /// True if the underlying `ignore::WalkBuilder` encountered any errors
    /// (permission denied on a subdirectory, IO hiccups, ...). When set, the
    /// `discovered` list is known-incomplete: staleness must skip the
    /// deletion phase because a missing path might still exist on disk but
    /// inside an unreadable branch. Preserving index entries over transient
    /// walk errors beats silently dropping them.
    pub walk_had_errors: bool,
}

/// A discovered file (may or may not be indexable).
/// Used by `rlm files` to show ALL files in the project.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredFile {
    /// Relative path from project root (forward slashes)
    pub path: String,
    /// File extension (lowercase, without dot)
    pub extension: String,
    /// File size in bytes
    pub size: u64,
    /// Whether the file has a supported extension for indexing
    pub supported: bool,
    /// Reason why file was skipped (only set when supported=false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<SkipReason>,
}

/// Parallel file scanner that respects .gitignore.
pub struct Scanner {
    root: PathBuf,
    /// Maximum file size in bytes (0 = unlimited).
    max_file_size_bytes: u64,
}

impl Scanner {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            max_file_size_bytes: 0,
        }
    }

    /// Create a scanner with a file size limit.
    pub fn with_max_file_size(root: impl Into<PathBuf>, max_size_mb: u32) -> Self {
        Self {
            root: root.into(),
            max_file_size_bytes: u64::from(max_size_mb) * BYTES_PER_MB,
        }
    }

    /// Scan the project directory in parallel, returning all indexable files.
    pub fn scan(&self) -> Result<Vec<ScannedFile>> {
        let entries = walk_supported_paths(&self.root);
        let root = &self.root;
        let max_size = self.max_file_size_bytes;
        let files: Vec<ScannedFile> = entries
            .par_iter()
            .filter_map(|path| {
                let meta = path.metadata().ok()?;
                let size = meta.len();

                // Skip files that exceed size limit
                if max_size > 0 && size > max_size {
                    return None;
                }

                // Nanosecond precision avoids the same-second false-negative
                // that second precision would have (edit → index → edit all
                // within one wall-clock second). Fall back to 0 (sentinel for
                // "unknown mtime") if the filesystem doesn't expose modified()
                // — staleness treats 0 as "always hash" to stay correct.
                let mtime_nanos = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
                    .unwrap_or(0);
                let hash = hasher::hash_file(path).ok()?;
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                Some(ScannedFile {
                    path: path.clone(),
                    relative_path: relative,
                    hash,
                    size,
                    extension: ext,
                    mtime_nanos,
                })
            })
            .collect();

        Ok(files)
    }

    /// Walk supported files without hashing.
    ///
    /// Returns `WalkedFile` entries containing path + size + mtime only.
    /// This is the cheap variant used by `staleness::detect_changes` to
    /// short-circuit SHA-256 on files whose on-disk mtime matches the
    /// per-file `files.mtime_nanos` stored by the indexer (i.e., the file
    /// hasn't been touched since it was last indexed).
    ///
    /// Same filtering rules as `scan()`: respects .gitignore, skips
    /// non-code directories, supported extensions only, `max_file_size_bytes`.
    pub fn walk(&self) -> Result<WalkResult> {
        let (entries, walk_had_errors) = walk_supported_paths_with_errors(&self.root);
        let root = &self.root;
        let max_size = self.max_file_size_bytes;

        let discovered: Vec<String> = entries.iter().map(|p| to_relative_path(p, root)).collect();
        let files: Vec<WalkedFile> = entries
            .into_par_iter()
            .filter_map(|path| build_walked_file(path, root, max_size))
            .collect();

        Ok(WalkResult {
            files,
            discovered,
            walk_had_errors,
        })
    }

    /// Scan ALL files in the project directory, including unsupported ones.
    ///
    /// Unlike `scan()`, this method does NOT filter by extension.
    /// Respects .gitignore and skips common non-code directories.
    /// Returns `DiscoveredFile` entries with `supported` flag indicating
    /// whether the file would be indexed.
    pub fn scan_all(&self) -> Result<Vec<DiscoveredFile>> {
        let entries: Vec<PathBuf> = WalkBuilder::new(&self.root)
            .hidden(true) // skip hidden dirs like .git
            .git_ignore(true)
            .git_global(false)
            .git_exclude(true)
            .follow_links(false) // Prevent symlink loops
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                // Skip common non-code directories
                !matches!(
                    name.as_ref(),
                    "node_modules"
                        | "target"
                        | ".rlm"
                        | ".git"
                        | "vendor"
                        | "dist"
                        | "build"
                        | "__pycache__"
                        | ".venv"
                        | "venv"
                )
            })
            .build()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
            .map(ignore::DirEntry::into_path)
            .collect();

        let root = &self.root;
        let max_size = self.max_file_size_bytes;
        let files: Vec<DiscoveredFile> = entries
            .par_iter()
            .filter_map(|path| {
                let meta = path.metadata().ok()?;
                let size = meta.len();
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/");
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                // Determine skip reason
                let (supported, skip_reason) = if max_size > 0 && size > max_size {
                    (false, Some(SkipReason::TooLarge))
                } else if !is_supported_extension(&ext) {
                    (false, Some(SkipReason::UnsupportedExtension))
                } else {
                    (true, None)
                };

                Some(DiscoveredFile {
                    path: relative,
                    extension: ext,
                    size,
                    supported,
                    reason: skip_reason,
                })
            })
            .collect();

        Ok(files)
    }
}

/// Walk the project and collect absolute paths of files with supported
/// extensions. Respects .gitignore and skips common non-code directories.
/// Shared filter used by `scan()` and `walk()` to avoid duplicating the
/// `WalkBuilder` configuration.
fn walk_supported_paths(root: &Path) -> Vec<PathBuf> {
    walk_supported_paths_with_errors(root).0
}

/// Same as `walk_supported_paths`, but also reports whether the walker hit
/// any errors (permission / IO on subdirectories). Staleness uses the flag
/// to stay safe against transient errors: an incomplete walk must not cause
/// indexed files to be wrongly classified as deleted.
fn walk_supported_paths_with_errors(root: &Path) -> (Vec<PathBuf>, bool) {
    let mut had_errors = false;
    let mut paths = Vec::new();
    for entry in WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .follow_links(false)
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !matches!(
                name.as_ref(),
                "node_modules"
                    | "target"
                    | ".rlm"
                    | ".git"
                    | "vendor"
                    | "dist"
                    | "build"
                    | "__pycache__"
                    | ".venv"
                    | "venv"
            )
        })
        .build()
    {
        match entry {
            Ok(e)
                if e.file_type().is_some_and(|ft| ft.is_file())
                    && e.path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(is_supported_extension) =>
            {
                paths.push(e.into_path());
            }
            Ok(_) => {} // dir or unsupported ext → skip
            Err(_) => had_errors = true,
        }
    }
    (paths, had_errors)
}

/// Build a `WalkedFile` for a single discovered path. Returns `None` only if
/// the metadata call itself fails or the file exceeds the size limit; missing
/// mtime falls back to 0 (sentinel) so staleness can still hash-verify the
/// file instead of treating it as deleted.
fn build_walked_file(path: PathBuf, root: &Path, max_size: u64) -> Option<WalkedFile> {
    let meta = path.metadata().ok()?;
    let size = meta.len();
    if max_size > 0 && size > max_size {
        return None;
    }
    // See scan() above: nanosecond precision so staleness' fast-path is safe
    // against same-second edits. 0 = unknown → always hash.
    let mtime_nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let relative = to_relative_path(&path, root);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    Some(WalkedFile {
        abs_path: path,
        relative_path: relative,
        extension,
        size,
        mtime_nanos,
    })
}

/// Convert an absolute path to a project-relative forward-slash path.
fn to_relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

// Re-export language mapping functions from the dedicated lang_map module.
pub use super::lang_map::{detect_ui_context, ext_to_lang, is_supported_extension};

#[cfg(test)]
#[path = "scanner_filter_tests.rs"]
mod filter_tests;
#[cfg(test)]
#[path = "scanner_tests.rs"]
mod tests;
