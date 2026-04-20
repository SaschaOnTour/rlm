//! MCP `read` tool handlers: symbol / section retrieval.

use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;

use crate::application::dto::chunk_dto::ChunkDto;
use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::interface::shared::{record_operation, AlternativeCost, OperationMeta};
use crate::output::Formatter;

use super::server::RlmServer;
use super::tools::ReadParams;

/// Max sections to show in "not found" error hints.
const MAX_HINT_SECTIONS: usize = 10;

/// Handle the `read` tool: read a specific symbol or markdown section.
// qual:api
pub fn handle_read(
    db: &Database,
    params: &ReadParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    match (&params.symbol, &params.section) {
        (Some(_), _) => handle_read_symbol(db, params, formatter),
        (_, Some(_)) => handle_read_section(db, params, formatter),
        _ => Ok(RlmServer::error_text(
            formatter,
            "read requires 'symbol' or 'section'. Use Claude Code's Read for full files or line ranges.".into(),
        )),
    }
}

/// Filter chunks to those belonging to a specific file path (operation: logic only).
fn filter_chunks_by_path<'a>(db: &Database, chunks: &'a [Chunk], path: &str) -> Vec<&'a Chunk> {
    // Single O(1) lookup instead of loading all files and scanning O(files × chunks)
    let file_id = match db.get_file_by_path(path) {
        Ok(Some(f)) => f.id,
        _ => return Vec::new(),
    };
    chunks.iter().filter(|c| c.file_id == file_id).collect()
}

/// Resolve which chunks to return and build the result (integration: calls only).
// qual:allow(iosp) reason: "MCP handler with inherent error matching and delegation"
fn handle_read_symbol(
    db: &Database,
    params: &ReadParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let sym = params.symbol.as_deref().unwrap_or_default();
    let chunks = match db.get_chunks_by_ident(sym) {
        Ok(c) => c,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };

    if chunks.is_empty() {
        return Ok(RlmServer::error_text(
            formatter,
            format!(
                "Symbol not found: {sym}. Use 'search' to find similar symbols, or check the 'path' parameter."
            ),
        ));
    }

    let file_chunks = filter_chunks_by_path(db, &chunks, &params.path);

    if file_chunks.is_empty() {
        let dtos: Vec<ChunkDto> = chunks.iter().cloned().map(Into::into).collect();
        RlmServer::read_symbol_result(db, params, &dtos, formatter)
    } else {
        let dtos: Vec<ChunkDto> = file_chunks
            .iter()
            .copied()
            .cloned()
            .map(Into::into)
            .collect();
        RlmServer::read_symbol_result(db, params, &dtos, formatter)
    }
}

fn section_not_found_hint(heading: &str, chunks: &[Chunk]) -> String {
    let total = chunks.len();
    let shown: Vec<&str> = chunks
        .iter()
        .take(MAX_HINT_SECTIONS)
        .map(|c| c.ident.as_str())
        .collect();
    if shown.is_empty() {
        format!("section not found: {heading}. File has no sections.")
    } else if total > shown.len() {
        format!(
            "section not found: {heading}. Available ({total} total, first {MAX_HINT_SECTIONS}): {}",
            shown.join(", ")
        )
    } else {
        format!(
            "section not found: {heading}. Available: {}",
            shown.join(", ")
        )
    }
}

fn handle_read_section(
    db: &Database,
    params: &ReadParams,
    formatter: Formatter,
) -> Result<CallToolResult, McpError> {
    let heading = params.section.as_deref().unwrap_or_default();

    let file = match db.get_file_by_path(&params.path) {
        Ok(Some(f)) => f,
        Ok(None) => {
            return Ok(RlmServer::error_text(
                formatter,
                format!(
                    "File not found: {}. Run 'index' to update, or check 'files' for available paths.",
                    params.path
                ),
            ));
        }
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };

    let chunks = match db.get_chunks_for_file(file.id) {
        Ok(c) => c,
        Err(e) => return Ok(RlmServer::error_text(formatter, e.to_string())),
    };

    let sections: Vec<_> = chunks.into_iter().filter(|c| c.kind.is_section()).collect();

    if let Some(c) = sections.iter().find(|c| c.ident == *heading) {
        let meta = OperationMeta {
            command: "read_section",
            files_touched: 1,
            alternative: AlternativeCost::SingleFile {
                path: params.path.clone(),
            },
        };
        let dto: ChunkDto = c.clone().into();
        let response = record_operation(db, &meta, &dto);
        return Ok(RlmServer::success_text(formatter, response.body));
    }

    Ok(RlmServer::error_text(
        formatter,
        section_not_found_hint(heading, &sections),
    ))
}
