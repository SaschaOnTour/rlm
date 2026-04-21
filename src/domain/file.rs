/// A file record stored in the index database.
#[derive(Debug, Clone)]
pub struct FileRecord {
    /// Database row ID (0 if not yet persisted).
    pub id: i64,
    /// Project-relative file path (forward slashes).
    pub path: String,
    /// Hex-encoded SHA-256 digest of file contents.
    pub hash: String,
    /// Detected language identifier.
    pub lang: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// File's own mtime at index time, in nanoseconds since the Unix epoch.
    /// Used by staleness detection to skip hashing files whose mtime is
    /// unchanged since last index. Nanosecond precision prevents same-second
    /// false negatives on modern filesystems. Defaults to 0 (sentinel for
    /// "unknown / legacy row", which forces a hash verification).
    pub mtime_nanos: i64,
}

impl FileRecord {
    /// Create a record with `mtime_nanos = 0`. Convenient for tests that don't
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
        mtime_nanos: i64,
    ) -> Self {
        Self {
            id: 0,
            path,
            hash,
            lang,
            size_bytes,
            mtime_nanos,
        }
    }
}

#[cfg(test)]
#[path = "file_tests.rs"]
mod tests;
