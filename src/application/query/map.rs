//! Map building shared between CLI and MCP.

use std::collections::HashMap;

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::Result;

/// Wrapped map result with token estimate.
#[derive(Debug, Clone, Serialize)]
pub struct MapResult {
    /// Map entries.
    pub results: Vec<MapEntry>,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}

/// A single entry in the project map.
#[derive(Debug, Clone, Serialize)]
pub struct MapEntry {
    /// File path.
    pub file: String,
    /// Language identifier.
    pub lang: String,
    /// Number of lines in the file.
    pub line_count: u32,
    /// Public symbols in format "kind:name".
    pub symbols: Vec<String>,
    /// Description of file contents (e.g., "3 fn, 2 struct").
    pub description: String,
}

/// Build a project map showing file overview.
///
/// For each file (optionally filtered by path prefix), returns:
/// - Language
/// - Line count
/// - Public symbols
/// - Description of contained items
pub fn build_map(db: &Database, path_filter: Option<&str>) -> Result<MapResult> {
    let files = db.get_all_files()?;

    let mut entries = Vec::new();
    for file in &files {
        if let Some(filter) = path_filter {
            if !file.path.starts_with(filter) {
                continue;
            }
        }

        let chunks = db.get_chunks_for_file(file.id)?;
        let max_line = chunks.iter().map(|c| c.end_line).max().unwrap_or(0);
        let pub_symbols: Vec<String> = chunks
            .iter()
            .filter(|c| {
                c.visibility
                    .as_ref()
                    .is_some_and(|v| v == "pub" || v == "public")
            })
            .map(|c| format!("{}:{}", c.kind.as_str(), c.ident))
            .collect();

        let kind_counts: HashMap<&str, usize> = chunks.iter().fold(HashMap::new(), |mut m, c| {
            *m.entry(c.kind.as_str()).or_insert(0) += 1;
            m
        });

        let desc_parts: Vec<String> = kind_counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect();

        entries.push(MapEntry {
            file: file.path.clone(),
            lang: file.lang.clone(),
            line_count: max_line,
            symbols: pub_symbols,
            description: desc_parts.join(", "),
        });
    }

    let mut result = MapResult {
        results: entries,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

#[cfg(test)]
#[path = "map_advanced_tests.rs"]
mod advanced_tests;
#[cfg(test)]
#[path = "map_tests.rs"]
mod tests;
