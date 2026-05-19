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

use std::path::Path;

use serde::Serialize;

use crate::application::dto::chunk_dto::ChunkDto;
use crate::application::savings;
use crate::application::symbol::impact::{analyze_impact, filter_impacted_by_parent};
use crate::application::symbol::signature::{SignatureEntry, SignatureResult};
use crate::application::symbol::type_info::TypeInfoResult;
use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::domain::token_budget::{estimate_json_tokens, estimate_output_tokens, TokenEstimate};
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
///
/// `project_root` threads through so the `--metadata` path can run
/// parent-aware ref counting via
/// [`filter_impacted_by_parent`](crate::application::symbol::impact::filter_impacted_by_parent),
/// which inspects source-file content to disambiguate refs by their
/// path-call form (`Foo::new` vs `Bar::new` at column N).
pub fn read_symbol(
    db: &Database,
    project_root: &Path,
    input: &ReadSymbolInput<'_>,
) -> Result<ReadOutput> {
    let chunks = db.get_chunks_by_ident(input.symbol)?;
    if chunks.is_empty() {
        return Err(crate::error::RlmError::SymbolNotFound {
            ident: input.symbol.to_string(),
        });
    }

    let selected_chunks = select_chunks(db, &chunks, input)?;
    let selected_dtos: Vec<ChunkDto> = selected_chunks.iter().map(|c| ChunkDto::from(*c)).collect();

    let body = if input.metadata {
        render_enriched_body(&EnrichedCtx {
            db,
            project_root,
            selected_chunks: &selected_chunks,
            selected_dtos: &selected_dtos,
            symbol: input.symbol,
            parent: input.parent,
        })
    } else {
        serde_json::to_string(&selected_dtos)
            .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
    };

    let tokens_out = estimate_json_tokens(body.len());
    savings::record_read_symbol(db, tokens_out, input.path);
    Ok(ReadOutput { body, tokens_out })
}

/// Apply the file + parent filter, with the parent-respecting
/// fallback when the requested path doesn't match anything. Extracted
/// from `read_symbol` so the metadata-enrichment branch sees a
/// single, already-selected slice.
fn select_chunks<'a>(
    db: &Database,
    chunks: &'a [Chunk],
    input: &ReadSymbolInput<'_>,
) -> Result<Vec<&'a Chunk>> {
    let file_chunks = filter_by_file_and_parent(db, chunks, input.path, input.parent)?;
    // Fallback policy:
    // * no `--parent`: path typos are common, so return every match
    //   for the ident across the project.
    // * with `--parent`: the flag exists to disambiguate (e.g.
    //   `Foo::new` vs `Bar::new`); dropping it on fallback would
    //   silently defeat the disambiguation. Filter the fallback by
    //   parent too, and error out if nothing matches that parent
    //   anywhere.
    if !file_chunks.is_empty() {
        return Ok(file_chunks);
    }
    if let Some(p) = input.parent {
        let parent_matches: Vec<&Chunk> = chunks
            .iter()
            .filter(|c| c.parent.as_deref() == Some(p))
            .collect();
        if parent_matches.is_empty() {
            return Err(crate::error::RlmError::SymbolNotFound {
                ident: format!("{p}::{}", input.symbol),
            });
        }
        return Ok(parent_matches);
    }
    Ok(chunks.iter().collect())
}

/// Bundled context the `--metadata` envelope needs. Keeps
/// `render_enriched_body` and its helpers under the SRP parameter
/// ceiling — every field here is already in scope at the call site
/// inside [`read_symbol`], the struct just groups them.
struct EnrichedCtx<'a, 'c> {
    db: &'a Database,
    project_root: &'a Path,
    /// Chunks the read actually returned (file + parent filtered).
    /// `type_info` derives its pick from this set so a read from
    /// `tests/fixture.rs` can't pick up `src/lib.rs`'s sibling.
    selected_chunks: &'a [&'c Chunk],
    /// Same chunks, wrapped in the wire DTO for serialisation +
    /// signature listing.
    selected_dtos: &'a [ChunkDto<'c>],
    symbol: &'a str,
    parent: Option<&'a str>,
}

/// Serialise the `--metadata` envelope. `type_info` and
/// `signatures` are derived from the chunks the read actually
/// returns, not from a fresh global lookup — so when two files both
/// define `Foo::new` and the user reads from one, those two views
/// describe **that** file's `Foo::new`, not the sibling.
///
/// `signature.ref_count` is the one field that stays parent-wide:
/// refs in the index carry only `target_ident`, so we can't tell
/// which `Foo::new` definition a `Foo::new()` call resolves to.
/// `--parent` lets us drop `Bar::new()` calls (column-aware
/// path-call filter), but among multiple `Foo::new` definitions the
/// callers can't be attributed to one specific definition without
/// flow analysis. See `SignatureResult::ref_count` for the contract
/// the caller sees, and the
/// `ref_count_is_parent_wide_not_definition_scoped` test for the pin.
fn render_enriched_body(ctx: &EnrichedCtx<'_, '_>) -> String {
    let type_info = build_type_info(ctx.db, ctx.selected_chunks, ctx.symbol).ok();
    let signature = build_signature(
        ctx.db,
        ctx.project_root,
        ctx.selected_dtos,
        ctx.symbol,
        ctx.parent,
    )
    .ok();
    #[derive(Serialize)]
    struct Enriched<'a> {
        chunks: &'a [ChunkDto<'a>],
        #[serde(skip_serializing_if = "Option::is_none")]
        type_info: Option<TypeInfoResult>,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<SignatureResult>,
    }
    serde_json::to_string(&Enriched {
        chunks: ctx.selected_dtos,
        type_info,
        signature,
    })
    .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
}

