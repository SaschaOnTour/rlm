use std::collections::HashSet;

use crate::config::Config;
use crate::db::Database;
use crate::error::Result;
use crate::ingest::dispatcher::Dispatcher;
use crate::ingest::scanner::{ext_to_lang, Scanner, SkipReason};
use crate::models::file::FileRecord;

/// Statistics from an indexing run.
#[derive(Debug, Clone, Default)]
pub struct IndexResult {
    pub files_scanned: usize,
    pub files_indexed: usize,
    /// Total files skipped (sum of all skip categories).
    pub files_skipped: usize,
    pub chunks_created: usize,
    pub refs_created: usize,
    /// Files skipped due to unsupported language.
    pub skipped_unsupported: usize,
    /// Files skipped because they exceed `max_file_size_mb`.
    pub skipped_too_large: usize,
    /// Files skipped because content is not valid UTF-8.
    pub skipped_non_utf8: usize,
    /// Files skipped due to IO errors.
    pub skipped_io_error: usize,
    /// Files skipped because hash unchanged (incremental).
    pub skipped_unchanged: usize,
    /// Files removed from index because they no longer exist on disk.
    pub deleted_from_index: usize,
}

impl IndexResult {
    fn skip(&mut self, reason: SkipReason) {
        self.files_skipped += 1;
        match reason {
            SkipReason::UnsupportedExtension | SkipReason::UnsupportedLanguage => {
                self.skipped_unsupported += 1;
            }
            SkipReason::TooLarge => self.skipped_too_large += 1,
            SkipReason::NonUtf8 => self.skipped_non_utf8 += 1,
            SkipReason::IoError => self.skipped_io_error += 1,
            SkipReason::Unchanged => self.skipped_unchanged += 1,
        }
    }
}

/// Run the indexer: scan files, parse chunks, store in DB.
pub fn run_index(config: &Config) -> Result<IndexResult> {
    config.ensure_rlm_dir()?;

    let db = Database::open(&config.db_path)?;
    let scanner = Scanner::with_max_file_size(
        &config.project_root,
        config.settings.indexing.max_file_size_mb,
    );
    let dispatcher = Dispatcher::new();

    let scanned = scanner.scan()?;
    let mut result = IndexResult {
        files_scanned: scanned.len(),
        ..Default::default()
    };

    // Collect scanned paths to detect deleted files later
    let scanned_paths: HashSet<String> = scanned.iter().map(|f| f.relative_path.clone()).collect();

    // Wrap entire indexing loop in a single transaction for performance
    db.conn().execute_batch("BEGIN IMMEDIATE")?;
    let tx_result = (|| -> Result<()> {
        // Phase 1: Remove files from index that no longer exist on disk
        let indexed_files = db.get_all_files()?;
        for indexed_file in &indexed_files {
            if !scanned_paths.contains(&indexed_file.path) {
                db.delete_file(indexed_file.id)?;
                result.deleted_from_index += 1;
            }
        }

        // Phase 2: Index new/changed files
        for file in &scanned {
            let lang = ext_to_lang(&file.extension);

            // Skip unsupported languages
            if !dispatcher.supports(lang) {
                result.skip(SkipReason::UnsupportedLanguage);
                continue;
            }

            // Check if file changed (incremental indexing)
            if let Some(existing) = db.get_file_by_path(&file.relative_path)? {
                if existing.hash == file.hash {
                    result.skip(SkipReason::Unchanged);
                    continue;
                }
                // File changed: delete old chunks and re-index
                db.delete_chunks_for_file(existing.id)?;
            }

            // Read file content as bytes first to check UTF-8 validity
            let bytes = if let Ok(b) = std::fs::read(&file.path) {
                b
            } else {
                result.skip(SkipReason::IoError);
                continue;
            };

            // Validate UTF-8
            let source = if let Ok(s) = String::from_utf8(bytes) {
                s
            } else {
                result.skip(SkipReason::NonUtf8);
                continue;
            };

            // Upsert file record
            let file_record = FileRecord::new(
                file.relative_path.clone(),
                file.hash.clone(),
                lang.to_string(),
                file.size,
            );
            let file_id = db.upsert_file(&file_record)?;

            // Single-pass: extract chunks and refs together for code languages
            let (mut chunks, refs) = if dispatcher.is_code_language(lang) {
                if let Ok(parse_result) = dispatcher.parse_with_quality(lang, &source, file_id) {
                    // Store parse quality if not complete
                    if parse_result.quality.fallback_recommended() {
                        let quality_str = match &parse_result.quality {
                            crate::ingest::code::ParseQuality::Partial { .. } => "partial",
                            crate::ingest::code::ParseQuality::Failed { .. } => "failed",
                            _ => "complete",
                        };
                        let _ = db.set_file_parse_quality(file_id, quality_str);
                    }
                    (parse_result.chunks, parse_result.refs)
                } else {
                    result.skip(SkipReason::IoError);
                    continue;
                }
            } else if let Ok(c) = dispatcher.parse(lang, &source, file_id) {
                (c, vec![])
            } else {
                result.skip(SkipReason::IoError);
                continue;
            };

            // Apply UI context detection
            let ui_ctx = crate::ingest::scanner::detect_ui_context(&file.relative_path);
            if let Some(ctx) = &ui_ctx {
                for chunk in &mut chunks {
                    chunk.ui_ctx = Some(ctx.clone());
                }
            }

            // Insert chunks and get their IDs
            let mut inserted_chunks = Vec::new();
            for mut chunk in chunks {
                let cid = db.insert_chunk(&chunk)?;
                chunk.id = cid;
                inserted_chunks.push(chunk);
                result.chunks_created += 1;
            }

            // PERF: Sort chunks by start_line for binary search lookup
            // This converts O(refs * chunks) to O(refs * log(chunks))
            inserted_chunks.sort_by_key(|c| c.start_line);

            // Insert references, re-mapping chunk IDs to inserted values
            for mut reference in refs {
                if reference.chunk_id == 0 {
                    // Binary search to find the first chunk that could contain this line
                    let idx = inserted_chunks.partition_point(|c| c.start_line <= reference.line);
                    // Check chunks from idx backwards until we find one that contains the line
                    reference.chunk_id = if idx > 0 {
                        inserted_chunks[..idx]
                            .iter()
                            .rev()
                            .find(|c| reference.line <= c.end_line)
                            .map_or(0, |c| c.id)
                    } else {
                        0
                    };
                }
                if reference.chunk_id > 0 {
                    db.insert_ref(&reference)?;
                    result.refs_created += 1;
                }
            }

            result.files_indexed += 1;
        }
        Ok(())
    })();
    match &tx_result {
        Ok(()) => db.conn().execute_batch("COMMIT")?,
        Err(_) => {
            let _ = db.conn().execute_batch("ROLLBACK");
        }
    }
    tx_result?;

    Ok(result)
}

