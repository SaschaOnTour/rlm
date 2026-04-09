//! File-level processing: reading, parsing, quality detection, UI context.

use crate::db::Database;
use crate::ingest::dispatcher::Dispatcher;
use crate::ingest::scanner::SkipReason;

/// Read file bytes from disk, returning `SkipReason` on failure.
pub(super) fn read_file_source(path: &std::path::Path) -> std::result::Result<String, SkipReason> {
    let bytes = std::fs::read(path).map_err(|_| SkipReason::IoError)?;
    String::from_utf8(bytes).map_err(|_| SkipReason::NonUtf8)
}

/// Parse chunks and refs for a single file via the dispatcher.
pub(super) fn parse_file_chunks(
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
pub(super) fn apply_ui_context(chunks: &mut [crate::models::chunk::Chunk], ui_ctx: &str) {
    for chunk in chunks.iter_mut() {
        chunk.ui_ctx = Some(ui_ctx.to_string());
    }
}
