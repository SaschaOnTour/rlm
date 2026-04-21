mod db_insert;
mod file_processing;
pub mod staleness;

#[cfg(test)]
#[path = "fixtures_tests.rs"]
mod fixtures;

use std::collections::HashSet;

use crate::config::Config;
use crate::db::Database;
use crate::domain::file::FileRecord;
use crate::error::Result;
use crate::ingest::dispatcher::Dispatcher;
use crate::ingest::scanner::{ext_to_lang, Scanner, SkipReason};

use db_insert::{insert_chunks, insert_refs};
use file_processing::{apply_ui_context, parse_file_chunks, read_file_source};

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

/// Shared parse+insert pipeline used by both bulk indexing and single-file reindex.
fn index_source(
    db: &Database,
    dispatcher: &Dispatcher,
    source: &str,
    file_id: i64,
    rel_path: &str,
) -> Result<(usize, usize)> {
    let ext = std::path::Path::new(rel_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let lang = ext_to_lang(ext);
    let (mut chunks, refs) =
        parse_file_chunks(dispatcher, db, lang, source, file_id).map_err(|_| {
            crate::error::RlmError::Parse {
                path: rel_path.to_string(),
                detail: "parse failed during indexing".into(),
            }
        })?;

    if let Some(ctx) = crate::ingest::scanner::detect_ui_context(rel_path) {
        apply_ui_context(&mut chunks, &ctx);
    }

    let inserted = insert_chunks(db, chunks)?;
    let chunks_created = inserted.len();
    let refs_created = insert_refs(db, refs, &inserted)?;

    Ok((chunks_created, refs_created))
}

/// Ingest a single file: read, parse, insert chunks/refs (integration: calls only).
fn ingest_file(
    db: &Database,
    dispatcher: &Dispatcher,
    file: &crate::ingest::scanner::ScannedFile,
    lang: &str,
    source: String,
) -> Result<FileOutcome> {
    let file_record = FileRecord::with_mtime(
        file.relative_path.clone(),
        file.hash.clone(),
        lang.to_string(),
        file.size,
        file.mtime_nanos,
    );
    let file_id = db.upsert_file(&file_record)?;

    match index_source(db, dispatcher, &source, file_id, &file.relative_path) {
        Ok((chunks_created, refs_created)) => Ok(FileOutcome::Indexed {
            chunks_created,
            refs_created,
        }),
        Err(_) => Ok(FileOutcome::Skipped(
            crate::ingest::scanner::SkipReason::IoError,
        )),
    }
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

/// Progress callback: (current_file_1based, total_files).
pub type ProgressCallback = dyn Fn(usize, usize) + Send;

/// Run `work` inside an IMMEDIATE transaction: commit on Ok, roll back
/// on Err. Shared by `run_index` and `reindex_single_file` so the
/// BEGIN/COMMIT/ROLLBACK boilerplate lives in exactly one place.
fn run_in_transaction<T>(db: &Database, work: impl FnOnce() -> Result<T>) -> Result<T> {
    db.conn().execute_batch("BEGIN IMMEDIATE")?;
    let outcome = work();
    match &outcome {
        Ok(_) => db.conn().execute_batch("COMMIT")?,
        Err(_) => {
            let _ = db.conn().execute_batch("ROLLBACK");
        }
    }
    outcome
}

/// Run the indexer: scan files, parse chunks, store in DB.
pub fn run_index(config: &Config, progress: Option<&ProgressCallback>) -> Result<IndexResult> {
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

    run_in_transaction(&db, || {
        result.deleted_from_index = purge_deleted_files(&db, &scanned_paths)?;

        let total = scanned.len();
        for (i, file) in scanned.iter().enumerate() {
            if let Some(cb) = &progress {
                cb(i + 1, total);
            }
            let outcome = process_single_file(&db, &dispatcher, file)?;
            accumulate_outcome(&mut result, outcome);
        }
        Ok(())
    })?;

    Ok(result)
}

/// Re-index a single file after a write operation (replace/insert).
pub fn reindex_single_file(
    db: &Database,
    config: &Config,
    rel_path: &str,
) -> Result<(usize, usize)> {
    let dispatcher = Dispatcher::new();
    let full_path = config.project_root.join(rel_path);
    let source = std::fs::read_to_string(&full_path)?;
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = ext_to_lang(ext);

    if !dispatcher.supports(lang) {
        return Ok((0, 0));
    }

    run_in_transaction(db, || {
        if let Some(existing) = db.get_file_by_path(rel_path)? {
            db.delete_chunks_for_file(existing.id)?;
        }
        let hash = crate::ingest::hasher::hash_bytes(source.as_bytes());
        // Nanosecond precision so staleness can distinguish sub-second edits.
        // `as_nanos()` returns u128 but fits in i64 until year 2262.
        let mtime_nanos = std::fs::metadata(&full_path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        let file_record = FileRecord::with_mtime(
            rel_path.into(),
            hash,
            lang.into(),
            source.len() as u64,
            mtime_nanos,
        );
        let file_id = db.upsert_file(&file_record)?;
        index_source(db, &dispatcher, &source, file_id, rel_path)
    })
}

/// Max lines to include in the post-write preview.
const PREVIEW_LINES: usize = 10;

/// What to preview after a write operation.
pub enum PreviewSource<'a> {
    /// Preview the named symbol (used by replace).
    Symbol(&'a str),
    /// Preview the chunk containing the given line (used by insert).
    Line(u32),
    /// Preview the last chunk in the file (used by insert at bottom).
    Last,
    /// No preview.
    None,
}

/// Re-index a file after write and build the JSON result with optional preview.
///
/// Shared by MCP and CLI write handlers to avoid duplicating reindex + preview logic.
pub fn reindex_with_result(
    db: &Database,
    config: &Config,
    rel_path: &str,
    source: PreviewSource<'_>,
) -> String {
    match reindex_single_file(db, config, rel_path) {
        Ok((chunks, refs)) => {
            let preview = find_preview(db, rel_path, &source);
            let mut result =
                serde_json::json!({"ok": true, "reindexed": true, "chunks": chunks, "refs": refs});
            if let Some(p) = preview {
                result["preview"] = serde_json::Value::String(p);
            }
            result.to_string()
        }
        Err(e) => {
            serde_json::json!({"ok": true, "reindexed": false, "hint": format!("reindex failed: {e}")})
                .to_string()
        }
    }
}

/// Find a preview string based on the preview source.
fn find_preview(db: &Database, rel_path: &str, source: &PreviewSource<'_>) -> Option<String> {
    // Early exit avoids DB queries when no preview is requested.
    if matches!(source, PreviewSource::None) {
        return None;
    }

    let file = db.get_file_by_path(rel_path).ok().flatten()?;
    let chunks = db.get_chunks_for_file(file.id).ok()?;

    let chunk = match source {
        PreviewSource::Symbol(sym) => chunks.into_iter().find(|c| c.ident == *sym),
        PreviewSource::Line(line) => chunks
            .into_iter()
            .find(|c| c.start_line <= *line && *line <= c.end_line),
        PreviewSource::Last => chunks.into_iter().max_by_key(|c| c.start_line),
        PreviewSource::None => return None,
    }?;

    let lines: Vec<&str> = chunk.content.lines().collect();
    let selected = match source {
        PreviewSource::Symbol(_) => &lines[..lines.len().min(PREVIEW_LINES)],
        PreviewSource::Line(line) => {
            let max_start = lines.len().saturating_sub(PREVIEW_LINES);
            let target_idx = (*line).saturating_sub(chunk.start_line) as usize;
            let start = target_idx.saturating_sub(PREVIEW_LINES / 2).min(max_start);
            let end = (start + PREVIEW_LINES).min(lines.len());
            &lines[start..end]
        }
        PreviewSource::Last => {
            let start = lines.len().saturating_sub(PREVIEW_LINES);
            &lines[start..]
        }
        PreviewSource::None => return None,
    };

    Some(selected.join("\n"))
}

/// Ensure the index exists, creating it if necessary (auto-index).
pub fn ensure_index(config: &Config) -> Result<Database> {
    if !config.index_exists() {
        run_index(config, None)?;
    }
    Database::open(&config.db_path)
}

#[cfg(test)]
#[path = "mod_reindex_tests.rs"]
mod reindex_tests;
#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
