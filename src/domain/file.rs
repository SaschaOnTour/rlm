//! File entity — a source file known to the index.

use serde::{Deserialize, Serialize};

use super::FileId;

/// A file known to the index.
///
/// Fields beyond `id` and `path` reflect what the indexer captures to drive
/// staleness detection and language dispatch. They are carried here rather
/// than split off into separate entities because every consumer that holds
/// a `File` today needs all of them; a split would only add indirection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub id: FileId,
    /// Project-relative path, forward slashes.
    pub path: String,
    /// Hex-encoded SHA-256 of file contents at index time.
    pub hash: String,
    /// Language identifier (e.g. `"rust"`, `"markdown"`).
    pub lang: String,
    pub size_bytes: u64,
    /// File mtime at index time, nanoseconds since the Unix epoch. Used by
    /// staleness detection to skip hashing unchanged files. `0` means
    /// "unknown / legacy row"; callers treat that as a forced re-hash.
    pub mtime_nanos: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_constructs_with_expected_fields() {
        let f = File {
            id: FileId::UNPERSISTED,
            path: "src/main.rs".into(),
            hash: "abc123".into(),
            lang: "rust".into(),
            size_bytes: 1024,
            mtime_nanos: 1_700_000_000,
        };
        assert_eq!(f.path, "src/main.rs");
        assert_eq!(f.lang, "rust");
        assert_eq!(f.size_bytes, 1024);
        assert_eq!(f.mtime_nanos, 1_700_000_000);
        assert!(!f.id.is_persisted());
    }

    #[test]
    fn file_serializes_round_trip() {
        let original = File {
            id: FileId::new(7),
            path: "docs/readme.md".into(),
            hash: "deadbeef".into(),
            lang: "markdown".into(),
            size_bytes: 4096,
            mtime_nanos: 0,
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: File = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, original.id);
        assert_eq!(back.path, original.path);
        assert_eq!(back.mtime_nanos, original.mtime_nanos);
    }
}
