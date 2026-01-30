// Pedantic lint configuration for the crate.
// Most of these are reasonable but too strict for this codebase:
// - cast_possible_truncation: We work with source files which won't exceed u32 limits
// - cast_sign_loss: Database IDs are always positive in our schema
// - cast_precision_loss: Acceptable for token estimates
// - missing_errors_doc: Error handling is self-evident from Result types
// - missing_panics_doc: Panics are rare and documented inline
// - items_after_statements: Output structs are clearer near their usage
// - too_many_lines: Complex parsers need cohesive logic
// - unused_async: Required by rmcp's #[tool] macro
// - similar_names: Variable naming is contextually clear
// - option_if_let_else: if-let is often clearer
// - fn_params_excessive_bools: CLI flags are naturally boolean
// - trivially_copy_pass_by_ref: Minor optimization not worth churn
// - needless_pass_by_value: Sometimes clearer semantically
// - match_same_arms: Combined arms can reduce readability
// - single_match_else: match is clearer than if-let for pattern matching
// - unnecessary_wraps: Some functions always return Some for API consistency
// - match_wildcard_for_single_variants: Pattern clarity over enum exhaustiveness
// - case_sensitive_file_extension_comparisons: Extensions are normalized upstream
// - manual_let_else: if-let with early return is often clearer in context
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::unused_async,
    clippy::similar_names,
    clippy::option_if_let_else,
    clippy::fn_params_excessive_bools,
    clippy::trivially_copy_pass_by_ref,
    clippy::needless_pass_by_value,
    clippy::match_same_arms,
    clippy::single_match_else,
    clippy::unnecessary_wraps,
    clippy::match_wildcard_for_single_variants,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::manual_let_else
)]

pub mod cli;
pub mod config;
pub mod db;
pub mod edit;
pub mod error;
pub mod indexer;
pub mod ingest;
pub mod mcp;
pub mod models;
pub mod operations;
pub mod rlm;
pub mod search;
