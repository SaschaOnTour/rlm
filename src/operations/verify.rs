//! Verify operations shared between CLI and MCP.
//!
//! Provides consistent behavior for verifying index integrity with proper
//! error handling (no silent failures).

use std::path::Path;

use serde::Serialize;

use crate::db::queries::VerifyReport;
use crate::db::Database;
use crate::error::Result;

/// Result of fixing integrity issues.
#[derive(Debug, Clone, Serialize)]
pub struct FixResult {
    /// Whether fixes were applied.
    pub fixed: bool,
    /// Number of orphan chunks deleted.
    pub orphan_chunks_deleted: u64,
    /// Number of orphan refs deleted.
    pub orphan_refs_deleted: u64,
    /// Number of missing files removed from index.
    pub missing_files_removed: u64,
}

/// Verify index integrity and check for missing files on disk.
///
/// Checks:
/// - `SQLite` integrity
/// - Orphan chunks (`file_id` points to deleted file)
/// - Orphan refs (`chunk_id` points to deleted chunk)
/// - Files in index that no longer exist on disk
pub fn verify_index(db: &Database, project_root: &Path) -> Result<VerifyReport> {
    let mut report = db.verify_integrity()?;

    // Check for files that no longer exist on disk
    let indexed_paths = db.get_all_file_paths()?;
    for path in &indexed_paths {
        let full_path = project_root.join(path);
        if !full_path.exists() {
            report.missing_files += 1;
            report.missing_file_paths.push(path.clone());
        }
    }

    Ok(report)
}

/// Fix integrity issues by deleting orphans and removing missing files.
///
/// Unlike the MCP version that used `unwrap_or(false)` which silently ignored errors,
/// this function properly propagates errors.
pub fn fix_integrity(db: &Database, report: &VerifyReport) -> Result<FixResult> {
    // Fix orphans (refs first, then chunks)
    let (chunks_fixed, refs_fixed) = db.fix_orphans()?;

    // Remove missing files from index - propagate errors instead of ignoring them
    let mut files_removed = 0u64;
    for path in &report.missing_file_paths {
        if db.delete_file_by_path(path)? {
            files_removed += 1;
        }
    }

    Ok(FixResult {
        fixed: true,
        orphan_chunks_deleted: chunks_fixed,
        orphan_refs_deleted: refs_fixed,
        missing_files_removed: files_removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;
    use rusqlite::params;
    use tempfile::TempDir;

    fn setup_test_db_and_dir() -> (Database, TempDir) {
        let db = Database::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        (db, tmp)
    }

    #[test]
    fn verify_clean_index() {
        let (db, tmp) = setup_test_db_and_dir();

        // Create file on disk and index it
        let file_path = tmp.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();

        let file = FileRecord::new("test.rs".into(), "hash".into(), "rust".into(), 100);
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 1,
            start_byte: 0,
            end_byte: 12,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() {}".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        let report = verify_index(&db, tmp.path()).unwrap();
        assert!(report.is_ok());
        assert!(report.sqlite_ok);
        assert_eq!(report.orphan_chunks, 0);
        assert_eq!(report.orphan_refs, 0);
        assert_eq!(report.missing_files, 0);
    }

    #[test]
    fn verify_detects_missing_file() {
        let (db, tmp) = setup_test_db_and_dir();

        // Index a file but don't create it on disk
        let file = FileRecord::new("missing.rs".into(), "hash".into(), "rust".into(), 100);
        db.upsert_file(&file).unwrap();

        let report = verify_index(&db, tmp.path()).unwrap();
        assert!(!report.is_ok());
        assert_eq!(report.missing_files, 1);
        assert!(report
            .missing_file_paths
            .contains(&"missing.rs".to_string()));
    }

    #[test]
    fn fix_removes_missing_files() {
        let (db, tmp) = setup_test_db_and_dir();

        // Index a file that doesn't exist on disk
        let file = FileRecord::new("missing.rs".into(), "hash".into(), "rust".into(), 100);
        db.upsert_file(&file).unwrap();

        let report = verify_index(&db, tmp.path()).unwrap();
        let fix_result = fix_integrity(&db, &report).unwrap();

        assert!(fix_result.fixed);
        assert_eq!(fix_result.missing_files_removed, 1);

        // Verify it's actually removed
        let files = db.get_all_files().unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn verify_and_fix_orphan_chunks() {
        let db = Database::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();

        // Create two files in the index
        let file1 = FileRecord::new("test.rs".into(), "hash".into(), "rust".into(), 100);
        let file1_id = db.upsert_file(&file1).unwrap();

        let file2 = FileRecord::new("other.rs".into(), "hash2".into(), "rust".into(), 100);
        let file2_id = db.upsert_file(&file2).unwrap();

        // Create a chunk for file2
        let chunk = Chunk {
            id: 0,
            file_id: file2_id,
            start_line: 1,
            end_line: 1,
            start_byte: 0,
            end_byte: 10,
            kind: ChunkKind::Function,
            ident: "orphan_soon".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn orphan()".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        // Create both files on disk
        std::fs::write(tmp.path().join("test.rs"), "").unwrap();
        std::fs::write(tmp.path().join("other.rs"), "").unwrap();

        // Delete file2 directly (bypassing cascade to create orphan)
        // This simulates a corrupted index where FK constraint wasn't enforced
        db.conn().execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        db.conn()
            .execute("DELETE FROM files WHERE id = ?1", params![file2_id])
            .unwrap();
        db.conn().execute("PRAGMA foreign_keys = ON;", []).unwrap();

        // Now we have an orphan chunk
        let report = verify_index(&db, tmp.path()).unwrap();
        assert_eq!(report.orphan_chunks, 1);

        let fix_result = fix_integrity(&db, &report).unwrap();
        assert_eq!(fix_result.orphan_chunks_deleted, 1);

        // Verify chunk is gone
        let all_chunks = db.get_all_chunks().unwrap();
        assert!(all_chunks.is_empty());
    }

    #[test]
    fn verify_and_fix_orphan_refs() {
        let db = Database::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();

        // Create a file and chunk
        let file = FileRecord::new("test.rs".into(), "hash".into(), "rust".into(), 100);
        let file_id = db.upsert_file(&file).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: 1,
            start_byte: 0,
            end_byte: 10,
            kind: ChunkKind::Function,
            ident: "test".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn test()".into(),
        };
        let chunk_id = db.insert_chunk(&chunk).unwrap();

        // Create a valid ref
        let valid_ref = Reference {
            id: 0,
            chunk_id,
            target_ident: "foo".into(),
            ref_kind: RefKind::Call,
            line: 1,
            col: 1,
        };
        db.insert_ref(&valid_ref).unwrap();

        // Create the file on disk
        std::fs::write(tmp.path().join("test.rs"), "").unwrap();

        // Delete the chunk directly (bypassing cascade to create orphan ref)
        db.conn().execute("PRAGMA foreign_keys = OFF;", []).unwrap();
        db.conn()
            .execute("DELETE FROM chunks WHERE id = ?1", params![chunk_id])
            .unwrap();
        db.conn().execute("PRAGMA foreign_keys = ON;", []).unwrap();

        // Now we have an orphan ref
        let report = verify_index(&db, tmp.path()).unwrap();
        assert_eq!(report.orphan_refs, 1);

        let fix_result = fix_integrity(&db, &report).unwrap();
        assert_eq!(fix_result.orphan_refs_deleted, 1);
    }
}
