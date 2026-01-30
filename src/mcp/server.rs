//! MCP server implementation using rmcp.
//!
//! Exposes all rlm-cli functionality as MCP tools over stdio transport.
//! Each tool calls the same core logic as the CLI commands.

use std::path::PathBuf;

use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{tool, ServerHandler, ServiceExt};
use serde::Serialize;

use crate::config::Config;
use crate::db::Database;
use crate::edit::syntax_guard::SyntaxGuard;
use crate::edit::{inserter, replacer};
use crate::indexer;
use crate::operations::{self, parse_position};
use crate::rlm::{batch, grep, partition, peek, summarize};
use crate::search::tree;

use super::tools::{
    BatchParams, CallgraphParams, ContextParams, DepsParams, DiffParams, FilesParams, GrepParams,
    ImpactParams, IndexParams, InsertParams, MapParams, PartitionParams, PatternsParams,
    PeekParams, ReadParams, RefsParams, ReplaceParams, ScopeParams, SearchParams, SignatureParams,
    SummarizeParams, TypeParams, VerifyParams,
};

/// The RLM MCP Server.
///
/// Holds the project root path. The database is opened on-demand for each tool
/// call to avoid lifetime issues with the sqlite connection.
#[derive(Debug, Clone)]
pub struct RlmServer {
    project_root: PathBuf,
}

// ── Helper functions ────────────────────────────────────────────

impl RlmServer {
    fn config(&self) -> Config {
        Config::new(&self.project_root)
    }

    /// Get the database. Returns an error if the index doesn't exist.
    /// Unlike the CLI, MCP does NOT auto-index to avoid blocking on large projects.
    fn ensure_db(&self) -> Result<Database, String> {
        let config = self.config();
        if !config.index_exists() {
            return Err("Index not found. Run 'rlm index .' first before using MCP tools.".into());
        }
        Database::open(&config.db_path).map_err(|e| format!("database error: {e}"))
    }

    fn to_json<T: Serialize>(val: &T) -> String {
        serde_json::to_string(val).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
    }
}

// ── Tool implementations ────────────────────────────────────────

#[tool(tool_box)]
impl RlmServer {
    #[must_use]
    pub fn new(project_root: PathBuf) -> Self {
        Self { project_root }
    }

    // ─── Indexing ───────────────────────────────────────────────

