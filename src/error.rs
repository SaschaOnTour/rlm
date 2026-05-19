use thiserror::Error;

/// Failures that can occur while applying a code edit.
#[derive(Error, Debug)]
pub enum EditError {
    /// The target line is beyond the end of the file.
    #[error("line {line} is beyond file length ({max})")]
    LineOutOfBounds { line: usize, max: usize },
}

/// Errors raised by atomic-write primitives.
#[derive(Error, Debug)]
pub enum AtomicWriteError {
    /// Any underlying filesystem error (directory creation, temp open,
    /// write, or rename).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The retry budget was exhausted without finding a free temp
    /// filename. Only plausible under extreme contention or clock skew.
    #[error("atomic write exhausted {attempts} temp-name attempts")]
    Exhausted { attempts: u32 },
}

/// Failures specific to `rlm setup`.
#[derive(Error, Debug)]
pub enum SetupError {
    /// The existing settings file is valid JSON but not an object; we refuse
    /// to overwrite user content of unknown shape.
    #[error("{path} is not a JSON object — rlm refuses to overwrite it. Remove or replace the file before re-running setup.")]
    NotJsonObject { path: String },

    /// The existing settings file is not parseable JSON.
    #[error("{path} is not valid JSON ({source}) — rlm refuses to overwrite it. Fix the file before re-running setup.")]
    InvalidJson {
        path: String,
        source: serde_json::Error,
    },
}

#[derive(Error, Debug)]
pub enum RlmError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("index not found: project must be indexed first")]
    IndexNotFound,

    #[error(transparent)]
    IndexAutoCreateDisabled(IndexAutoCreateDisabledError),

    #[error("file not found: {path}")]
    FileNotFound { path: String },

    #[error("symbol not found: {ident}")]
    SymbolNotFound { ident: String },

    #[error(transparent)]
    AmbiguousSymbol(AmbiguousSymbolError),

    #[error(transparent)]
    SectionNotFound(SectionNotFoundError),

    #[error("parse error in {path}: {detail}")]
    Parse { path: String, detail: String },

    #[error("syntax guard rejected: {detail}")]
    SyntaxGuard { detail: String },

    #[error("unsupported language: {ext}")]
    UnsupportedLanguage { ext: String },

    #[error("no parent container found for insertion")]
    NoContainer,

    #[error("edit conflict: file changed on disk")]
    EditConflict,

    #[error("config error: {0}")]
    Config(String),

    #[error("path traversal rejected: {path}")]
    PathTraversal { path: String },

    #[error("invalid pattern {pattern:?}: {reason}")]
    InvalidPattern { pattern: String, reason: String },

    #[error("mcp server error: {0}")]
    Mcp(String),

    #[error(transparent)]
    Setup(#[from] SetupError),

    #[error(transparent)]
    Edit(#[from] EditError),

    #[error(transparent)]
    AtomicWrite(#[from] AtomicWriteError),
}

pub type Result<T> = std::result::Result<T, RlmError>;

/// Maximum number of section headings surfaced in a [`SectionNotFoundError`] hint.
pub const MAX_SECTION_HINT: usize = 10;

/// Section-lookup failure with a hint for the agent. Wrapped by
/// [`RlmError::SectionNotFound`]; the `Display` impl renders the
/// "available sections" hint.
#[derive(Debug, Clone)]
pub struct SectionNotFoundError {
    pub heading: String,
    pub available: Vec<String>,
    pub total: usize,
}

impl std::fmt::Display for SectionNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.available.is_empty() {
            return write!(
                f,
                "section not found: {}. File has no sections.",
                self.heading
            );
        }
        if self.total > self.available.len() {
            write!(
                f,
                "section not found: {}. Available ({} total, first {MAX_SECTION_HINT}): {}",
                self.heading,
                self.total,
                self.available.join(", ")
            )
        } else {
            write!(
                f,
                "section not found: {}. Available: {}",
                self.heading,
                self.available.join(", ")
            )
        }
    }
}

impl std::error::Error for SectionNotFoundError {}

