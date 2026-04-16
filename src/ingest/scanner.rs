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
}

/// Stat-only file metadata from `Scanner::walk` — no hash.
///
/// Used by `staleness::detect_changes` to skip SHA-256 on files whose mtime
/// is still older than the DB's `indexed_at`. Much cheaper than `ScannedFile`
/// for the clean-project common case.
#[derive(Debug, Clone)]
pub struct WalkedFile {
    pub abs_path: PathBuf,
    pub relative_path: String,
    pub extension: String,
    pub size: u64,
    /// Last-modified time, Unix seconds since epoch.
    pub mtime_secs: i64,
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
                })
            })
            .collect();

        Ok(files)
    }

    /// Walk supported files without hashing.
    ///
    /// Returns `WalkedFile` entries containing path + size + mtime only.
    /// This is the cheap variant used by `staleness::detect_changes` to
    /// short-circuit SHA-256 on files whose mtime matches the DB's
    /// `indexed_at` (i.e., clean since the last index).
    ///
    /// Same filtering rules as `scan()`: respects .gitignore, skips
    /// non-code directories, supported extensions only, `max_file_size_bytes`.
    pub fn walk(&self) -> Result<WalkResult> {
        let entries = walk_supported_paths(&self.root);
        let root = &self.root;
        let max_size = self.max_file_size_bytes;

        let discovered: Vec<String> = entries.iter().map(|p| to_relative_path(p, root)).collect();
        let files: Vec<WalkedFile> = entries
            .into_par_iter()
            .filter_map(|path| build_walked_file(path, root, max_size))
            .collect();

        Ok(WalkResult { files, discovered })
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
    WalkBuilder::new(root)
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
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_type().is_some_and(|ft| ft.is_file()))
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(is_supported_extension)
        })
        .map(ignore::DirEntry::into_path)
        .collect()
}