    #[tool(
        description = "Scan and index the codebase into the .rlm/index.db database. Returns file/chunk/ref counts."
    )]
    async fn index(&self, #[tool(aggr)] params: IndexParams) -> String {
        let config = if let Some(path) = &params.path {
            Config::new(path)
        } else {
            self.config()
        };

        if let Err(e) = config.ensure_rlm_dir() {
            return format!("{{\"error\":\"{e}\"}}");
        }

        match indexer::run_index(&config) {
            Ok(result) => {
                let output: operations::IndexOutput = result.into();
                Self::to_json(&output)
            }
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Search ─────────────────────────────────────────────────

    #[tool(
        description = "Full-text search across indexed chunks (symbols and content). Returns matching chunks with content."
    )]
    async fn search(&self, #[tool(aggr)] params: SearchParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        let limit = params.limit.unwrap_or(20);

        match operations::search_chunks(&db, &params.query, limit) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Read ───────────────────────────────────────────────────

    #[tool(
        description = "Read file content. Can read full file, a specific symbol, a markdown section, or a line range."
    )]
    async fn read(&self, #[tool(aggr)] params: ReadParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        let config = self.config();

        if let Some(sym) = &params.symbol {
            match db.get_chunks_by_ident(sym) {
                Ok(chunks) => {
                    let file_chunks: Vec<_> = chunks
                        .iter()
                        .filter(|c| {
                            db.get_all_files().ok().is_some_and(|files| {
                                files
                                    .iter()
                                    .any(|f| f.id == c.file_id && f.path == params.path)
                            })
                        })
                        .collect();

                    if file_chunks.is_empty() {
                        if chunks.is_empty() {
                            return format!("{{\"error\":\"symbol not found: {sym}\"}}");
                        }
                        Self::to_json(&chunks)
                    } else {
                        Self::to_json(&file_chunks)
                    }
                }
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        } else if let Some(heading) = &params.section {
            match db.get_file_by_path(&params.path) {
                Ok(Some(file)) => match db.get_chunks_for_file(file.id) {
                    Ok(chunks) => match chunks.iter().find(|c| c.ident == *heading) {
                        Some(c) => Self::to_json(c),
                        None => format!("{{\"error\":\"section not found: {heading}\"}}"),
                    },
                    Err(e) => format!("{{\"error\":\"{e}\"}}"),
                },
                Ok(None) => format!("{{\"error\":\"file not found: {}\"}}", params.path),
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        } else if let Some(line_range) = &params.lines {
            let full_path = config.project_root.join(&params.path);
            match std::fs::read_to_string(&full_path) {
                Ok(source) => {
                    let all_lines: Vec<&str> = source.lines().collect();
                    let parts: Vec<&str> = line_range.split('-').collect();
                    if parts.len() != 2 {
                        return "{\"error\":\"line range must be START-END\"}".into();
                    }
                    let start: usize = parts[0].parse::<usize>().unwrap_or(1).saturating_sub(1);
                    let end: usize = parts[1]
                        .parse()
                        .unwrap_or(all_lines.len())
                        .min(all_lines.len());
                    let content = all_lines[start..end].join("\n");
                    Self::to_json(&content)
                }
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        } else {
            let full_path = config.project_root.join(&params.path);
            match std::fs::read_to_string(&full_path) {
                Ok(content) => Self::to_json(&content),
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        }
    }

    // ─── Tree ───────────────────────────────────────────────────

    #[tool(
        description = "Display the folder structure with symbol annotations. Shows files and their contained symbols hierarchically."
    )]
    async fn tree(&self) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        match tree::build_tree(&db) {
            Ok(nodes) => Self::to_json(&nodes),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Refs ───────────────────────────────────────────────────

    #[tool(
        description = "Find all usages/call sites of a symbol across the codebase. Returns reference locations with kind (call, import, type_use)."
    )]
    async fn refs(&self, #[tool(aggr)] params: RefsParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::get_refs(&db, &params.symbol) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Signature ──────────────────────────────────────────────

    #[tool(
        description = "Get the signature of a symbol plus the count of all call sites. Useful for refactoring."
    )]
    async fn signature(&self, #[tool(aggr)] params: SignatureParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::get_signature(&db, &params.symbol) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Replace ────────────────────────────────────────────────

    #[tool(
        description = "Replace an AST node (function, struct, etc.) by symbol name. Validates syntax before writing. Use preview=true to see diff without writing."
    )]
    async fn replace(&self, #[tool(aggr)] params: ReplaceParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        if params.preview.unwrap_or(false) {
            match replacer::preview_replace(&db, &params.path, &params.symbol, &params.code) {
                Ok(diff) => {
                    #[derive(Serialize)]
                    struct Out {
                        file: String,
                        symbol: String,
                        old_lines: (u32, u32),
                        old_code: String,
                        new_code: String,
                    }
                    Self::to_json(&Out {
                        file: diff.file,
                        symbol: diff.symbol,
                        old_lines: (diff.start_line, diff.end_line),
                        old_code: diff.old_code,
                        new_code: diff.new_code,
                    })
                }
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        } else {
            let guard = SyntaxGuard::new();
            match replacer::replace_symbol(&db, &params.path, &params.symbol, &params.code, &guard)
            {
                Ok(_) => "{\"ok\":true}".to_string(),
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        }
    }

    // ─── Insert ─────────────────────────────────────────────────

    #[tool(
        description = "Insert code into a file at a specified position (top, bottom, before:N, after:N). Validates syntax before writing."
    )]
    async fn insert(&self, #[tool(aggr)] params: InsertParams) -> String {
        let pos = match parse_position(&params.position) {
            Ok(p) => p,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        let guard = SyntaxGuard::new();
        match inserter::insert_code(&params.path, &pos, &params.code, &guard) {
            Ok(_) => "{\"ok\":true}".to_string(),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Stats ──────────────────────────────────────────────────

    #[tool(
        description = "Get indexing statistics: file count, chunk count, reference count, total bytes, language breakdown, and index age."
    )]
    async fn stats(&self) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::get_stats(&db) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Peek (RLM) ────────────────────────────────────────────

    #[tool(
        description = "Quick structure preview - shows symbols with their kinds and line counts, NO content. Cheapest operation (~50 tokens)."
    )]
    async fn peek(&self, #[tool(aggr)] params: PeekParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        match peek::peek(&db, params.path.as_deref()) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Grep (RLM) ────────────────────────────────────────────

    #[tool(
        description = "Pattern-based search using regex. Returns matching lines with optional context. Use for targeted content finding."
    )]
    async fn grep(&self, #[tool(aggr)] params: GrepParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        let config = self.config();
        let context = params.context.unwrap_or(0);
        match grep::grep(
            &db,
            &params.pattern,
            context,
            params.path.as_deref(),
            &config.project_root,
        ) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Partition (RLM) ────────────────────────────────────────

    #[tool(
        description = "Split a file into chunks using a strategy: 'semantic' (AST boundaries), 'uniform:N' (N lines each), or 'keyword:PATTERN' (regex split)."
    )]
    async fn partition(&self, #[tool(aggr)] params: PartitionParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        let config = self.config();

        let strategy = if params.strategy == "semantic" {
            partition::Strategy::Semantic
        } else if params.strategy.starts_with("uniform:") {
            match params.strategy[8..].parse::<usize>() {
                Ok(n) => partition::Strategy::Uniform(n),
                Err(e) => return format!("{{\"error\":\"invalid chunk size: {e}\"}}"),
            }
        } else if params.strategy.starts_with("keyword:") {
            partition::Strategy::Keyword(params.strategy[8..].to_string())
        } else {
            return "{\"error\":\"strategy must be: semantic, uniform:N, or keyword:PATTERN\"}"
                .into();
        };

        match partition::partition_file(&db, &params.path, &strategy, &config.project_root) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Summarize (RLM) ───────────────────────────────────────

    #[tool(
        description = "Generate a condensed summary of a file: language, line count, symbols with descriptions."
    )]
    async fn summarize(&self, #[tool(aggr)] params: SummarizeParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        match summarize::summarize(&db, &params.path) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Batch (RLM) ───────────────────────────────────────────

    #[tool(
        description = "Run a search query across all indexed files. Returns results grouped by file."
    )]
    async fn batch(&self, #[tool(aggr)] params: BatchParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        let limit = params.limit.unwrap_or(20);
        match batch::batch_search(&db, &params.query, limit) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Diff (RLM) ────────────────────────────────────────────

    #[tool(
        description = "Compare the indexed version of a file/symbol with the current disk version. Shows if content has changed since last index."
    )]
    async fn diff(&self, #[tool(aggr)] params: DiffParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };
        let config = self.config();

        if let Some(sym) = &params.symbol {
            match operations::diff_symbol(&db, &params.path, sym, &config.project_root) {
                Ok(result) => Self::to_json(&result),
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        } else {
            match operations::diff_file(&db, &params.path, &config.project_root) {
                Ok(result) => Self::to_json(&result),
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        }
    }

    // ─── Map ────────────────────────────────────────────────────

    #[tool(
        description = "Project overview: for each file shows language, line count, public symbols, and a brief description. One-call orientation."
    )]
    async fn map(&self, #[tool(aggr)] params: MapParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::build_map(&db, params.path.as_deref()) {
            Ok(entries) => Self::to_json(&entries),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Callgraph ──────────────────────────────────────────────

    #[tool(
        description = "Build call graph for a symbol: who calls it (callers) and what it calls (callees). Returns directed graph edges."
    )]
    async fn callgraph(&self, #[tool(aggr)] params: CallgraphParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::build_callgraph(&db, &params.symbol) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Impact ─────────────────────────────────────────────────

    #[tool(
        description = "Impact analysis: shows all locations that would need updating if a symbol changes. Lists file, containing symbol, line, and reference kind."
    )]
    async fn impact(&self, #[tool(aggr)] params: ImpactParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::analyze_impact(&db, &params.symbol) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Context ────────────────────────────────────────────────

    #[tool(
        description = "Complete understanding of a symbol: body content, signatures, caller count, and callee names. One-call deep understanding."
    )]
    async fn context(&self, #[tool(aggr)] params: ContextParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::build_context(&db, &params.symbol) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Deps ───────────────────────────────────────────────────

    #[tool(
        description = "File dependency analysis: lists all imports/use declarations found in the specified file."
    )]
    async fn deps(&self, #[tool(aggr)] params: DepsParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::get_deps(&db, &params.path) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Scope ──────────────────────────────────────────────────

    #[tool(
        description = "Show what symbols are visible at a specific line in a file. Lists containing scopes and all symbols defined before that line."
    )]
    async fn scope(&self, #[tool(aggr)] params: ScopeParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::get_scope(&db, &params.path, params.line) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Type ───────────────────────────────────────────────────

    #[tool(
        description = "Get type information for a symbol: kind (fn/struct/class/etc.), signature, and full content."
    )]
    async fn type_info(&self, #[tool(aggr)] params: TypeParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::get_type_info(&db, &params.symbol) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Patterns ───────────────────────────────────────────────

    #[tool(
        description = "Find similar implementations in the codebase. Returns matching symbols with their kind, signature, and line count."
    )]
    async fn patterns(&self, #[tool(aggr)] params: PatternsParams) -> String {
        let db = match self.ensure_db() {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        match operations::find_patterns(&db, &params.query) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Files ─────────────────────────────────────────────────────

    #[tool(
        description = "List ALL files in the project (indexed + skipped). Unlike map/tree/search, this shows files with unsupported extensions (.cshtml, .kt, etc.). Use skipped_only=true to find files that need your own tools."
    )]
    async fn files(&self, #[tool(aggr)] params: FilesParams) -> String {
        let filter = operations::FilesFilter {
            path_prefix: params.path,
            skipped_only: params.skipped_only.unwrap_or(false),
            indexed_only: params.indexed_only.unwrap_or(false),
        };

        match operations::list_files(&self.project_root, filter) {
            Ok(result) => Self::to_json(&result),
            Err(e) => format!("{{\"error\":\"{e}\"}}"),
        }
    }

    // ─── Verify ────────────────────────────────────────────────────

    #[tool(
        description = "Verify index integrity. Checks for SQLite corruption, orphan chunks/refs, and files that no longer exist on disk. Use fix=true to auto-repair issues."
    )]
    async fn verify(&self, #[tool(aggr)] params: VerifyParams) -> String {
        let config = self.config();

        if !config.index_exists() {
            return "{\"error\":\"Index not found. Run 'rlm index' first.\"}".to_string();
        }

        let db = match crate::db::Database::open(&config.db_path) {
            Ok(db) => db,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        let report = match operations::verify_index(&db, &config.project_root) {
            Ok(r) => r,
            Err(e) => return format!("{{\"error\":\"{e}\"}}"),
        };

        if params.fix.unwrap_or(false) && !report.is_ok() {
            match operations::fix_integrity(&db, &report) {
                Ok(fix_result) => Self::to_json(&fix_result),
                Err(e) => format!("{{\"error\":\"{e}\"}}"),
            }
        } else {
            Self::to_json(&report)
        }
    }

    // ─── Supported ─────────────────────────────────────────────────

    #[tool(
        description = "List all supported file extensions with their language and parser type (tree-sitter, structural, semantic, plaintext)."
    )]
    async fn supported(&self) -> String {
        Self::to_json(&operations::list_supported())
    }
}

