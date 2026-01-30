use crate::db::Database;
use crate::edit::syntax_guard::SyntaxGuard;
use crate::error::{Result, RlmError};
use crate::ingest::scanner::ext_to_lang;

/// Replace an AST node (function, struct, etc.) by identifier.
pub fn replace_symbol(
    db: &Database,
    file_path: &str,
    symbol: &str,
    new_code: &str,
    guard: &SyntaxGuard,
) -> Result<String> {
    // Find the file
    let file = db
        .get_file_by_path(file_path)?
        .ok_or_else(|| RlmError::FileNotFound {
            path: file_path.into(),
        })?;

    // Find the chunk for this symbol in this file
    let chunks = db.get_chunks_for_file(file.id)?;
    let chunk =
        chunks
            .iter()
            .find(|c| c.ident == symbol)
            .ok_or_else(|| RlmError::SymbolNotFound {
                ident: symbol.into(),
            })?;

    // Read the actual file content
    let full_path = std::path::Path::new(file_path);
    let source = if full_path.exists() {
        std::fs::read_to_string(full_path)?
    } else {
        return Err(RlmError::FileNotFound {
            path: file_path.into(),
        });
    };

    // Replace the byte range
    let start = chunk.start_byte as usize;
    let end = chunk.end_byte as usize;

    if start > source.len() || end > source.len() {
        return Err(RlmError::EditConflict);
    }

    let mut modified = String::with_capacity(source.len() - (end - start) + new_code.len());
    modified.push_str(&source[..start]);
    modified.push_str(new_code);
    modified.push_str(&source[end..]);

    // Determine language from file extension
    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = ext_to_lang(ext);

    // Validate and write
    guard.validate_and_write(lang, &modified, full_path)?;

    Ok(modified)
}

/// Preview a replacement without writing (returns the diff).
pub fn preview_replace(
    db: &Database,
    file_path: &str,
    symbol: &str,
    new_code: &str,
) -> Result<ReplaceDiff> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    #[test]
    fn preview_replace_works() {
        let db = Database::open_in_memory().unwrap();
        let f = FileRecord::new("test.rs".into(), "h".into(), "rust".into(), 100);
        let fid = db.upsert_file(&f).unwrap();
        let c = Chunk {
            id: 0,
            file_id: fid,
            start_line: 1,
            end_line: 3,
            start_byte: 0,
            end_byte: 14,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() {\n}".into(),
        };
        db.insert_chunk(&c).unwrap();

        let diff =
            preview_replace(&db, "test.rs", "main", "fn main() { println!(\"hi\"); }").unwrap();
        assert_eq!(diff.symbol, "main");
        assert!(diff.old_code.contains("fn main()"));
    }
}
