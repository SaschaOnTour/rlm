use serde::Serialize;

/// A file record stored in the index database.
#[derive(Debug, Clone, Serialize)]
pub struct FileRecord {
    /// Database row ID (0 if not yet persisted).
    pub id: i64,
    /// Project-relative file path (forward slashes).
    pub path: String,
    /// SHA-256 hash of file contents.
    pub hash: String,
    /// Detected language identifier.
    pub lang: String,
    /// File size in bytes.
    pub size_bytes: u64,
}

impl FileRecord {
    #[must_use]
    pub fn new(path: String, hash: String, lang: String, size_bytes: u64) -> Self {
        Self {
            id: 0,
            path,
            hash,
            lang,
            size_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_record_new_sets_zero_id() {
        let f = FileRecord::new("src/main.rs".into(), "abc123".into(), "rust".into(), 1024);
        assert_eq!(f.id, 0);
        assert_eq!(f.path, "src/main.rs");
        assert_eq!(f.lang, "rust");
        assert_eq!(f.size_bytes, 1024);
    }
}
