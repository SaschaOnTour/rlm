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
mod tests {
    use super::*;
    use crate::models::file::FileRecord;

    const FILE_SIZE: u64 = 4_000;

    fn test_db() -> Database {
        Database::open_in_memory().expect("open in-memory db")
    }

    #[derive(Serialize)]
    struct Payload {
        label: String,
    }

    fn payload(label: &str) -> Payload {
        Payload {
            label: label.into(),
        }
    }

    #[test]
    fn records_single_file_op_and_returns_json_body() {
        let db = test_db();
        let file = FileRecord::new("src/main.rs".into(), "abc".into(), "rust".into(), FILE_SIZE);
        db.upsert_file(&file).unwrap();

        let meta = OperationMeta {
            command: "summarize",
            files_touched: 1,
            alternative: AlternativeCost::SingleFile {
                path: "src/main.rs".into(),
            },
        };

        let response = record_operation(&db, &meta, &payload("hello"));
        assert!(response.body.contains("hello"));
        assert!(!response.body.contains('\n'), "body must be minified JSON");
        assert!(response.tokens_out > 0);

        let rows = db.get_savings_by_command(None).unwrap();
        let entry = rows
            .iter()
            .find(|r| r.command == "summarize")
            .expect("summarize entry recorded");
        assert_eq!(entry.ops, 1);
    }

    #[test]
    fn records_symbol_files_op() {
        let db = test_db();
        let meta = OperationMeta {
            command: "refs",
            files_touched: 3,
            alternative: AlternativeCost::SymbolFiles {
                symbol: "foo".into(),
            },
        };

        let response = record_operation(&db, &meta, &payload("refs-out"));
        assert!(response.body.contains("refs-out"));

        let rows = db.get_savings_by_command(None).unwrap();
        assert!(rows.iter().any(|r| r.command == "refs"));
    }

    #[test]
    fn records_scoped_op_with_prefix() {
        let db = test_db();
        let meta = OperationMeta {
            command: "overview",
            files_touched: 0,
            alternative: AlternativeCost::ScopedFiles {
                prefix: Some("src/".into()),
            },
        };

        let response = record_operation(&db, &meta, &payload("o"));
        assert!(response.body.contains('o'));

        let rows = db.get_savings_by_command(None).unwrap();
        assert!(rows.iter().any(|r| r.command == "overview"));
    }

    #[test]
    fn fixed_alternative_uses_precomputed_token_count() {
        const FIXED_TOKENS: u64 = 500;

        let db = test_db();
        let meta = OperationMeta {
            command: "search",
            files_touched: 2,
            alternative: AlternativeCost::Fixed(FIXED_TOKENS),
        };

        let response = record_operation(&db, &meta, &payload("s"));
        assert!(!response.body.is_empty());

        let rows = db.get_savings_by_command(None).unwrap();
        let row = rows
            .iter()
            .find(|r| r.command == "search")
            .expect("search entry");
        assert_eq!(row.alt_tokens, FIXED_TOKENS);
        // Regression guard: Fixed and AtLeastBody must record through the
        // V2-aware savings::record, not the bare Database::record_savings
        // INSERT. Otherwise rlm_calls/alt_calls stay NULL and the savings
        // report undercounts call overhead.
        assert_eq!(row.rlm_calls, 1);
        assert_eq!(row.alt_calls, 1);
    }

    #[test]
    fn at_least_body_clamps_up_to_body_size() {
        // base smaller than actual body tokens → alt = body tokens.
        const SMALL_BASE: u64 = 1;

        let db = test_db();
        let meta = OperationMeta {
            command: "search",
            files_touched: 1,
            alternative: AlternativeCost::AtLeastBody { base: SMALL_BASE },
        };

        // Use a payload large enough to exceed SMALL_BASE in estimated tokens.
        let big = Payload {
            label: "x".repeat(200),
        };
        let response = record_operation(&db, &meta, &big);

        let rows = db.get_savings_by_command(None).unwrap();
        let row = rows
            .iter()
            .find(|r| r.command == "search")
            .expect("search entry");
        // alt must be at least the body's token count, which exceeds SMALL_BASE.
        assert!(row.alt_tokens >= response.tokens_out);
        assert!(row.alt_tokens > SMALL_BASE);
    }

    #[test]
    fn at_least_body_keeps_base_when_larger_than_body() {
        // base larger than body tokens → alt = base.
        const LARGE_BASE: u64 = 10_000;

        let db = test_db();
        let meta = OperationMeta {
            command: "search",
            files_touched: 1,
            alternative: AlternativeCost::AtLeastBody { base: LARGE_BASE },
        };

        let response = record_operation(&db, &meta, &payload("s"));
        assert!(response.tokens_out < LARGE_BASE);

        let rows = db.get_savings_by_command(None).unwrap();
        let row = rows
            .iter()
            .find(|r| r.command == "search")
            .expect("search entry");
        assert_eq!(row.alt_tokens, LARGE_BASE);
        // Same regression guard as the Fixed test.
        assert_eq!(row.rlm_calls, 1);
        assert_eq!(row.alt_calls, 1);
    }

    #[test]
    fn body_is_always_minified_json() {
        let db = test_db();
        let meta = OperationMeta {
            command: "fixed",
            files_touched: 0,
            alternative: AlternativeCost::Fixed(10),
        };

        let response = record_operation(&db, &meta, &payload("min"));
        // Middleware returns raw minified JSON regardless of the adapter
        // downstream: no newlines, no pretty-printing.
        assert!(!response.body.contains('\n'));
    }
}
