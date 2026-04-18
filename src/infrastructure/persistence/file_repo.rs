//! Storage operations for `FileRecord`.

use crate::db::queries::IndexedFileMeta;
use crate::db::Database;
use crate::error::Result;
use crate::models::file::FileRecord;

/// Read and write access to indexed files.
pub trait FileRepo {
    fn upsert_file(&self, file: &FileRecord) -> Result<i64>;
    fn get_file_by_path(&self, path: &str) -> Result<Option<FileRecord>>;
    fn get_all_files(&self) -> Result<Vec<FileRecord>>;
    fn get_all_file_paths(&self) -> Result<Vec<String>>;
    fn get_indexed_files_meta(&self) -> Result<Vec<IndexedFileMeta>>;
    fn update_file_mtime(&self, file_id: i64, mtime_nanos: i64) -> Result<()>;
    fn delete_file(&self, file_id: i64) -> Result<()>;
    fn delete_file_by_path(&self, path: &str) -> Result<bool>;
    fn set_file_parse_quality(&self, file_id: i64, quality: &str) -> Result<()>;
    fn get_files_with_quality_issues(&self) -> Result<Vec<(String, String)>>;
}

impl FileRepo for Database {
    fn upsert_file(&self, file: &FileRecord) -> Result<i64> {
        Database::upsert_file(self, file)
    }

    fn get_file_by_path(&self, path: &str) -> Result<Option<FileRecord>> {
        Database::get_file_by_path(self, path)
    }

    fn get_all_files(&self) -> Result<Vec<FileRecord>> {
        Database::get_all_files(self)
    }

    fn get_all_file_paths(&self) -> Result<Vec<String>> {
        Database::get_all_file_paths(self)
    }

    fn get_indexed_files_meta(&self) -> Result<Vec<IndexedFileMeta>> {
        Database::get_indexed_files_meta(self)
    }

    fn update_file_mtime(&self, file_id: i64, mtime_nanos: i64) -> Result<()> {
        Database::update_file_mtime(self, file_id, mtime_nanos)
    }

    fn delete_file(&self, file_id: i64) -> Result<()> {
        Database::delete_file(self, file_id)
    }

    fn delete_file_by_path(&self, path: &str) -> Result<bool> {
        Database::delete_file_by_path(self, path)
    }

    fn set_file_parse_quality(&self, file_id: i64, quality: &str) -> Result<()> {
        Database::set_file_parse_quality(self, file_id, quality)
    }

    fn get_files_with_quality_issues(&self) -> Result<Vec<(String, String)>> {
        Database::get_files_with_quality_issues(self)
    }
}
