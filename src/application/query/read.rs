//! Read-symbol / read-section queries.
//!
//! Both surfaces (`rlm read <path> --symbol X` and the MCP `read`
//! tool) funnel through these functions. Before 0.5.0 the CLI and MCP
//! each carried ~100 lines of chunk-filter / file-resolve / metadata-
//! enrichment logic — that duplicated orchestration lives here now.
//! Adapters translate typed results to their output channel; they do
//! not filter chunks themselves.
//!
//! Both functions return a pre-serialised JSON body (matching the
//! pattern of [`crate::application::middleware::OperationResponse`])
//! because `ChunkDto` borrows from `Chunk`; serialising immediately
//! keeps lifetimes contained and savings recording in one place.
//! Not-found cases (symbol, section, file) bubble up as typed
//! [`RlmError`](crate::error::RlmError) variants whose `Display`
//! produces the user-facing message — adapters route them through
//! their normal error channel without further branching.

use serde::Serialize;

use crate::application::dto::chunk_dto::ChunkDto;
use crate::application::savings;
use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::domain::token_budget::estimate_json_tokens;
use crate::error::{Result, RlmError, SectionNotFoundError, MAX_SECTION_HINT};

/// Inputs for [`read_symbol`], grouped so the signature stays within
/// the SRP parameter budget and adapters construct the same shape.
pub struct ReadSymbolInput<'a> {
    pub path: &'a str,
    pub symbol: &'a str,
    pub parent: Option<&'a str>,
    pub metadata: bool,
}

/// Raw optional fields both adapters parse out of their CLI / JSON
/// inputs. Built once at the adapter boundary; the facade applies
/// `ReadRequest::from_optional_inputs` to validate and dispatch.
/// Keeps the application's `read_project` signature under the SRP
/// parameter ceiling without duplicating the dispatch knowledge in
/// adapter code.
pub struct ReadInputs<'a> {
    pub path: &'a str,
    pub symbol: Option<&'a str>,
    pub section: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub metadata: bool,
}

/// Unified request for [`crate::application::session::RlmSession::read`].
/// The application layer builds one of these via
/// [`ReadRequest::from_optional_inputs`]; the session matches on
/// the variant and dispatches to [`read_symbol`] or [`read_section`].
pub enum ReadRequest<'a> {
    /// Read a symbol body, optionally enriched with type / signature
    /// metadata.
    Symbol(ReadSymbolInput<'a>),
    /// Read a Markdown section by its heading.
    Section { path: &'a str, heading: &'a str },
}

impl<'a> ReadRequest<'a> {
    /// Build a [`ReadRequest`] from a [`ReadInputs`] bundle.
    /// Enforces the symbol-XOR-section invariant in one place so
    /// neither adapter has to re-implement the validation. The CLI's
    /// clap group already catches the conflict at parse time; the
    /// MCP side receives raw JSON and has no such gate, so the
    /// application layer owns the source of truth.
    pub fn from_optional_inputs(inputs: &ReadInputs<'a>) -> Result<Self> {
        match (inputs.symbol, inputs.section) {
            (Some(_), Some(_)) => Err(RlmError::Config(
                "read accepts exactly one of 'symbol' or 'section', got both".into(),
            )),
            (None, None) => Err(RlmError::Config(
                "read requires 'symbol' or 'section'. Use Claude Code's Read for full files or line ranges.".into(),
            )),
            (Some(sym), None) => Ok(Self::Symbol(ReadSymbolInput {
                path: inputs.path,
                symbol: sym,
                parent: inputs.parent,
                metadata: inputs.metadata,
            })),
            (None, Some(heading)) => Ok(Self::Section {
                path: inputs.path,
                heading,
            }),
        }
    }
}

/// Common shape returned by both `read_symbol` and `read_section`:
/// the pre-serialised JSON body plus its token count. Adapters emit
/// `body` through their own formatter.
#[derive(Debug)]
pub struct ReadOutput {
    pub body: String,
    pub tokens_out: u64,
}

/// Resolve a symbol read. Ambiguity is intentional: "show me every X
/// in this file" returns multiple matches; ambiguity is a write-side
/// concern only (handled inside `replacer`/`extractor`).
pub fn read_symbol(db: &Database, input: &ReadSymbolInput<'_>) -> Result<ReadOutput> {
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
        render_enriched_body(db, &selected, input.symbol, input.parent)
    } else {
        serde_json::to_string(&selected)
            .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
    };

    let tokens_out = estimate_json_tokens(body.len());
    savings::record_read_symbol(db, tokens_out, input.path);
    Ok(ReadOutput { body, tokens_out })
}

/// Serialise the `--metadata` envelope: the parent-filtered chunks
/// plus the same-parent-scoped `type_info` and `signature` lookups.
/// Both metadata calls take the caller's `parent` filter so the
/// enriched view stays consistent with the chunks slice — without
/// this, `rlm read --symbol new --parent Foo --metadata` would leak
/// `Bar::new`'s signature into the response.
fn render_enriched_body(
    db: &Database,
    chunks: &[ChunkDto<'_>],
    symbol: &str,
    parent: Option<&str>,
) -> String {
    let type_info = crate::application::symbol::type_info::get_type_info(db, symbol, parent).ok();
    let signature = crate::application::symbol::signature::get_signature(db, symbol, parent).ok();
    #[derive(Serialize)]
    struct Enriched<'a> {
        chunks: &'a [ChunkDto<'a>],
        #[serde(skip_serializing_if = "Option::is_none")]
        type_info: Option<crate::application::symbol::type_info::TypeInfoResult>,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<crate::application::symbol::signature::SignatureResult>,
    }
    serde_json::to_string(&Enriched {
        chunks,
        type_info,
        signature,
    })
    .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
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

/// Resolve a Markdown section read. Savings are recorded on the
/// success path only; not-found cases bubble up as typed
/// [`RlmError`] variants and bypass savings accounting.
pub fn read_section(db: &Database, path: &str, heading: &str) -> Result<ReadOutput> {
    let Some(file) = db.get_file_by_path(path)? else {
        return Err(RlmError::FileNotFound {
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
        return Err(RlmError::SectionNotFound(SectionNotFoundError {
            heading: heading.to_string(),
            available,
            total,
        }));
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
    Ok(ReadOutput { body, tokens_out })
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
