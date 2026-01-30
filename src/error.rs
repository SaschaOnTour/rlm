use thiserror::Error;

#[derive(Error, Debug)]
pub enum RlmError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("index not found: run `rlm index` first")]
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

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, RlmError>;
