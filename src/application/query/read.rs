//! Read-symbol / read-section queries.
//!
//! Both surfaces (`rlm read <path> --symbol X` and the MCP `read`
//! tool) funnel through these functions. Before 0.5.0 the CLI and MCP
//! each carried ~100 lines of chunk-filter / file-resolve / metadata-
//! enrichment logic — that duplicated orchestration lives here now.
//! Adapters translate typed results to their output channel; they do
//! not filter chunks themselves.
//!
//! [`read_symbol`] returns a pre-serialised JSON body (matching the
//! pattern of [`crate::application::middleware::OperationResponse`])
//! because `ChunkDto` borrows from `Chunk`; serialising immediately
//! keeps lifetimes contained and savings recording in one place.
//! [`read_section`] returns a typed enum because adapters need to
//! produce structured error messages for the two "not found" cases.

use serde::Serialize;

use crate::application::dto::chunk_dto::ChunkDto;
use crate::application::savings;
use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::domain::token_budget::estimate_json_tokens;
use crate::error::Result;

/// Inputs for [`read_symbol`], grouped so the signature stays within
/// the SRP parameter budget and adapters construct the same shape.
pub struct ReadSymbolInput<'a> {
    pub path: &'a str,
    pub symbol: &'a str,
    pub parent: Option<&'a str>,
    pub metadata: bool,
}

/// Response from [`read_symbol`]: the pre-serialised JSON body plus
/// its token count. Adapters emit `body` through their own formatter.
#[derive(Debug)]
pub struct ReadSymbolOutput {
    pub body: String,
    pub tokens_out: u64,
}

/// Resolve a symbol read. Ambiguity is intentional: "show me every X
/// in this file" returns multiple matches; ambiguity is a write-side
/// concern only (handled inside `replacer`/`extractor`).
pub fn read_symbol(db: &Database, input: &ReadSymbolInput<'_>) -> Result<ReadSymbolOutput> {
    let chunks = db.get_chunks_by_ident(input.symbol)?;
    if chunks.is_empty() {
        return Err(crate::error::RlmError::SymbolNotFound {
            ident: input.symbol.to_string(),
        });
    }

    let file_chunks = filter_by_file_and_parent(db, &chunks, input.path, input.parent)?;
    // Fallback policy:
    // * no `--parent`: path typos are common, so return every match
    //   for the ident across the project.
    // * with `--parent`: the flag exists to disambiguate (e.g.
    //   `Foo::new` vs `Bar::new`); dropping it on fallback would
    //   silently defeat the disambiguation. Filter the fallback by
    //   parent too, and error out if nothing matches that parent
    //   anywhere.
    let selected: Vec<ChunkDto> = if !file_chunks.is_empty() {
        file_chunks.iter().copied().map(ChunkDto::from).collect()
    } else if let Some(p) = input.parent {
        let parent_matches: Vec<&Chunk> = chunks
            .iter()
            .filter(|c| c.parent.as_deref() == Some(p))
            .collect();
        if parent_matches.is_empty() {
            return Err(crate::error::RlmError::SymbolNotFound {
                ident: format!("{p}::{}", input.symbol),
            });
        }
        parent_matches.iter().copied().map(ChunkDto::from).collect()
    } else {
        chunks.iter().map(ChunkDto::from).collect()
    };

    let body = if input.metadata {
        let type_info = crate::application::symbol::type_info::get_type_info(db, input.symbol).ok();
        let signature = crate::application::symbol::signature::get_signature(db, input.symbol).ok();
        #[derive(Serialize)]
        struct Enriched<'a> {
            chunks: &'a [ChunkDto<'a>],
            #[serde(skip_serializing_if = "Option::is_none")]
            type_info: Option<crate::application::symbol::type_info::TypeInfoResult>,
            #[serde(skip_serializing_if = "Option::is_none")]
            signature: Option<crate::application::symbol::signature::SignatureResult>,
        }
        serde_json::to_string(&Enriched {
            chunks: &selected,
            type_info,
            signature,
        })
        .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
    } else {
        serde_json::to_string(&selected)
            .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
    };

    let tokens_out = estimate_json_tokens(body.len());
    savings::record_read_symbol(db, tokens_out, input.path);
    Ok(ReadSymbolOutput { body, tokens_out })
}

