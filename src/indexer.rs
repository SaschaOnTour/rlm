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

/// Outcome of processing a single scanned file.
enum FileOutcome {
    /// File was skipped for the given reason.
    Skipped(SkipReason),
    /// File was indexed, producing this many chunks and refs.
    Indexed {
        chunks_created: usize,
        refs_created: usize,
    },
}

/// Read file bytes from disk, returning `None` (with skip reason) on failure.
fn read_file_source(path: &std::path::Path) -> std::result::Result<String, SkipReason> {
    let bytes = std::fs::read(path).map_err(|_| SkipReason::IoError)?;
    String::from_utf8(bytes).map_err(|_| SkipReason::NonUtf8)
}

/// Parse chunks and refs for a single file via the dispatcher.
fn parse_file_chunks(
    dispatcher: &Dispatcher,
    db: &Database,
    lang: &str,
    source: &str,
    file_id: i64,
) -> std::result::Result<
    (
        Vec<crate::models::chunk::Chunk>,
        Vec<crate::models::chunk::Reference>,
    ),
    SkipReason,
> {
    if dispatcher.is_code_language(lang) {
        let parse_result = dispatcher
            .parse_with_quality(lang, source, file_id)
            .map_err(|_| SkipReason::IoError)?;
        if parse_result.quality.fallback_recommended() {
            let quality_str = quality_label(&parse_result.quality);
            let _ = db.set_file_parse_quality(file_id, quality_str);
        }
        Ok((parse_result.chunks, parse_result.refs))
    } else {
        let chunks = dispatcher
            .parse(lang, source, file_id)
            .map_err(|_| SkipReason::IoError)?;
        Ok((chunks, vec![]))
    }
}

/// Map a `ParseQuality` to its database label.
fn quality_label(quality: &crate::ingest::code::ParseQuality) -> &'static str {
    match quality {
        crate::ingest::code::ParseQuality::Partial { .. } => "partial",
        crate::ingest::code::ParseQuality::Failed { .. } => "failed",
        _ => "complete",
    }
}

/// Tag every chunk with the UI context string, if present.
fn apply_ui_context(chunks: &mut [crate::models::chunk::Chunk], ui_ctx: &str) {
    for chunk in chunks.iter_mut() {
        chunk.ui_ctx = Some(ui_ctx.to_string());
    }
}

/// Insert chunks into the DB and return them sorted by `start_line` for fast ref lookup.
fn insert_chunks(
    db: &Database,
    chunks: Vec<crate::models::chunk::Chunk>,
) -> Result<Vec<crate::models::chunk::Chunk>> {
    let mut inserted = Vec::with_capacity(chunks.len());
    for mut chunk in chunks {
        let cid = db.insert_chunk(&chunk)?;
        chunk.id = cid;
        inserted.push(chunk);
    }
    inserted.sort_by_key(|c| c.start_line);
    Ok(inserted)
}

/// Resolve the chunk ID for a reference using binary search over sorted chunks.
fn resolve_ref_chunk_id(inserted_chunks: &[crate::models::chunk::Chunk], line: u32) -> i64 {
    let idx = inserted_chunks.partition_point(|c| c.start_line <= line);
    if idx > 0 {
        inserted_chunks[..idx]
            .iter()
            .rev()
            .find(|c| line <= c.end_line)
            .map_or(0, |c| c.id)
    } else {
        0
    }
}