/// Build the `type_info` view for the chunks the read returned. The
/// `src/ > default > fixtures/test` priority lattice still applies
/// — but only within the already-selected set, so a read from
/// `tests/fixture.rs` can't pick up `src/lib.rs`'s `Foo::new`.
fn build_type_info(db: &Database, selected: &[&Chunk], symbol: &str) -> Result<TypeInfoResult> {
    let path_by_id = build_file_path_index(db)?;
    let chunk = pick_priority_chunk(selected, &path_by_id, symbol)?;
    let file = path_by_id.get(&chunk.file_id).cloned().unwrap_or_default();
    Ok(type_info_from_chunk(chunk, symbol, file))
}

/// Construct a `TypeInfoResult` from one chunk. Clones the chunk
/// once and moves its `parent` / `signature` / `content` fields out
/// instead of cloning them individually in the struct literal —
/// rustqual's BP-008 fires on 3+ inline `.clone()` calls, and the
/// whole-chunk clone is honest about what we're doing: we need an
/// owned snapshot of those four fields. `kind` is materialised via
/// `as_str().to_string()` because `ChunkKind`'s string form lives in
/// rustqual-checked static slices, not on the chunk. Token estimate
/// runs after construction since it needs the populated value.
fn type_info_from_chunk(chunk: &Chunk, symbol: &str, file: String) -> TypeInfoResult {
    let owned = chunk.clone();
    let mut result = TypeInfoResult {
        symbol: symbol.to_string(),
        parent: owned.parent,
        kind: owned.kind.as_str().to_string(),
        signature: owned.signature,
        content: owned.content,
        file,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    result
}

/// Build the `signature` view: signatures listed straight from the
/// returned chunks, ref_count parent-scoped when the caller asked for
/// a specific parent.
fn build_signature(
    db: &Database,
    project_root: &Path,
    selected_dtos: &[ChunkDto<'_>],
    symbol: &str,
    parent: Option<&str>,
) -> Result<SignatureResult> {
    let signatures: Vec<SignatureEntry> = selected_dtos
        .iter()
        .filter_map(|c| {
            c.signature.map(|s| SignatureEntry {
                parent: c.parent.map(String::from),
                signature: s.to_string(),
            })
        })
        .collect();
    let ref_count = count_refs(db, project_root, symbol, parent)?;
    let mut result = SignatureResult {
        symbol: symbol.to_string(),
        signatures,
        ref_count,
        tokens: TokenEstimate::default(),
    };
    result.tokens = estimate_output_tokens(&result);
    Ok(result)
}

/// Count refs to `symbol`, parent-aware when set. **Parent-wide,
/// not definition-scoped**: when several files define the same
/// `parent::symbol`, the count is the same from every file's
/// perspective because refs don't carry a per-definition target
/// id. See `SignatureResult::ref_count` for the caller-facing
/// contract.
///
/// Without `parent`: every ref with `target_ident = symbol` counts
/// (matches pre-S31 behaviour for the unfiltered case).
///
/// With `parent`: impact analysis filtered through
/// [`filter_impacted_by_parent`], which uses column-aware source
/// inspection to keep only calls of the path-qualified form
/// (`parent::symbol`). Drops sibling `other_parent::symbol` calls
/// and bare `symbol()` calls.
fn count_refs(
    db: &Database,
    project_root: &Path,
    symbol: &str,
    parent: Option<&str>,
) -> Result<usize> {
    if let Some(p) = parent {
        let mut impact = analyze_impact(db, symbol)?;
        filter_impacted_by_parent(&mut impact, p, project_root)?;
        Ok(impact.count)
    } else {
        Ok(db.get_refs_to(symbol)?.len())
    }
}

/// Build an `O(1)` lookup from `file_id` to file path so the
/// priority pass doesn't run an `O(chunks × files)` scan.
fn build_file_path_index(db: &Database) -> Result<std::collections::HashMap<i64, String>> {
    Ok(db
        .get_all_files()?
        .into_iter()
        .map(|f| (f.id, f.path))
        .collect())
}

/// Pick the highest-priority chunk (`src/` > default > `fixtures` /
/// `test`) from a non-empty slice. Errors with `SymbolNotFound` if
/// the slice is empty — that should never happen on the read path
/// (the caller bailed earlier) but we keep the guarantee explicit.
fn pick_priority_chunk<'a>(
    selected: &[&'a Chunk],
    path_by_id: &std::collections::HashMap<i64, String>,
    symbol: &str,
) -> Result<&'a Chunk> {
    selected
        .iter()
        .copied()
        .min_by_key(|c| match path_by_id.get(&c.file_id) {
            Some(path) => priority_for_path(path),
            None => UNKNOWN_FILE_PRIORITY,
        })
        .ok_or_else(|| crate::error::RlmError::SymbolNotFound {
            ident: symbol.to_string(),
        })
}

/// Priority lattice used by [`pick_priority_chunk`]. Lower wins.
fn priority_for_path(path: &str) -> i32 {
    if path.starts_with("src/") {
        0
    } else if path.contains("fixtures") || path.contains("test") {
        2
    } else {
        1
    }
}

/// Priority value assigned to chunks whose file record is missing,
/// ensuring they sort below src/ (0), default (1), and fixtures/tests (2).
const UNKNOWN_FILE_PRIORITY: i32 = 3;

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