/// Validate that a relative path is safe to join with a project root.
///
/// Rejects absolute paths, `..` components, prefix/root components (Windows drive letters),
/// and paths that escape the project root via symlinks. Canonicalization failures on
/// `project_root` propagate as I/O errors; failures on the target path are treated as
/// path traversal rejections.
pub fn validate_relative_path(
    rel_path: &str,
    project_root: &std::path::Path,
) -> Result<std::path::PathBuf> {
    use std::path::Component;

    let rel = std::path::Path::new(rel_path);

    // Reject absolute paths
    if rel.is_absolute() {
        return Err(RlmError::PathTraversal {
            path: rel_path.into(),
        });
    }

    // Reject .., prefix (Windows drive), and root components
    for component in rel.components() {
        match component {
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                return Err(RlmError::PathTraversal {
                    path: rel_path.into(),
                });
            }
            _ => {}
        }
    }

    let full_path = project_root.join(rel_path);
    let canonical_root = project_root.canonicalize()?;
    verify_containment(&full_path, &canonical_root, rel_path)?;

    // Return canonical path (existing files) to minimize TOCTOU gap.
    // For new files, join under the validated canonical root.
    if full_path.exists() {
        Ok(full_path.canonicalize()?)
    } else {
        Ok(canonical_root.join(rel_path))
    }
}

/// Verify that `full_path` resolves to a location under `canonical_root`.
///
/// For paths that do not exist yet, resolves the nearest existing ancestor
/// so symlink escapes through existing path components are still detected.
fn verify_containment(
    full_path: &std::path::Path,
    canonical_root: &std::path::Path,
    rel_path: &str,
) -> Result<()> {
    let mut existing_ancestor = full_path;
    while !existing_ancestor.exists() {
        existing_ancestor = existing_ancestor
            .parent()
            .ok_or_else(|| RlmError::PathTraversal {
                path: rel_path.into(),
            })?;
    }

    let canonical_existing =
        existing_ancestor
            .canonicalize()
            .map_err(|_| RlmError::PathTraversal {
                path: rel_path.into(),
            })?;

    if !canonical_existing.starts_with(canonical_root) {
        return Err(RlmError::PathTraversal {
            path: rel_path.into(),
        });
    }
    Ok(())
}

/// One entry in [`AmbiguousSymbolError`]'s candidate list. Captures
/// just enough to disambiguate at the CLI / agent layer (parent
/// container name, chunk kind, line number).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolCandidate {
    pub parent: Option<String>,
    pub kind: String,
    pub line: u32,
}

/// Ambiguous-symbol resolution error. Wrapped by
/// [`RlmError::AmbiguousSymbol`]; lives as its own type so `Display`
/// can render the candidate list without a free helper (which
/// rustqual's static analysis flagged as dead code).
#[derive(Debug, Clone)]
pub struct AmbiguousSymbolError {
    pub ident: String,
    pub candidates: Vec<SymbolCandidate>,
}

impl std::fmt::Display for AmbiguousSymbolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ambiguous symbol '{ident}': {n} candidates — ",
            ident = self.ident,
            n = self.candidates.len()
        )?;
        for (i, c) in self.candidates.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            if let Some(p) = &c.parent {
                write!(f, "{p}::")?;
            }
            write!(f, "{kind} (line {line})", kind = c.kind, line = c.line)?;
        }
        write!(f, ". Specify --parent <name>.")
    }
}

impl std::error::Error for AmbiguousSymbolError {}

/// Structured failure when `ensure_index` would create a new
/// `.rlm/index.db` but the project's `[indexing] auto_create_index =
/// false` setting forbids it. The `#[error(...)]` template renders
/// the actionable hint adapters surface to the user.
#[derive(Debug, Clone, Error)]
#[error(
    "no index at {db_path}: run `rlm index {project_root}` first \
     (auto_create_index is disabled in .rlm/config.toml)",
    db_path = .db_path.display(),
    project_root = .project_root.display(),
)]
pub struct IndexAutoCreateDisabledError {
    /// Absolute path the index would have been created at.
    pub db_path: std::path::PathBuf,
    /// Project root the user invoked the command from.
    pub project_root: std::path::PathBuf,
}
