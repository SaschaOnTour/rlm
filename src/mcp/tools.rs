//! MCP tool parameter types.
//!
//! Each struct corresponds to the input parameters for one MCP tool.
//! All parameter structs derive `Deserialize` and `JsonSchema` as required by rmcp.

use serde::Deserialize;

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
    /// Optional symbol name to read (instead of full file).
    #[schemars(description = "Symbol name to read (e.g. function/class name)")]
    pub symbol: Option<String>,
    /// Optional markdown section heading to read.
    #[schemars(description = "Markdown section heading to read")]
    pub section: Option<String>,
    /// Optional line range in format 'START-END'.
    #[schemars(description = "Line range to read (format: 'START-END')")]
    pub lines: Option<String>,
}

// ── Tree ────────────────────────────────────────────────────────
// No parameters needed.

// ── Refs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RefsParams {
    /// Symbol name to find references for.
    #[schemars(description = "Symbol name to find all usages/call sites for")]
    pub symbol: String,
}

// ── Signature ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SignatureParams {
    /// Symbol name to get signature for.
    #[schemars(description = "Symbol name to get signature and call sites for")]
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
    #[schemars(description = "Insert position: 'top', 'bottom', 'before:N', or 'after:N'")]
    pub position: String,
}

// ── Stats ───────────────────────────────────────────────────────
// No parameters needed.

// ── Peek ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PeekParams {
    /// Optional path filter (only show files under this path).
    #[schemars(description = "Optional path prefix filter")]
    pub path: Option<String>,
}

// ── Grep ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GrepParams {
    /// Regex pattern to search for.
    #[schemars(description = "Regex pattern to search for in file contents")]
    pub pattern: String,
    /// Number of context lines around matches.
    #[schemars(description = "Number of context lines around matches (default: 0)")]
    pub context: Option<usize>,
    /// Optional path filter.
    #[schemars(description = "Optional path prefix filter")]
    pub path: Option<String>,
}

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

// ── Batch ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct BatchParams {
    /// Search query to run across all files.
    #[schemars(description = "Search query to run across all indexed files")]
    pub query: String,
    /// Maximum results per file.
    #[schemars(description = "Maximum results to return (default: 20)")]
    pub limit: Option<usize>,
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

// ── Map ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MapParams {
    /// Optional path filter.
    #[schemars(description = "Optional path prefix filter")]
    pub path: Option<String>,
}

// ── Callgraph ───────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CallgraphParams {
    /// Symbol to build call graph for.
    #[schemars(description = "Symbol name to build call graph for (callers + callees)")]
    pub symbol: String,
}

// ── Impact ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ImpactParams {
    /// Symbol to analyze impact for.
    #[schemars(description = "Symbol name - shows what breaks if this symbol changes")]
    pub symbol: String,
}

// ── Context ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ContextParams {
    /// Symbol to get full context for.
    #[schemars(
        description = "Symbol name to get complete understanding (body + callers + callees + types)"
    )]
    pub symbol: String,
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

// ── Type ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TypeParams {
    /// Symbol to get type info for.
    #[schemars(description = "Symbol name to get type info for (return type, fields, signature)")]
    pub symbol: String,
}

// ── Patterns ────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PatternsParams {
    /// Query to find similar implementations.
    #[schemars(description = "Query to find similar implementations in the codebase")]
    pub query: String,
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

// ── Verify ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct VerifyParams {
    /// Auto-fix recoverable issues (delete orphans, remove missing files).
    #[schemars(description = "Auto-fix recoverable issues (default: false)")]
    pub fix: Option<bool>,
}

// ── Supported ───────────────────────────────────────────────────
// No parameters needed.
