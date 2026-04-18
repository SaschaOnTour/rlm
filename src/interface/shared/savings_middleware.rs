//! Savings-recording middleware for operation pipelines.
//!
//! `record_operation` is the single point where an adapter hands a
//! serializable result plus its [`OperationMeta`] and receives back an
//! [`OperationResponse`] ready to emit. It serializes the result once
//! (JSON for savings, then formatted for the caller), records the
//! savings entry against the Claude Code alternative cost model, and
//! returns the formatted body together with the estimated token count.
//!
//! Existing `operations::savings::record_*` helpers are reused under the
//! hood for each [`AlternativeCost`] variant so behavior stays identical
//! to the legacy CLI/MCP paths.

use serde::Serialize;

use crate::db::Database;
use crate::domain::token_budget::estimate_json_tokens;
use crate::operations::savings;
use crate::output::{self, Formatter, OutputFormat};

use super::{AlternativeCost, OperationMeta, OperationResponse};

/// Serialize `result`, record savings for `meta`, and produce an
/// [`OperationResponse`] with the body formatted per `formatter`.
///
/// Dispatches on [`OperationMeta::alternative`]:
///
/// - [`AlternativeCost::SingleFile`]  — delegates to `record_file_op`.
/// - [`AlternativeCost::SymbolFiles`] — delegates to `record_symbol_op`.
/// - [`AlternativeCost::ScopedFiles`] — delegates to `record_scoped_op`.
/// - [`AlternativeCost::Fixed`]       — uses the legacy `record` path
///   with the caller-supplied alternative token count.
pub fn record_operation<T: Serialize>(
    db: &Database,
    formatter: Formatter,
    meta: &OperationMeta,
    result: &T,
) -> OperationResponse {
    let json = match &meta.alternative {
        AlternativeCost::SingleFile { path } => {
            savings::record_file_op(db, meta.command, result, path)
        }
        AlternativeCost::SymbolFiles { symbol } => {
            savings::record_symbol_op(db, meta.command, result, symbol, meta.files_touched)
        }
        AlternativeCost::ScopedFiles { prefix } => {
            savings::record_scoped_op(db, meta.command, result, prefix.as_deref())
        }
        AlternativeCost::Fixed(alt_tokens) => {
            let json = output::to_json(result);
            let out_tokens = estimate_json_tokens(json.len());
            let _ = db.record_savings(meta.command, out_tokens, *alt_tokens, meta.files_touched);
            json
        }
    };

    let tokens_out = estimate_json_tokens(json.len());
    let body = if formatter.format() == OutputFormat::Json {
        // JSON path — the helper already returned minified JSON, no
        // reformatting needed and no extra allocation.
        json
    } else {
        formatter.reformat_str(&json).into_owned()
    };

    OperationResponse::new(body, tokens_out)
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
    fn records_single_file_op_and_returns_body() {
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

        let response = record_operation(&db, Formatter::default(), &meta, &payload("hello"));
        assert!(response.body.contains("hello"));
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

        let response = record_operation(&db, Formatter::default(), &meta, &payload("refs-out"));
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

        let response = record_operation(&db, Formatter::default(), &meta, &payload("o"));
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

        let response = record_operation(&db, Formatter::default(), &meta, &payload("s"));
        assert!(!response.body.is_empty());

        let rows = db.get_savings_by_command(None).unwrap();
        let row = rows
            .iter()
            .find(|r| r.command == "search")
            .expect("search entry");
        assert_eq!(row.alt_tokens, FIXED_TOKENS);
    }

    #[test]
    fn pretty_format_reformats_body() {
        let db = test_db();
        let meta = OperationMeta {
            command: "fixed",
            files_touched: 0,
            alternative: AlternativeCost::Fixed(100),
        };

        let pretty = Formatter::new(OutputFormat::Pretty);
        let response = record_operation(&db, pretty, &meta, &payload("multi"));
        // Pretty-printed JSON has at least one newline between the
        // brace and the first field; minified JSON has none.
        assert!(response.body.contains('\n'));
    }

    #[test]
    fn json_format_skips_reformat_allocation() {
        let db = test_db();
        let meta = OperationMeta {
            command: "fixed",
            files_touched: 0,
            alternative: AlternativeCost::Fixed(10),
        };

        let response = record_operation(&db, Formatter::default(), &meta, &payload("min"));
        // Minified JSON body must have no newline.
        assert!(!response.body.contains('\n'));
    }
}
