//! MCP tool parameter types.
//!
//! Each struct corresponds to the input parameters for one MCP tool.
//! All parameter structs derive `Deserialize` and `JsonSchema` as required by rmcp.

use rmcp::schemars;
use serde::Deserialize;

use crate::edit::inserter::InsertPosition;

// ── Index ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IndexParams {
    /// Path to the project root to index. Defaults to current directory.
    #[schemars(description = "Path to the project root to index (default: current directory)")]
    pub path: Option<String>,
}

// ── Search ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// The search query string.
    #[schemars(description = "Full-text search query")]
    pub query: String,
    /// Maximum number of results to return.
    #[schemars(description = "Maximum results to return (default: 20)")]
    pub limit: Option<usize>,
}

// ── Read ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReadParams {
    /// Relative path to the file.
    #[schemars(description = "Relative path to the file")]
    pub path: String,
    /// Symbol name to read (function, class, struct, etc.)
    #[schemars(description = "Symbol name to read (e.g. function/class name)")]
    pub symbol: Option<String>,
    /// Markdown section heading to read.
    #[schemars(description = "Markdown section heading to read")]
    pub section: Option<String>,
    /// When true and symbol is set, include enriched metadata: kind, signature, visibility, call count.
    #[schemars(
        description = "When true with symbol, include kind/signature/visibility/call-count (default: false)"
    )]
    pub metadata: Option<bool>,
}

// ── Overview ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OverviewParams {
    /// Detail level: 'minimal' (symbol names/kinds/lines only, ~50 tokens),
    /// 'standard' (file map: language, line count, public symbols, descriptions),
    /// 'tree' (directory hierarchy with symbol annotations). Default: 'standard'.
    #[schemars(
        description = "Detail level: 'minimal', 'standard', or 'tree' (default: 'standard')"
    )]
    pub detail: Option<String>,
    /// Optional path prefix filter.
    #[schemars(description = "Optional path prefix filter")]
    pub path: Option<String>,
}

// ── Refs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RefsParams {
    /// Symbol name to find references for.
    #[schemars(description = "Symbol name to find all usages and impact analysis for")]
    pub symbol: String,
}

// ── Replace ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ReplaceParams {
    /// Path to the file containing the symbol.
    #[schemars(description = "Relative path to the file")]
    pub path: String,
    /// Symbol name to replace.
    #[schemars(description = "Symbol name to replace (e.g. function name)")]
    pub symbol: String,
    /// New code to replace the symbol with.
    #[schemars(description = "New code to replace the symbol body with")]
    pub code: String,
    /// If true, preview the change without writing.
    #[schemars(description = "Preview the change without writing (default: false)")]
    pub preview: Option<bool>,
}

// ── Insert ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsertParams {
    /// Path to the file to insert into.
    #[schemars(description = "Relative path to the file")]
    pub path: String,
    /// Code to insert.
    #[schemars(description = "Code to insert")]
    pub code: String,
    /// Position: 'top', 'bottom', 'before:N', or 'after:N'.
    #[schemars(
        with = "String",
        description = "Insert position: 'top', 'bottom', 'before:N', or 'after:N'"
    )]
    pub position: InsertPosition,
}

// ── Stats ───────────────────────────────────────────────────────
// No parameters needed.

// ── Partition ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PartitionParams {
    /// Path to the file to partition.
    #[schemars(description = "Relative path to the file to partition")]
    pub path: String,
    /// Strategy: 'semantic', 'uniform:N', or 'keyword:PATTERN'.
    #[schemars(description = "Partition strategy: 'semantic', 'uniform:N', or 'keyword:PATTERN'")]
    pub strategy: String,
}

// ── Summarize ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SummarizeParams {
    /// Path to the file to summarize.
    #[schemars(description = "Relative path to the file to summarize")]
    pub path: String,
}

// ── Diff ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DiffParams {
    /// Path to the file to diff.
    #[schemars(description = "Relative path to the file")]
    pub path: String,
    /// Optional symbol to diff (instead of full file).
    #[schemars(description = "Optional symbol name to diff")]
    pub symbol: Option<String>,
}

// ── Context ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ContextParams {
    /// Symbol to get full context for.
    #[schemars(
        description = "Symbol name to get complete understanding (body + callers + callees + types)"
    )]
    pub symbol: String,
    /// When true, include full callgraph: caller names + callee names (not just counts).
    #[schemars(description = "Include full callgraph with caller names (default: false)")]
    pub graph: Option<bool>,
}

// ── Deps ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DepsParams {
    /// Path to the file to analyze dependencies.
    #[schemars(description = "Relative path to the file")]
    pub path: String,
}

// ── Scope ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScopeParams {
    /// Path to the file.
    #[schemars(description = "Relative path to the file")]
    pub path: String,
    /// Line number to check scope at.
    #[schemars(description = "Line number to check what is visible at")]
    pub line: u32,
}

// ── Files ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FilesParams {
    /// Optional path prefix filter.
    #[schemars(description = "Filter by path prefix (e.g., 'src/' or 'Views/')")]
    pub path: Option<String>,
    /// Show only skipped files.
    #[schemars(description = "Only show files that were skipped (unsupported extensions)")]
    pub skipped_only: Option<bool>,
    /// Show only indexed files.
    #[schemars(description = "Only show files that were indexed")]
    pub indexed_only: Option<bool>,
}

// ── Savings ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SavingsParams {
    /// Filter savings since date (ISO 8601, e.g. "2026-03-14").
    #[schemars(description = "Filter savings since date (ISO 8601, e.g. '2026-03-14')")]
    pub since: Option<String>,
}

// ── Verify ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VerifyParams {
    /// Auto-fix recoverable issues (delete orphans, remove missing files).
    #[schemars(description = "Auto-fix recoverable issues (default: false)")]
    pub fix: Option<bool>,
}

// ── Supported ───────────────────────────────────────────────────
// No parameters needed.