/// Ensure the index exists, creating it if necessary (auto-index).
pub fn ensure_index(config: &Config) -> Result<Database> {
    if !config.index_exists() {
        run_index(config)?;
    }
    Database::open(&config.db_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn index_rust_project() {
        let tmp = TempDir::new().unwrap();
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(
            src_dir.join("main.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n\nfn helper() -> i32 {\n    42\n}\n",
        )
        .unwrap();

        let config = Config::new(tmp.path());
        let result = run_index(&config).unwrap();

        assert!(result.files_indexed > 0);
        assert!(result.chunks_created > 0);
        assert!(config.index_exists());
    }

    #[test]
    fn incremental_index_skips_unchanged() {
        let tmp = TempDir::new().unwrap();
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();

        let config = Config::new(tmp.path());

        // First index
        let r1 = run_index(&config).unwrap();
        assert!(r1.files_indexed > 0);

        // Second index (no changes)
        let r2 = run_index(&config).unwrap();
        assert_eq!(r2.files_indexed, 0);
        assert!(r2.files_skipped > 0);
    }

    #[test]
    fn incremental_index_reindexes_changed() {
        let tmp = TempDir::new().unwrap();
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let file_path = src_dir.join("main.rs");
        fs::write(&file_path, "fn main() {}").unwrap();

        let config = Config::new(tmp.path());
        run_index(&config).unwrap();

        // Modify file
        fs::write(&file_path, "fn main() { println!(\"changed\"); }").unwrap();

        let r2 = run_index(&config).unwrap();
        assert!(r2.files_indexed > 0);
    }

    #[test]
    fn incremental_index_removes_deleted_files() {
        let tmp = TempDir::new().unwrap();
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // Create two files
        fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();
        fs::write(src_dir.join("helper.rs"), "fn helper() {}").unwrap();

        let config = Config::new(tmp.path());
        let r1 = run_index(&config).unwrap();
        assert_eq!(r1.files_indexed, 2);

        // Delete one file
        fs::remove_file(src_dir.join("helper.rs")).unwrap();

        let r2 = run_index(&config).unwrap();
        assert_eq!(r2.deleted_from_index, 1);
        assert_eq!(r2.skipped_unchanged, 1); // main.rs unchanged

        // Verify only main.rs remains in the database
        let db = crate::db::Database::open(&config.db_path).unwrap();
        let files = db.get_all_files().unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].path.contains("main.rs"));
    }

    #[test]
    fn index_result_categorizes_skips() {
        let tmp = TempDir::new().unwrap();
        let src_dir = tmp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        // Create a valid Rust file
        fs::write(src_dir.join("main.rs"), "fn main() {}").unwrap();

        // Create a binary file (non-UTF8)
        fs::write(src_dir.join("binary.rs"), &[0xFF, 0xFE, 0x00, 0x01]).unwrap();

        let config = Config::new(tmp.path());
        let result = run_index(&config).unwrap();

        assert_eq!(result.files_indexed, 1);
        assert_eq!(result.skipped_non_utf8, 1);
        assert_eq!(result.files_skipped, 1);
    }
}
