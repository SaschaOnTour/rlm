use thiserror::Error;

#[derive(Error, Debug)]
pub enum RlmError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("index not found: project must be indexed first")]
    IndexNotFound,

    #[error("file not found: {path}")]
    FileNotFound { path: String },

    #[error("symbol not found: {ident}")]
    SymbolNotFound { ident: String },

    #[error("section not found: {heading}")]
    SectionNotFound { heading: String },

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

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("path traversal rejected: {path}")]
    PathTraversal { path: String },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, RlmError>;

/// Validate that a relative path is safe to join with a project root.
///
/// Rejects absolute paths, `..` components, and paths that escape the project root.
pub fn validate_relative_path(
    rel_path: &str,
    project_root: &std::path::Path,
) -> Result<std::path::PathBuf> {
    let rel = std::path::Path::new(rel_path);

    // Reject absolute paths
    if rel.is_absolute() {
        return Err(RlmError::PathTraversal {
            path: rel_path.into(),
        });
    }

    // Reject .. components
    for component in rel.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(RlmError::PathTraversal {
                path: rel_path.into(),
            });
        }
    }

    // Canonicalize and verify the resolved path is under project_root
    let full_path = project_root.join(rel_path);
    if let (Ok(canonical_root), Ok(canonical_full)) =
        (project_root.canonicalize(), full_path.canonicalize())
    {
        if !canonical_full.starts_with(&canonical_root) {
            return Err(RlmError::PathTraversal {
                path: rel_path.into(),
            });
        }
    }

    Ok(full_path)
}
