use std::path::PathBuf;

use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::Serialize;

use crate::error::Result;
use crate::ingest::hasher;

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

/// A discovered file (may or may not be indexable).
/// Used by `rlm files` to show ALL files in the project.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredFile {
    /// Relative path from project root (forward slashes)
    #[serde(rename = "p")]
    pub relative_path: String,
    /// File extension (lowercase, without dot)
    #[serde(rename = "x")]
    pub extension: String,
    /// File size in bytes
    #[serde(rename = "sz")]
    pub size: u64,
    /// Whether the file has a supported extension for indexing
    #[serde(rename = "i")]
    pub supported: bool,
    /// Reason why file was skipped (only set when supported=false)
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<SkipReason>,
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
            max_file_size_bytes: u64::from(max_size_mb) * 1024 * 1024,
        }
    }

    /// Scan the project directory in parallel, returning all indexable files.
    pub fn scan(&self) -> Result<Vec<ScannedFile>> {
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
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(is_supported_extension)
            })
            .map(ignore::DirEntry::into_path)
            .collect();

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
                    relative_path: relative,
                    extension: ext,
                    size,
                    supported,
                    skip_reason,
                })
            })
            .collect();

        Ok(files)
    }
}

/// Check if a file extension is supported for indexing.
#[must_use]
pub fn is_supported_extension(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "rs" | "go"
            | "java"
            | "cs"
            | "py"
            | "php"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "md"
            | "markdown"
            | "pdf"
            | "json"
            | "html"
            | "css"
            | "sh"
            | "bash"
            | "yaml"
            | "yml"
            | "toml"
            | "sql"
            // C#/.NET ecosystem
            | "xml"
            | "csproj"
            | "fsproj"
            | "vbproj"
            | "sln"
            | "props"
            | "targets"
            // Java/Kotlin ecosystem
            | "gradle"
            | "kts"
            | "properties"
            // Python/config ecosystem
            | "pyi"
            | "cfg"
            | "ini"
            // Schema/IDL
            | "proto"
            | "graphql"
            | "gql"
            // Text documentation
            | "txt"
            | "rst"
            // Infrastructure as Code
            | "tf"
            | "hcl"
    )
}

/// Map file extension to language identifier.
#[must_use]
pub fn ext_to_lang(ext: &str) -> &str {
    match ext.to_lowercase().as_str() {
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "cs" => "csharp",
        "py" => "python",
        "php" => "php",
        "js" | "jsx" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "cxx" => "cpp",
        "md" | "markdown" => "markdown",
        "pdf" => "pdf",
        "json" => "json",
        "html" | "htm" => "html",
        "css" => "css",
        "sh" | "bash" => "bash",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sql" => "sql",
        // C#/.NET ecosystem (XML-based project files)
        "xml" | "csproj" | "fsproj" | "vbproj" | "props" | "targets" => "xml",
        "sln" => "plaintext",
        // Java/Kotlin ecosystem
        "gradle" => "plaintext",
        "kts" => "plaintext",
        "properties" => "plaintext",
        // Python type stubs (valid Python syntax)
        "pyi" => "python",
        // Config files
        "cfg" | "ini" => "plaintext",
        // Schema/IDL
        "proto" => "plaintext",
        "graphql" | "gql" => "plaintext",
        // Text documentation
        "txt" | "rst" => "plaintext",
        // Infrastructure as Code
        "tf" | "hcl" => "plaintext",
        _ => "unknown",
    }
}

/// Detect UI context from file path.
#[must_use]
pub fn detect_ui_context(path: &str) -> Option<String> {
    let lower = path.to_lowercase();
    if lower.contains("/pages/") || lower.contains("/app/") {
        Some("page".into())
    } else if lower.contains("/components/") {
        Some("component".into())
    } else if lower.contains("/screens/") {
        Some("screen".into())
    } else if lower.contains("/layouts/") {
        Some("layout".into())
    } else if lower.ends_with(".tsx") || lower.ends_with(".jsx") || lower.ends_with(".vue") {
        Some("ui".into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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
        assert!(rs.skip_reason.is_none());

        let cshtml = files.iter().find(|f| f.extension == "cshtml").unwrap();
        assert!(!cshtml.supported);
        assert_eq!(cshtml.skip_reason, Some(SkipReason::UnsupportedExtension));
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
        assert!(!files.iter().any(|f| f.relative_path.contains("ignored.rs")));
        // But should include main.rs
        assert!(files.iter().any(|f| f.relative_path.contains("main.rs")));
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
        assert!(files[0].relative_path.contains("main.rs"));
    }

    #[test]
    fn scanner_skips_large_files() {
        let tmp = TempDir::new().unwrap();
        // Create a file larger than 1KB
        let large_content = "x".repeat(2000);
        fs::write(tmp.path().join("large.rs"), &large_content).unwrap();
        fs::write(tmp.path().join("small.rs"), "fn main() {}").unwrap();

        // Scanner with 1KB limit (1KB = 1/1024 MB, but we use MB so set to 0 for bytes test)
        // Actually we need to use with_max_file_size which takes MB
        // Let's create a scanner that only accepts files < 1KB
        let scanner = Scanner {
            root: tmp.path().to_path_buf(),
            max_file_size_bytes: 1000, // 1KB limit for test
        };
        let files = scanner.scan().unwrap();

        // Only the small file should be included
        assert_eq!(files.len(), 1);
        assert!(files[0].relative_path.contains("small.rs"));
    }

    #[test]
    fn scan_all_reports_large_files() {
        let tmp = TempDir::new().unwrap();
        let large_content = "x".repeat(2000);
        fs::write(tmp.path().join("large.rs"), &large_content).unwrap();
        fs::write(tmp.path().join("small.rs"), "fn main() {}").unwrap();

        let scanner = Scanner {
            root: tmp.path().to_path_buf(),
            max_file_size_bytes: 1000,
        };
        let files = scanner.scan_all().unwrap();

        // Both files should be listed
        assert_eq!(files.len(), 2);

        let large = files
            .iter()
            .find(|f| f.relative_path.contains("large"))
            .unwrap();
        assert!(!large.supported);
        assert_eq!(large.skip_reason, Some(SkipReason::TooLarge));

        let small = files
            .iter()
            .find(|f| f.relative_path.contains("small"))
            .unwrap();
        assert!(small.supported);
        assert!(small.skip_reason.is_none());
    }

    #[test]
    fn with_max_file_size_constructor() {
        let tmp = TempDir::new().unwrap();
        let scanner = Scanner::with_max_file_size(tmp.path(), 10);
        assert_eq!(scanner.max_file_size_bytes, 10 * 1024 * 1024);
    }
}
