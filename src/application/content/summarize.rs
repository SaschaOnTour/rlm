use serde::Serialize;

use crate::application::FileQuery;
use crate::db::Database;
use crate::domain::token_budget::{estimate_output_tokens, TokenEstimate};
use crate::error::{Result, RlmError};

/// A condensed summary of a file.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub file: String,
    pub lang: String,
    pub line_count: u32,
    pub symbols: Vec<SymbolSummary>,
    pub description: String,
    pub tokens: TokenEstimate,
}

#[derive(Debug, Clone, Serialize)]
pub struct SymbolSummary {
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    pub line_count: u32,
}

/// Generate a condensed summary of a file from its index data.
pub fn summarize(db: &Database, file_path: &str) -> Result<Summary> {
    let file = db
        .get_file_by_path(file_path)?
        .ok_or_else(|| RlmError::FileNotFound {
            path: file_path.into(),
        })?;

    let chunks = db.get_chunks_for_file(file.id)?;

    let max_line = chunks.iter().map(|c| c.end_line).max().unwrap_or(0);

    let symbols: Vec<SymbolSummary> = chunks
        .iter()
        .map(|c| SymbolSummary {
            kind: c.kind.as_str().to_string(),
            name: c.ident.clone(),
            signature: c.signature.clone(),
            visibility: c.visibility.clone(),
            line_count: c.line_count(),
        })
        .collect();

    // Generate a brief description based on the symbols
    let description = generate_description(&file.lang, &symbols);

    let mut result = Summary {
        file: file_path.to_string(),
        lang: file.lang,
        line_count: max_line,
        symbols,
        description,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

fn generate_description(lang: &str, symbols: &[SymbolSummary]) -> String {
    if symbols.is_empty() {
        return format!("{lang} file with no indexed symbols");
    }

    let mut kinds = std::collections::HashMap::new();
    for s in symbols {
        *kinds.entry(s.kind.as_str()).or_insert(0u32) += 1;
    }

    let parts: Vec<String> = kinds
        .iter()
        .map(|(k, v)| {
            if *v == 1 {
                format!("1 {k}")
            } else {
                format!("{v} {k}s")
            }
        })
        .collect();

    let pub_count = symbols
        .iter()
        .filter(|s| {
            s.visibility
                .as_ref()
                .is_some_and(|v| v == "pub" || v == "public")
        })
        .count();

    format!(
        "{lang} file with {}. {pub_count} public symbol(s).",
        parts.join(", ")
    )
}

/// `summarize <path>` as a [`FileQuery`].
pub struct SummarizeQuery;

impl FileQuery for SummarizeQuery {
    type Output = Summary;
    const COMMAND: &'static str = "summarize";

    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output> {
        summarize(db, path)
    }
}

#[cfg(test)]
#[path = "summarize_tests.rs"]
mod tests;
