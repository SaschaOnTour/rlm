//! Savings-recording middleware for operation pipelines.
//!
//! `record_operation` is the single point where an adapter hands a
//! serializable result plus its [`OperationMeta`] and receives back an
//! [`OperationResponse`] containing the JSON body and its token count.
//! The function serializes the result once, records the savings entry
//! against the Claude Code alternative cost model, and returns the raw
//! JSON body so each adapter can apply its own downstream handling
//! (CLI reformats via `Formatter`; MCP guards against truncation before
//! reformatting).
//!
//! Existing `operations::savings::record_*` helpers are reused under the
//! hood for each [`AlternativeCost`] variant so behavior stays identical
//! to the legacy CLI/MCP paths.

use serde::Serialize;

use crate::application::symbol::SymbolQuery;
use crate::application::FileQuery;
use crate::db::Database;
use crate::domain::token_budget::estimate_json_tokens;
use crate::error::Result;
use crate::operations::savings;
use crate::output;

use super::{AlternativeCost, OperationMeta, OperationResponse};

/// Serialize `result`, record savings for `meta`, and return the raw
/// JSON body together with its estimated token count.
///
/// Dispatches on [`OperationMeta::alternative`]:
///
/// - [`AlternativeCost::SingleFile`]  — delegates to `record_file_op`.
/// - [`AlternativeCost::SymbolFiles`] — delegates to `record_symbol_op`.
/// - [`AlternativeCost::ScopedFiles`] — delegates to `record_scoped_op`.
/// - [`AlternativeCost::Fixed`]       — records via `savings::record`
///   (V2-aware legacy wrapper, sets `rlm_calls = alt_calls = 1`)
///   with the caller-supplied alternative token count.
/// - [`AlternativeCost::AtLeastBody`] — same as `Fixed` but clamps the
///   alternative count up to the actual body token count, matching the
///   `base.max(out_tokens)` safeguard used by operations whose native-
///   tool estimate approximates the result size (e.g. `search`).
pub fn record_operation<T: Serialize>(
    db: &Database,
    meta: &OperationMeta,
    result: &T,
) -> OperationResponse {
    let (json, tokens_out) = match &meta.alternative {
        AlternativeCost::SingleFile { path } => {
            let json = savings::record_file_op(db, meta.command, result, path);
            let tokens_out = estimate_json_tokens(json.len());
            (json, tokens_out)
        }
        AlternativeCost::SymbolFiles { symbol } => {
            let json =
                savings::record_symbol_op(db, meta.command, result, symbol, meta.files_touched);
            let tokens_out = estimate_json_tokens(json.len());
            (json, tokens_out)
        }
        AlternativeCost::ScopedFiles { prefix } => {
            let json = savings::record_scoped_op(db, meta.command, result, prefix.as_deref());
            let tokens_out = estimate_json_tokens(json.len());
            (json, tokens_out)
        }
        AlternativeCost::Fixed(alt_tokens) => {
            let json = output::to_json(result);
            let out_tokens = estimate_json_tokens(json.len());
            // Route through savings::record (V2-aware legacy wrapper)
            // rather than Database::record_savings — the latter leaves
            // rlm_calls/alt_calls NULL, which COALESCEs to 0 in the
            // aggregate SQL and undercounts call overhead in reports.
            savings::record(
                db,
                meta.command,
                out_tokens,
                *alt_tokens,
                meta.files_touched,
            );
            (json, out_tokens)
        }
        AlternativeCost::AtLeastBody { base } => {
            let json = output::to_json(result);
            let out_tokens = estimate_json_tokens(json.len());
            let alt_tokens = (*base).max(out_tokens);
            savings::record(db, meta.command, out_tokens, alt_tokens, meta.files_touched);
            (json, out_tokens)
        }
    };

    OperationResponse::new(json, tokens_out)
}

/// Run a [`SymbolQuery`] end-to-end: execute, record savings against the
/// `SymbolFiles` cost model, and return an [`OperationResponse`]. Adapters
/// use this as a one-liner wrapper around every symbol-scoped tool.
pub fn record_symbol_query<Q: SymbolQuery>(
    db: &Database,
    symbol: &str,
) -> Result<OperationResponse> {
    let output = Q::execute(db, symbol)?;
    let meta = OperationMeta {
        command: Q::COMMAND,
        files_touched: Q::file_count(&output),
        alternative: AlternativeCost::SymbolFiles {
            symbol: symbol.to_string(),
        },
    };
    Ok(record_operation(db, &meta, &output))
}

/// Run a [`FileQuery`] end-to-end: execute, record savings against the
/// `SingleFile` cost model, and return an [`OperationResponse`].
/// `files_touched` is always 1 for this pipeline.
pub fn record_file_query<Q: FileQuery>(
    db: &Database,
    query: &Q,
    path: &str,
) -> Result<OperationResponse> {
    let output = query.execute(db, path)?;
    let meta = OperationMeta {
        command: Q::COMMAND,
        files_touched: 1,
        alternative: AlternativeCost::SingleFile {
            path: path.to_string(),
        },
    };
    Ok(record_operation(db, &meta, &output))
}

#[cfg(test)]
#[path = "savings_middleware_scoped_tests.rs"]
mod scoped_tests;
#[cfg(test)]
#[path = "savings_middleware_tests.rs"]
mod tests;