/// Insert references into the DB, resolving chunk IDs via binary search.
fn insert_refs(
    db: &Database,
    refs: Vec<crate::models::chunk::Reference>,
    inserted_chunks: &[crate::models::chunk::Chunk],
) -> Result<usize> {
    let mut count = 0;
    for mut reference in refs {
        if reference.chunk_id == 0 {
            reference.chunk_id = resolve_ref_chunk_id(inserted_chunks, reference.line);
        }
        if reference.chunk_id > 0 {
            db.insert_ref(&reference)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Check whether a file should be skipped before reading (integration: calls only).
///
/// Returns `Some(SkipReason)` if the file should be skipped, `None` if it should be processed.
/// Also cleans up stale chunks when the file hash has changed.
fn check_file_freshness(
    db: &Database,
    dispatcher: &Dispatcher,
    file: &crate::ingest::scanner::ScannedFile,
    lang: &str,
) -> Result<Option<SkipReason>> {
    if !dispatcher.supports(lang) {
        return Ok(Some(SkipReason::UnsupportedLanguage));
    }

    if let Some(existing) = db.get_file_by_path(&file.relative_path)? {
        if existing.hash == file.hash {
            return Ok(Some(SkipReason::Unchanged));
        }
        db.delete_chunks_for_file(existing.id)?;
    }

    Ok(None)
}

/// Ingest a single file: read, parse, insert chunks/refs (integration: calls only).
fn ingest_file(
    db: &Database,
    dispatcher: &Dispatcher,
    file: &crate::ingest::scanner::ScannedFile,
    lang: &str,
    source: String,
) -> Result<FileOutcome> {
    let file_record = FileRecord::new(
        file.relative_path.clone(),
        file.hash.clone(),
        lang.to_string(),
        file.size,
    );
    let file_id = db.upsert_file(&file_record)?;

    let (mut chunks, refs) = match parse_file_chunks(dispatcher, db, lang, &source, file_id) {
        Ok(pair) => pair,
        Err(reason) => return Ok(FileOutcome::Skipped(reason)),
    };

    if let Some(ctx) = crate::ingest::scanner::detect_ui_context(&file.relative_path) {
        apply_ui_context(&mut chunks, &ctx);
    }

    let inserted_chunks = insert_chunks(db, chunks)?;
    let chunks_created = inserted_chunks.len();
    let refs_created = insert_refs(db, refs, &inserted_chunks)?;

    Ok(FileOutcome::Indexed {
        chunks_created,
        refs_created,
    })
}

/// Process a single scanned file: check freshness, read, parse, insert chunks/refs (integration).
fn process_single_file(
    db: &Database,
    dispatcher: &Dispatcher,
    file: &crate::ingest::scanner::ScannedFile,
) -> Result<FileOutcome> {
    let lang = ext_to_lang(&file.extension);

    if let Some(reason) = check_file_freshness(db, dispatcher, file, lang)? {
        return Ok(FileOutcome::Skipped(reason));
    }

    let source = match read_file_source(&file.path) {
        Ok(s) => s,
        Err(reason) => return Ok(FileOutcome::Skipped(reason)),
    };

    ingest_file(db, dispatcher, file, lang, source)
}

/// Remove indexed files that no longer exist on disk.
fn purge_deleted_files(db: &Database, scanned_paths: &HashSet<String>) -> Result<usize> {
    let indexed_files = db.get_all_files()?;
    let mut deleted = 0;
    for indexed_file in &indexed_files {
        if !scanned_paths.contains(&indexed_file.path) {
            db.delete_file(indexed_file.id)?;
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// Accumulate a `FileOutcome` into the running `IndexResult`.
fn accumulate_outcome(result: &mut IndexResult, outcome: FileOutcome) {
    match outcome {
        FileOutcome::Skipped(reason) => result.skip(reason),
        FileOutcome::Indexed {
            chunks_created,
            refs_created,
        } => {
            result.files_indexed += 1;
            result.chunks_created += chunks_created;
            result.refs_created += refs_created;
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

    let scanned_paths: HashSet<String> = scanned.iter().map(|f| f.relative_path.clone()).collect();

    db.conn().execute_batch("BEGIN IMMEDIATE")?;
    let tx_result = (|| -> Result<()> {
        result.deleted_from_index = purge_deleted_files(&db, &scanned_paths)?;

        for file in &scanned {
            let outcome = process_single_file(&db, &dispatcher, file)?;
            accumulate_outcome(&mut result, outcome);
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
// qual:allow(iosp) reason: "check-then-act: ensure index exists before opening"
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

    /// Non-UTF-8 byte sequence used to test binary file rejection.
    const NON_UTF8_BYTES: [u8; 4] = [0xFF, 0xFE, 0x00, 0x01];

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
        fs::write(src_dir.join("binary.rs"), NON_UTF8_BYTES).unwrap();

        let config = Config::new(tmp.path());
        let result = run_index(&config).unwrap();

        assert_eq!(result.files_indexed, 1);
        assert_eq!(result.skipped_non_utf8, 1);
        assert_eq!(result.files_skipped, 1);
    }
}
