use thiserror::Error;

use crate::edit::error::EditError;
use crate::setup::SetupError;

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
}

pub type Result<T> = std::result::Result<T, RlmError>;

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
