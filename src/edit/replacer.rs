use crate::db::Database;
use crate::edit::syntax_guard::{validate_and_write, SyntaxGuard};
use crate::error::{Result, RlmError};
use crate::ingest::scanner::ext_to_lang;
use crate::models::chunk::Chunk;

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
) -> Result<String> {
    let chunk = find_symbol_in_file(db, file_path, symbol)?;

    // Resolve relative path against project root for disk I/O
    let full_path = project_root.join(file_path);
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

    Ok(modified)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    /// File size in bytes for the test file record.
    const TEST_FILE_SIZE: u64 = 100;
    /// End line of the test chunk (3 lines of code).
    const CHUNK_END_LINE: u32 = 3;
    /// End byte offset of the test chunk content "fn main() {\n}".
    const CHUNK_END_BYTE: u32 = 14;

    #[test]
    fn preview_replace_works() {
        let db = Database::open_in_memory().unwrap();
        let f = FileRecord::new("test.rs".into(), "h".into(), "rust".into(), TEST_FILE_SIZE);
        let fid = db.upsert_file(&f).unwrap();
        let c = Chunk {
            id: 0,
            file_id: fid,
            start_line: 1,
            end_line: CHUNK_END_LINE,
            start_byte: 0,
            end_byte: CHUNK_END_BYTE,
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

    /// Helper: set up a temp file with content, index it in an in-memory DB,
    /// and return `(TempDir, Database, relative-path, project-root)`.
    fn setup_temp_project(
        content: &str,
    ) -> (tempfile::TempDir, Database, String, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        std::fs::write(&file_path, content).unwrap();

        let db = Database::open_in_memory().unwrap();
        let rel_path = "lib.rs".to_string();
        let f = FileRecord::new(
            rel_path.clone(),
            "h".into(),
            "rust".into(),
            content.len() as u64,
        );
        let fid = db.upsert_file(&f).unwrap();
        let c = Chunk {
            kind: ChunkKind::Function,
            ident: "greet".into(),
            end_line: CHUNK_END_LINE,
            end_byte: content.len() as u32,
            content: content.into(),
            ..Chunk::stub(fid)
        };
        db.insert_chunk(&c).unwrap();

        let project_root = dir.path().to_path_buf();
        (dir, db, rel_path, project_root)
    }

    #[test]
    fn replace_stale_content_rejects() {
        let original = "fn greet() {\n    println!(\"hello\");\n}";
        let (_dir, db, path, root) = setup_temp_project(original);

        // Modify the file on disk after indexing
        std::fs::write(
            root.join(&path),
            "fn greet() {\n    println!(\"goodbye\");\n}",
        )
        .unwrap();

        let result = replace_symbol(&db, &path, "greet", "fn greet() {}", &root);
        assert!(result.is_err(), "should reject stale content");
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("edit conflict"),
            "error should mention edit conflict, got: {msg}"
        );
    }

    #[test]
    fn replace_same_length_different_content_rejects() {
        let original = "fn greet() {\n    println!(\"AAAA\");\n}";
        let (_dir, db, path, root) = setup_temp_project(original);

        let tampered = "fn greet() {\n    println!(\"BBBB\");\n}";
        assert_eq!(
            original.len(),
            tampered.len(),
            "test premise: same byte length"
        );
        std::fs::write(root.join(&path), tampered).unwrap();

        let result = replace_symbol(&db, &path, "greet", "fn greet() {}", &root);
        assert!(
            result.is_err(),
            "should reject same-length different content"
        );
    }
}
