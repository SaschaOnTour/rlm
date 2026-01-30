use serde::Serialize;

use crate::db::Database;
use crate::error::Result;
use crate::models::token_estimate::{estimate_tokens, TokenEstimate};

/// A peek result: structure only, no content. Minimal tokens.
#[derive(Debug, Clone, Serialize)]
pub struct PeekResult {
    /// File entries with symbol summaries.
    #[serde(rename = "f")]
    pub files: Vec<PeekFile>,
    /// Token estimate for this response.
    #[serde(rename = "t")]
    pub tokens: TokenEstimate,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeekFile {
    /// File path.
    #[serde(rename = "p")]
    pub path: String,
    /// Language.
    #[serde(rename = "l")]
    pub lang: String,
    /// Line count of the file (approximated from chunks).
    #[serde(rename = "lc")]
    pub line_count: u32,
    /// Symbols in this file.
    #[serde(rename = "s")]
    pub symbols: Vec<PeekSymbol>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeekSymbol {
    /// Symbol kind (fn, struct, class, etc.).
    #[serde(rename = "k")]
    pub kind: String,
    /// Symbol name.
    #[serde(rename = "n")]
    pub name: String,
    /// Line number.
    #[serde(rename = "l")]
    pub line: u32,
}

/// Peek at the project structure: symbols and line counts, NO content.
/// This is the cheapest operation (~50 tokens per file).
pub fn peek(db: &Database, path_filter: Option<&str>) -> Result<PeekResult> {
    let files = db.get_all_files()?;
    let mut peek_files = Vec::new();

    for file in &files {
        // Apply path filter if specified
        if let Some(filter) = path_filter {
            if !file.path.starts_with(filter) {
                continue;
            }
        }

        let chunks = db.get_chunks_for_file(file.id)?;

        let max_line = chunks.iter().map(|c| c.end_line).max().unwrap_or(0);

        let symbols: Vec<PeekSymbol> = chunks
            .iter()
            .map(|c| PeekSymbol {
                kind: c.kind.as_str().to_string(),
                name: c.ident.clone(),
                line: c.start_line,
            })
            .collect();

        peek_files.push(PeekFile {
            path: file.path.clone(),
            lang: file.lang.clone(),
            line_count: max_line,
            symbols,
        });
    }

    // Estimate output tokens
    let output_str = serde_json::to_string(&peek_files).unwrap_or_default();
    let out_tokens = estimate_tokens(output_str.len());

    Ok(PeekResult {
        files: peek_files,
        tokens: TokenEstimate::new(0, out_tokens),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    #[test]
    fn peek_returns_structure_no_content() {
        let db = Database::open_in_memory().unwrap();
        let f = FileRecord::new("src/main.rs".into(), "h".into(), "rust".into(), 100);
        let fid = db.upsert_file(&f).unwrap();
        let c = Chunk {
            id: 0,
            file_id: fid,
            start_line: 1,
            end_line: 5,
            start_byte: 0,
            end_byte: 50,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: None,
            visibility: None,
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() { ... }".into(),
        };
        db.insert_chunk(&c).unwrap();

        let result = peek(&db, None).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].symbols.len(), 1);
        assert_eq!(result.files[0].symbols[0].name, "main");

        // Verify no content is in the output
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("fn main()"));
    }

    #[test]
    fn peek_with_path_filter() {
        let db = Database::open_in_memory().unwrap();
        db.upsert_file(&FileRecord::new(
            "src/a.rs".into(),
            "h1".into(),
            "rust".into(),
            50,
        ))
        .unwrap();
        db.upsert_file(&FileRecord::new(
            "lib/b.rs".into(),
            "h2".into(),
            "rust".into(),
            50,
        ))
        .unwrap();

        let result = peek(&db, Some("src/")).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "src/a.rs");
    }
}
