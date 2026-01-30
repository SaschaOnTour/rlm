//! Stats operations shared between CLI and MCP.
//!
//! Provides consistent behavior for getting index statistics including quality info.

use serde::Serialize;

use crate::db::Database;
use crate::error::Result;

/// Result of getting index statistics.
#[derive(Debug, Clone, Serialize)]
pub struct StatsResult {
    /// Number of indexed files.
    pub files: u64,
    /// Number of chunks.
    pub chunks: u64,
    /// Number of references.
    pub refs: u64,
    /// Total bytes of indexed files.
    pub total_bytes: u64,
    /// Language breakdown (language, count).
    pub languages: Vec<(String, i64)>,
    /// Oldest indexed timestamp (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_indexed: Option<String>,
    /// Newest indexed timestamp (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest_indexed: Option<String>,
}

/// Quality information for files.
#[derive(Debug, Clone, Serialize)]
pub struct QualityInfo {
    /// Number of files with parse warnings.
    pub files_with_parse_warnings: usize,
    /// Details of files with quality issues.
    pub files: Vec<QualityFileInfo>,
}

/// Quality info for a single file.
#[derive(Debug, Clone, Serialize)]
pub struct QualityFileInfo {
    /// The file path.
    #[serde(rename = "p")]
    pub path: String,
    /// The quality status.
    #[serde(rename = "q")]
    pub quality: String,
}

/// Get index statistics.
pub fn get_stats(db: &Database) -> Result<StatsResult> {
    let stats = db.stats()?;

    Ok(StatsResult {
        files: stats.file_count,
        chunks: stats.chunk_count,
        refs: stats.ref_count,
        total_bytes: stats.total_bytes,
        languages: stats.languages,
        oldest_indexed: stats.oldest_indexed,
        newest_indexed: stats.newest_indexed,
    })
}

/// Get quality information for files with parse issues.
pub fn get_quality_info(db: &Database) -> Result<Option<QualityInfo>> {
    let quality_issues = db.get_files_with_quality_issues()?;

    if quality_issues.is_empty() {
        return Ok(None);
    }

    let files: Vec<QualityFileInfo> = quality_issues
        .into_iter()
        .map(|(path, quality)| QualityFileInfo { path, quality })
        .collect();

    Ok(Some(QualityInfo {
        files_with_parse_warnings: files.len(),
        files,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn get_stats_basic() {
        let db = test_db();

        let file = FileRecord::new("src/lib.rs".into(), "hash".into(), "rust".into(), 1024);
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
            kind: ChunkKind::Function,
            ident: "test".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn test() {}".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        let result = get_stats(&db).unwrap();
        assert_eq!(result.files, 1);
        assert_eq!(result.chunks, 1);
        assert_eq!(result.total_bytes, 1024);
    }

    #[test]
    fn get_quality_info_empty() {
        let db = test_db();
        let result = get_quality_info(&db).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_quality_info_with_issues() {
        let db = test_db();

        let file = FileRecord::new("src/lib.rs".into(), "hash".into(), "rust".into(), 100);
        let file_id = db.upsert_file(&file).unwrap();
        db.set_file_parse_quality(file_id, "partial").unwrap();

        let result = get_quality_info(&db).unwrap();
        assert!(result.is_some());
        let info = result.unwrap();
        assert_eq!(info.files_with_parse_warnings, 1);
        assert_eq!(info.files[0].path, "src/lib.rs");
        assert_eq!(info.files[0].quality, "partial");
    }
}
