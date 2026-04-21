use super::validator::{validate_and_write, SyntaxGuard};
use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::error::{Result, RlmError};
use crate::ingest::scanner::ext_to_lang;

/// Look up a file and its matching chunk by symbol identifier.
///
/// Returns the resolved `Chunk` (cloned) so callers can use byte offsets, content, etc.
fn find_symbol_in_file(db: &Database, file_path: &str, symbol: &str) -> Result<Chunk> {
    let file = db
        .get_file_by_path(file_path)?
        .ok_or_else(|| RlmError::FileNotFound {
            path: file_path.into(),
        })?;

    let chunks = db.get_chunks_for_file(file.id)?;
    let chunk =
        chunks
            .iter()
            .find(|c| c.ident == symbol)
            .ok_or_else(|| RlmError::SymbolNotFound {
                ident: symbol.into(),
            })?;

    Ok(chunk.clone())
}

/// Result of a successful `replace_symbol` call.
#[derive(Debug)]
pub struct ReplaceOutcome {
    /// Length of the old code that was replaced (in bytes).
    pub old_code_len: usize,
}

/// Replace an AST node (function, struct, etc.) by identifier.
///
/// `file_path` is the project-relative path (as stored in the DB).
/// `project_root` is used to resolve the absolute path for disk I/O.
pub fn replace_symbol(
    db: &Database,
    file_path: &str,
    symbol: &str,
    new_code: &str,
    project_root: &std::path::Path,
) -> Result<ReplaceOutcome> {
    // Validate and resolve the project-relative path before the DB lookup and file read below.
    let full_path = crate::error::validate_relative_path(file_path, project_root)?;

    let chunk = find_symbol_in_file(db, file_path, symbol)?;
    let source = std::fs::read_to_string(&full_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            RlmError::FileNotFound {
                path: file_path.into(),
            }
        } else {
            RlmError::from(e)
        }
    })?;

    // Replace the byte range
    let start = chunk.start_byte as usize;
    let end = chunk.end_byte as usize;

    if start > source.len() || end > source.len() {
        return Err(RlmError::EditConflict);
    }

    // Verify the content at the indexed byte range still matches the chunk.
    let actual_content = source.get(start..end).ok_or(RlmError::EditConflict)?;
    if actual_content != chunk.content {
        return Err(RlmError::EditConflict);
    }

    let mut modified = String::with_capacity(source.len() - (end - start) + new_code.len());
    modified.push_str(source.get(..start).ok_or(RlmError::EditConflict)?);
    modified.push_str(new_code);
    modified.push_str(source.get(end..).ok_or(RlmError::EditConflict)?);

    // Determine language from file extension
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = ext_to_lang(ext);

    // Validate and write
    let guard = SyntaxGuard::new();
    validate_and_write(&guard, lang, &modified, &full_path)?;

    let old_code_len = chunk.content.len();
    Ok(ReplaceOutcome { old_code_len })
}

/// Preview a replacement without writing (returns the diff).
pub fn preview_replace(
    db: &Database,
    file_path: &str,
    symbol: &str,
    new_code: &str,
) -> Result<ReplaceDiff> {
    let chunk = find_symbol_in_file(db, file_path, symbol)?;

    Ok(ReplaceDiff {
        file: file_path.to_string(),
        symbol: symbol.to_string(),
        old_code: chunk.content.clone(),
        new_code: new_code.to_string(),
        start_line: chunk.start_line,
        end_line: chunk.end_line,
    })
}

/// A diff showing what would change.
#[derive(Debug, Clone)]
pub struct ReplaceDiff {
    pub file: String,
    pub symbol: String,
    pub old_code: String,
    pub new_code: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// Serialize with backward-compatible `old_lines: [start, end]` instead of separate fields.
impl serde::Serialize for ReplaceDiff {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ReplaceDiff", 5)?;
        state.serialize_field("file", &self.file)?;
        state.serialize_field("symbol", &self.symbol)?;
        state.serialize_field("old_code", &self.old_code)?;
        state.serialize_field("new_code", &self.new_code)?;
        state.serialize_field("old_lines", &[self.start_line, self.end_line])?;
        state.end()
    }
}

#[cfg(test)]
#[path = "replacer_edge_tests.rs"]
mod edge_tests;
#[cfg(test)]
#[path = "replacer_tests.rs"]
mod tests;