/// Build a `WalkedFile` for a single discovered path. Returns `None` if the
/// file exceeds the size limit or its metadata can't be read.
fn build_walked_file(path: PathBuf, root: &Path, max_size: u64) -> Option<WalkedFile> {
    let meta = path.metadata().ok()?;
    let size = meta.len();
    if max_size > 0 && size > max_size {
        return None;
    }
    let mtime_secs = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
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
        mtime_secs,
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
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// File size limit in bytes for the large-file-skip tests.
    const TEST_MAX_FILE_SIZE_BYTES: u64 = 1000;
    /// Content length used to create a file larger than `TEST_MAX_FILE_SIZE_BYTES`.
    const LARGE_FILE_CONTENT_LENGTH: usize = 2000;
    /// Expected max_file_size_bytes when constructing with 10 MB.
    const TEST_MAX_FILE_SIZE_MB: u32 = 10;
    const TEN_MB_IN_BYTES: u64 = TEST_MAX_FILE_SIZE_MB as u64 * BYTES_PER_MB;

    #[test]
    fn is_supported_extension_works() {
        assert!(is_supported_extension("rs"));
        assert!(is_supported_extension("py"));
        assert!(is_supported_extension("md"));
        assert!(!is_supported_extension("exe"));
        assert!(!is_supported_extension("png"));
    }

    #[test]
    fn ext_to_lang_maps_correctly() {
        assert_eq!(ext_to_lang("rs"), "rust");
        assert_eq!(ext_to_lang("py"), "python");
        assert_eq!(ext_to_lang("cs"), "csharp");
        assert_eq!(ext_to_lang("ts"), "typescript");
        assert_eq!(ext_to_lang("md"), "markdown");
        assert_eq!(ext_to_lang("xyz"), "unknown");
    }

    #[test]
    fn detect_ui_context_works() {
        assert_eq!(detect_ui_context("src/pages/Home.tsx"), Some("page".into()));
        assert_eq!(
            detect_ui_context("src/components/Button.tsx"),
            Some("component".into())
        );
        assert_eq!(
            detect_ui_context("src/screens/Login.tsx"),
            Some("screen".into())
        );
        assert_eq!(detect_ui_context("src/utils/helper.ts"), None);
        assert_eq!(detect_ui_context("src/App.tsx"), Some("ui".into()));
    }

    #[test]
    fn scanner_finds_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(tmp.path().join("ignore.exe"), "binary").unwrap();
        let scanner = Scanner::new(tmp.path());
        let files = scanner.scan().unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].extension, "rs");
    }

    #[test]
    fn scanner_skips_target_dir() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("target");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("lib.rs"), "// compiled").unwrap();
        fs::write(tmp.path().join("src.rs"), "fn main() {}").unwrap();
        let scanner = Scanner::new(tmp.path());
        let files = scanner.scan().unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].relative_path.contains("src.rs"));
    }

    #[test]
    fn scan_all_includes_unsupported() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main(){}").unwrap();
        fs::write(tmp.path().join("view.cshtml"), "@model X").unwrap();

        let scanner = Scanner::new(tmp.path());
        let files = scanner.scan_all().unwrap();

        assert_eq!(files.len(), 2);

        let rs = files.iter().find(|f| f.extension == "rs").unwrap();
        assert!(rs.supported);
        assert!(rs.reason.is_none());

        let cshtml = files.iter().find(|f| f.extension == "cshtml").unwrap();
        assert!(!cshtml.supported);
        assert_eq!(cshtml.reason, Some(SkipReason::UnsupportedExtension));
    }

    #[test]
    fn scan_all_respects_gitignore() {
        let tmp = TempDir::new().unwrap();
        // Create a minimal .git directory so ignore crate respects .gitignore
        fs::create_dir(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join(".gitignore"), "ignored.rs").unwrap();
        fs::write(tmp.path().join("main.rs"), "").unwrap();
        fs::write(tmp.path().join("ignored.rs"), "").unwrap();

        let scanner = Scanner::new(tmp.path());
        let files = scanner.scan_all().unwrap();

        // Should not include ignored.rs (respects .gitignore)
        assert!(!files.iter().any(|f| f.path.contains("ignored.rs")));
        // But should include main.rs
        assert!(files.iter().any(|f| f.path.contains("main.rs")));
    }

    #[test]
    fn scan_all_skips_rlm_dir() {
        let tmp = TempDir::new().unwrap();
        let rlm_dir = tmp.path().join(".rlm");
        fs::create_dir_all(&rlm_dir).unwrap();
        fs::write(rlm_dir.join("index.db"), "binary").unwrap();
        fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();

        let scanner = Scanner::new(tmp.path());
        let files = scanner.scan_all().unwrap();

        assert_eq!(files.len(), 1);
        assert!(files[0].path.contains("main.rs"));
    }

    #[test]
    fn scanner_skips_large_files() {
        let tmp = TempDir::new().unwrap();
        // Create a file larger than TEST_MAX_FILE_SIZE_BYTES
        let large_content = "x".repeat(LARGE_FILE_CONTENT_LENGTH);
        fs::write(tmp.path().join("large.rs"), &large_content).unwrap();
        fs::write(tmp.path().join("small.rs"), "fn main() {}").unwrap();

        let scanner = Scanner {
            root: tmp.path().to_path_buf(),
            max_file_size_bytes: TEST_MAX_FILE_SIZE_BYTES,
        };
        let files = scanner.scan().unwrap();

        // Only the small file should be included
        assert_eq!(files.len(), 1);
        assert!(files[0].relative_path.contains("small.rs"));
    }

    #[test]
    fn scan_all_reports_large_files() {
        let tmp = TempDir::new().unwrap();
        let large_content = "x".repeat(LARGE_FILE_CONTENT_LENGTH);
        fs::write(tmp.path().join("large.rs"), &large_content).unwrap();
        fs::write(tmp.path().join("small.rs"), "fn main() {}").unwrap();

        let scanner = Scanner {
            root: tmp.path().to_path_buf(),
            max_file_size_bytes: TEST_MAX_FILE_SIZE_BYTES,
        };
        let files = scanner.scan_all().unwrap();

        // Both files should be listed
        assert_eq!(files.len(), 2);

        let large = files.iter().find(|f| f.path.contains("large")).unwrap();
        assert!(!large.supported);
        assert_eq!(large.reason, Some(SkipReason::TooLarge));

        let small = files.iter().find(|f| f.path.contains("small")).unwrap();
        assert!(small.supported);
        assert!(small.reason.is_none());
    }

    #[test]
    fn with_max_file_size_constructor() {
        let tmp = TempDir::new().unwrap();
        let scanner = Scanner::with_max_file_size(tmp.path(), TEST_MAX_FILE_SIZE_MB);
        assert_eq!(scanner.max_file_size_bytes, TEN_MB_IN_BYTES);
    }
}