// ── ServerHandler implementation ────────────────────────────────

#[tool(tool_box)]
impl ServerHandler for RlmServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "rlm-cli: The Context Broker for semantic code exploration. \
                 Use progressive disclosure: peek → grep → map → tree → search → read. \
                 For code intelligence: refs, signature, callgraph, impact, context, deps, scope, type, patterns. \
                 For editing: replace (swap AST node), insert (add code). Syntax Guard validates all writes. \
                 Indexing respects .gitignore and excludes hidden files and common build directories. \
                 IMPORTANT: Most tools (tree, map, search, refs, etc.) only show files with supported extensions. \
                 To see ALL files including skipped ones (.cshtml, .kt, etc.), use the 'files' tool. \
                 To see only skipped files: files(skipped_only=true). \
                 IMPORTANT: Check the 'q' field in responses. If 'fallback_recommended' is true, \
                 the file contains syntax that couldn't be fully parsed (e.g. Java records, Python match). \
                 In that case, prefer 'read --lines' or 'grep' over AST-based commands for affected lines."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            ..Default::default()
        }
    }
}

// ── Server startup ──────────────────────────────────────────────

/// Start the MCP server on stdio transport.
pub async fn start_mcp_server() -> crate::error::Result<()> {
    // Initialize tracing to stderr (stdout is the MCP transport)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting rlm-cli MCP server");

    // Determine project root from current working directory
    let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let server = RlmServer::new(project_root);

    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| crate::error::RlmError::Other(format!("MCP server error: {e}")))?;

    tracing::info!("MCP server running on stdio");

    service
        .waiting()
        .await
        .map_err(|e| crate::error::RlmError::Other(format!("MCP server error: {e}")))?;

    Ok(())
}