fn filter_by_file_and_parent<'a>(
    db: &Database,
    chunks: &'a [Chunk],
    path: &str,
    parent: Option<&str>,
) -> Result<Vec<&'a Chunk>> {
    let file_id = db.get_file_by_path(path)?.map(|f| f.id);
    Ok(chunks
        .iter()
        .filter(|c| file_id.is_some_and(|fid| c.file_id == fid))
        .filter(|c| match parent {
            None => true,
            Some(p) => c.parent.as_deref() == Some(p),
        })
        .collect())
}

/// Outcome of a [`read_section`] lookup. Typed so each adapter can
/// build the appropriate error message without re-implementing the
/// "available sections" hint.
pub enum ReadSectionResult {
    /// Section was found — the DTO is pre-serialised so the body is
    /// ready to emit and the borrowed `ChunkDto<'_>` doesn't escape.
    Found { body: String, tokens_out: u64 },
    /// Section not found. `available` carries the first N section
    /// headings (used by adapters to produce a helpful error hint);
    /// `total` is the real count so adapters can say "N total, first M shown".
    NotFound {
        heading: String,
        available: Vec<String>,
        total: usize,
    },
    /// The file itself is missing from the index.
    FileNotFound { path: String },
}

/// Maximum number of section headings surfaced in a `NotFound` hint.
pub const MAX_SECTION_HINT: usize = 10;

impl ReadSectionResult {
    /// Destructure into `(body, None)` on `Found` or
    /// `(String::new(), Some(error_message))` otherwise. Used by
    /// adapters that want to emit body-or-error without reimplementing
    /// the error-message format per surface.
    pub fn into_body_or_error(self) -> std::result::Result<String, String> {
        match self {
            Self::Found { body, .. } => Ok(body),
            Self::FileNotFound { path } => Err(format!(
                "File not found: {path}. Run 'index' to update, or check 'files' for available paths."
            )),
            Self::NotFound {
                heading,
                available,
                total,
            } => Err(format_section_not_found(&heading, &available, total)),
        }
    }
}

fn format_section_not_found(heading: &str, available: &[String], total: usize) -> String {
    if available.is_empty() {
        return format!("section not found: {heading}. File has no sections.");
    }
    if total > available.len() {
        format!(
            "section not found: {heading}. Available ({total} total, first {MAX_SECTION_HINT}): {}",
            available.join(", ")
        )
    } else {
        format!(
            "section not found: {heading}. Available: {}",
            available.join(", ")
        )
    }
}

/// Resolve a Markdown section read. Savings are recorded on the
/// success path only; `NotFound` doesn't count as a "real" read.
pub fn read_section(db: &Database, path: &str, heading: &str) -> Result<ReadSectionResult> {
    let Some(file) = db.get_file_by_path(path)? else {
        return Ok(ReadSectionResult::FileNotFound {
            path: path.to_string(),
        });
    };

    let chunks = db.get_chunks_for_file(file.id)?;
    let sections: Vec<Chunk> = chunks.into_iter().filter(|c| c.kind.is_section()).collect();

    let Some(hit) = sections.iter().find(|c| c.ident == heading) else {
        let total = sections.len();
        let available = sections
            .iter()
            .take(MAX_SECTION_HINT)
            .map(|c| c.ident.clone())
            .collect();
        return Ok(ReadSectionResult::NotFound {
            heading: heading.to_string(),
            available,
            total,
        });
    };

    let dto = ChunkDto::from(hit);
    let body = serde_json::to_string(&dto)
        .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string());
    let tokens_out = estimate_json_tokens(body.len());
    // Savings accounting: equivalent to Read(path).
    let file_tokens = savings::alternative_single_file(db, path).unwrap_or(tokens_out);
    let entry = crate::domain::savings::SavingsEntry {
        command: "read_section".to_string(),
        rlm_input: 0,
        rlm_output: tokens_out,
        rlm_calls: 1,
        alt_input: 0,
        alt_output: file_tokens,
        alt_calls: 1,
        files_touched: 1,
    };
    savings::record_v2(db, &entry);
    Ok(ReadSectionResult::Found { body, tokens_out })
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
