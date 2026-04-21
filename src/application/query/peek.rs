use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// A peek result: structure only, no content. Minimal tokens.
#[derive(Debug, Clone, Serialize)]
pub struct PeekResult {
    /// File entries with symbol summaries.
    pub files: Vec<PeekFile>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeekFile {
    /// File path.
    pub path: String,
    /// Language.
    pub lang: String,
    /// Line count of the file (approximated from chunks).
    pub line_count: u32,
    /// Symbols in this file.
    pub symbols: Vec<PeekSymbol>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeekSymbol {
    /// Symbol kind (fn, struct, class, etc.).
    pub kind: String,
    /// Symbol name.
    pub name: String,
    /// Line number.
    pub line: u32,
}

/// Check if a file passes the path filter (operation: logic only).
fn matches_path_filter(file_path: &str, path_filter: Option<&str>) -> bool {
    match path_filter {
        Some(filter) => file_path.starts_with(filter),
        None => true,
    }
}

/// Build a `PeekFile` from chunks (operation: logic only, uses only std methods).
fn build_peek_file(path: &str, lang: &str, chunks: &[crate::domain::chunk::Chunk]) -> PeekFile {
    let max_line = chunks.iter().map(|c| c.end_line).max().unwrap_or(0);

    let symbols: Vec<PeekSymbol> = chunks
        .iter()
        .map(|c| PeekSymbol {
            kind: c.kind.as_str().to_string(),
            name: c.ident.clone(),
            line: c.start_line,
        })
        .collect();

    PeekFile {
        path: path.to_string(),
        lang: lang.to_string(),
        line_count: max_line,
        symbols,
    }
}

/// Peek at the project structure: symbols and line counts, NO content (integration: calls only).
/// This is the cheapest operation (~50 tokens per file).
pub fn peek(db: &Database, path_filter: Option<&str>) -> Result<PeekResult> {
    let files = db.get_all_files()?;
    let mut peek_files = Vec::new();

    for file in &files {
        if !matches_path_filter(&file.path, path_filter) {
            continue;
        }

        let chunks = db.get_chunks_for_file(file.id)?;
        peek_files.push(build_peek_file(&file.path, &file.lang, &chunks));
    }

    let mut result = PeekResult {
        files: peek_files,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
#[path = "peek_tests.rs"]
mod tests;
