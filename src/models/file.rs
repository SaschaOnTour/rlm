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
    /// File's own mtime at index time, Unix seconds. Used by staleness
    /// detection to skip hashing files whose mtime is unchanged since last
    /// index. Defaults to 0 for records created without a known mtime.
    pub mtime_secs: i64,
}

impl FileRecord {
    /// Create a record with `mtime_secs = 0`. Convenient for tests that don't
    /// exercise staleness; production indexing code should use `with_mtime`.
    #[must_use]
    pub fn new(path: String, hash: String, lang: String, size_bytes: u64) -> Self {
        Self::with_mtime(path, hash, lang, size_bytes, 0)
    }

    /// Create a record with an explicit file-mtime. Used by the indexer so the
    /// staleness detector can compare on-disk mtime against this value.
    #[must_use]
    pub fn with_mtime(
        path: String,
        hash: String,
        lang: String,
        size_bytes: u64,
        mtime_secs: i64,
    ) -> Self {
        Self {
            id: 0,
            path,
            hash,
            lang,
            size_bytes,
            mtime_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_record_new_sets_zero_id() {
        const FILE_SIZE: u64 = 1024;

        let f = FileRecord::new(
            "src/main.rs".into(),
            "abc123".into(),
            "rust".into(),
            FILE_SIZE,
        );
        assert_eq!(f.id, 0);
        assert_eq!(f.path, "src/main.rs");
        assert_eq!(f.lang, "rust");
        assert_eq!(f.size_bytes, FILE_SIZE);
        assert_eq!(f.mtime_secs, 0);
    }

    #[test]
    fn file_record_with_mtime_sets_field() {
        const FILE_SIZE: u64 = 2048;
        const SAMPLE_MTIME: i64 = 1_700_000_000;

        let f = FileRecord::with_mtime(
            "src/lib.rs".into(),
            "def456".into(),
            "rust".into(),
            FILE_SIZE,
            SAMPLE_MTIME,
        );
        assert_eq!(f.mtime_secs, SAMPLE_MTIME);
        assert_eq!(f.size_bytes, FILE_SIZE);
    }
}
