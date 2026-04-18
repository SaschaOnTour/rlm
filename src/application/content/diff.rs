//! Diff operations shared between CLI and MCP.
//!
//! Provides consistent behavior for comparing indexed versions with current disk versions.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::application::FileQuery;
use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;
use crate::ingest::hasher;

/// Result of comparing a file with its indexed version.
#[derive(Debug, Clone, Serialize)]
pub struct FileDiffResult {
    /// The file path.
    pub file: String,
    /// Whether the file has changed since indexing.
    pub changed: bool,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Result of comparing a symbol with its indexed version.
#[derive(Debug, Clone, Serialize)]
pub struct SymbolDiffResult {
    /// The file path.
    pub file: String,
    /// The symbol name.
    pub symbol: String,
    /// The indexed content.
    pub indexed: String,
    /// The current content.
    pub current: String,
    /// Whether the content has changed.
    pub changed: bool,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// Compare a file's current state with its indexed version.
///
/// Returns `changed = true` if:
/// - The file is not in the index, OR
/// - The file's hash differs from the indexed hash
pub fn diff_file(db: &Database, path: &str, project_root: &Path) -> Result<FileDiffResult> {
    let full_path = crate::error::validate_relative_path(path, project_root)?;

    let file = db.get_file_by_path(path)?;

    let current = std::fs::read_to_string(&full_path)?;
    let current_hash = hasher::hash_bytes(current.as_bytes());

    // Unified logic: changed if file not indexed OR hash differs
    let changed = file.is_none_or(|f| f.hash != current_hash);

    let mut result = FileDiffResult {
        file: path.to_string(),
        changed,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Compare a symbol's current state with its indexed version.
///
/// Reads the current file content and extracts the same line range as the indexed chunk.
pub fn diff_symbol(
    db: &Database,
    path: &str,
    symbol: &str,
    project_root: &Path,
) -> Result<SymbolDiffResult> {
    let full_path = crate::error::validate_relative_path(path, project_root)?;

    let chunks = db.get_chunks_by_ident(symbol)?;
    let chunk = chunks
        .first()
        .ok_or_else(|| crate::error::RlmError::SymbolNotFound {
            ident: symbol.to_string(),
        })?;

    let current = std::fs::read_to_string(&full_path)?;

    // Extract current content at the same line range
    let lines: Vec<&str> = current.lines().collect();
    let start = (chunk.start_line as usize).saturating_sub(1);
    let end = (chunk.end_line as usize).min(lines.len());
    let current_content = lines[start..end].join("\n");

    let changed = chunk.content.trim() != current_content.trim();

    let mut result = SymbolDiffResult {
        file: path.to_string(),
        symbol: symbol.to_string(),
        indexed: chunk.content.clone(),
        current: current_content,
        changed,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// `diff <path>` without a symbol filter, as a [`FileQuery`].
pub struct DiffFileQuery {
    pub project_root: PathBuf,
}

impl FileQuery for DiffFileQuery {
    type Output = FileDiffResult;
    const COMMAND: &'static str = "diff";

    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output> {
        diff_file(db, path, &self.project_root)
    }
}

/// `diff <path> --symbol <sym>` as a [`FileQuery`].
pub struct DiffSymbolQuery {
    pub symbol: String,
    pub project_root: PathBuf,
}

impl FileQuery for DiffSymbolQuery {
    type Output = SymbolDiffResult;
    const COMMAND: &'static str = "diff";

    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output> {
        diff_symbol(db, path, &self.symbol, &self.project_root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;
    use std::io::Write;
    use tempfile::TempDir;

    const TEST_FILE_BYTES: u64 = 100;
    const TEST_START_LINE: u32 = 1;
    const TEST_END_LINE: u32 = 3;
    const TEST_START_BYTE: u32 = 0;
    const TEST_END_BYTE: u32 = 50;

    fn setup_test_db_and_dir() -> (Database, TempDir) {
        let db = Database::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        (db, tmp)
    }

    #[test]
    fn diff_file_unchanged() {
        let (db, tmp) = setup_test_db_and_dir();

        // Create file on disk
        let file_path = tmp.path().join("test.rs");
        let content = "fn main() {}";
        std::fs::write(&file_path, content).unwrap();

        // Index with matching hash
        let hash = hasher::hash_bytes(content.as_bytes());
        let file = FileRecord::new("test.rs".into(), hash, "rust".into(), content.len() as u64);
        db.upsert_file(&file).unwrap();

        let result = diff_file(&db, "test.rs", tmp.path()).unwrap();
        assert!(!result.changed);
    }

    #[test]
    fn diff_file_changed() {
        let (db, tmp) = setup_test_db_and_dir();

        // Create file on disk
        let file_path = tmp.path().join("test.rs");
        std::fs::write(&file_path, "fn main() { new code }").unwrap();

        // Index with different hash
        let file = FileRecord::new(
            "test.rs".into(),
            "oldhash".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        db.upsert_file(&file).unwrap();

        let result = diff_file(&db, "test.rs", tmp.path()).unwrap();
        assert!(result.changed);
    }

    #[test]
    fn diff_file_not_indexed() {
        let (db, tmp) = setup_test_db_and_dir();

        // Create file on disk but don't index it
        let file_path = tmp.path().join("test.rs");
        std::fs::write(&file_path, "fn main() {}").unwrap();

        let result = diff_file(&db, "test.rs", tmp.path()).unwrap();
        assert!(result.changed); // Not indexed = changed
    }

    #[test]
    fn diff_symbol_works() {
        let (db, tmp) = setup_test_db_and_dir();

        // Create file on disk
        let file_path = tmp.path().join("test.rs");
        let mut file = std::fs::File::create(&file_path).unwrap();
        writeln!(file, "fn main() {{").unwrap();
        writeln!(file, "    println!(\"hello\");").unwrap();
        writeln!(file, "}}").unwrap();

        // Index the file and chunk
        let file_rec = FileRecord::new(
            "test.rs".into(),
            "hash".into(),
            "rust".into(),
            TEST_FILE_BYTES,
        );
        let file_id = db.upsert_file(&file_rec).unwrap();

        let chunk = Chunk {
            id: 0,
            file_id,
            start_line: TEST_START_LINE,
            end_line: TEST_END_LINE,
            start_byte: TEST_START_BYTE,
            end_byte: TEST_END_BYTE,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: Some("fn main()".into()),
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() {\n    println!(\"hello\");\n}".into(),
        };
        db.insert_chunk(&chunk).unwrap();

        let result = diff_symbol(&db, "test.rs", "main", tmp.path()).unwrap();
        assert!(!result.changed);
    }
}
