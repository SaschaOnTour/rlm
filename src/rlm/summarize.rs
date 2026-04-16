use serde::Serialize;

use crate::db::Database;
use crate::error::{Result, RlmError};
use crate::models::token_estimate::{estimate_output_tokens, TokenEstimate};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chunk::{Chunk, ChunkKind};
    use crate::models::file::FileRecord;

    /// File size in bytes for the test file record.
    const TEST_FILE_SIZE: u64 = 500;
    /// End line of each test chunk (symbol spans 10 lines).
    const CHUNK_END_LINE: u32 = 10;
    /// End byte offset of each test chunk.
    const CHUNK_END_BYTE: u32 = 100;

    #[test]
    fn summarize_file() {
        let db = Database::open_in_memory().unwrap();
        let f = FileRecord::new(
            "src/lib.rs".into(),
            "h".into(),
            "rust".into(),
            TEST_FILE_SIZE,
        );
        let fid = db.upsert_file(&f).unwrap();

        for (name, kind, vis) in [
            ("Config", ChunkKind::Struct, "pub"),
            ("new", ChunkKind::Method, "pub"),
            ("helper", ChunkKind::Function, "private"),
        ] {
            db.insert_chunk(&Chunk {
                id: 0,
                file_id: fid,
                start_line: 1,
                end_line: CHUNK_END_LINE,
                start_byte: 0,
                end_byte: CHUNK_END_BYTE,
                kind,
                ident: name.into(),
                parent: None,
                signature: None,
                visibility: Some(vis.into()),
                ui_ctx: None,
                doc_comment: None,
                attributes: None,
                content: "...".into(),
            })
            .unwrap();
        }

        let summary = summarize(&db, "src/lib.rs").unwrap();
        assert_eq!(summary.file, "src/lib.rs");
        assert_eq!(summary.symbols.len(), 3);
        assert!(summary.description.contains("rust"));
    }
}
