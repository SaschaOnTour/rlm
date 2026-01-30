use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rlm",
    version,
    about = "The Context Broker - semantic code exploration for AI agents",
    after_help = "NOTE: Most commands (tree, map, search, refs, etc.) only show files with \
                  supported extensions. To see ALL files including skipped ones (.cshtml, .kt, etc.), \
                  use 'rlm files'. To see only skipped files: 'rlm files --skipped-only'."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Scan and index the codebase into .rlm/index.db.
    ///
    /// Respects .gitignore and skips hidden files/directories.
    /// Common build directories (`node_modules`, target, dist, etc.) are excluded.
    /// Files with unsupported extensions are skipped but can be discovered via `rlm files`.
    /// Files with unsupported syntax features are still indexed but marked
    /// with parse quality warnings. Use `rlm stats` to see files with quality issues.
    Index {
        /// Project root directory (default: current directory)
        #[arg(default_value = ".")]
        path: String,
    },

    /// Full-text search across indexed symbols and content
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Read file content, optionally a specific symbol or section
    Read {
        /// File path (project-relative)
        path: String,
        /// Read a specific symbol (function, struct, class)
        #[arg(short, long)]
        symbol: Option<String>,
        /// Read a specific markdown section (heading text)
        #[arg(long)]
        section: Option<String>,
        /// Read specific line range (e.g. "10-20")
        #[arg(long)]
        lines: Option<String>,
    },

    /// Display folder structure with symbol annotations
    Tree,

    /// Find all usages/call sites of a symbol
    Refs {
        /// Symbol name to find references for
        symbol: String,
    },

    /// Get symbol signature and all call sites
    Signature {
        /// Symbol name
        symbol: String,
    },

    /// Replace an AST node by identifier
    Replace {
        /// File path
        path: String,
        /// Symbol to replace
        #[arg(short, long)]
        symbol: String,
        /// New code
        #[arg(short, long)]
        code: String,
        /// Preview only (don't write)
        #[arg(long)]
        preview: bool,
    },

    /// Insert code at a position in a file
    Insert {
        /// File path
        path: String,
        /// Code to insert
        #[arg(short, long)]
        code: String,
        /// Position: top, bottom, before:N, after:N
        #[arg(short, long, default_value = "bottom")]
        position: String,
    },

    /// Show indexing statistics (files, chunks, refs, languages, parse quality warnings)
    Stats,

    /// Quick structure preview (symbols, line counts, NO content)
    Peek {
        /// Path filter (e.g. "src/")
        path: Option<String>,
    },

    /// Pattern match across indexed files
    Grep {
        /// Regex pattern
        pattern: String,
        /// Context lines before/after match
        #[arg(short, long, default_value = "0")]
        context: usize,
        /// Path filter
        #[arg(short, long)]
        path: Option<String>,
    },

    /// Partition a file into chunks
    Partition {
        /// File path
        path: String,
        /// Strategy: uniform:N, semantic, keyword:PATTERN
        #[arg(short, long, default_value = "semantic")]
        strategy: String,
    },

    /// Condensed file summary (symbols + description)
    Summarize {
        /// File path
        path: String,
    },

    /// Parallel search across files
    Batch {
        /// Search query
        query: String,
        /// Limit per file
        #[arg(short, long, default_value = "5")]
        limit: usize,
    },

    /// Show diff between indexed and current content
    Diff {
        /// File path
        path: String,
        /// Symbol to diff
        #[arg(short, long)]
        symbol: Option<String>,
    },

    /// Project overview (file→purpose, key symbols)
    Map {
        /// Path filter
        path: Option<String>,
    },

    /// Full call graph for a symbol
    Callgraph {
        /// Symbol name
        symbol: String,
    },

    /// What breaks if this symbol changes
    Impact {
        /// Symbol name
        symbol: String,
    },

    /// Complete understanding: body + callers + callees + types
    Context {
        /// Symbol name
        symbol: String,
    },

    /// File/module dependency graph
    Deps {
        /// File path
        path: String,
    },

    /// What's visible at a location
    Scope {
        /// File path
        path: String,
        /// Line number
        #[arg(short, long)]
        line: u32,
    },

    /// Type info: return type, fields, required methods
    Type {
        /// Symbol name
        symbol: String,
    },

    /// Find similar implementations in the codebase
    Patterns {
        /// Search pattern or symbol name
        query: String,
    },

    /// Start MCP server (stdio transport)
    Mcp,

    /// Inspect parse quality issues
    Quality {
        /// Show only unknown issues (without tests)
        #[arg(long)]
        unknown_only: bool,
        /// Show all issues (including known)
        #[arg(long)]
        all: bool,
        /// Clear the quality log
        #[arg(long)]
        clear: bool,
        /// Show summary statistics
        #[arg(long)]
        summary: bool,
    },

    /// List ALL files in the project (indexed + skipped).
    ///
    /// Unlike `map` or `tree`, this shows files that were skipped during
    /// indexing due to unsupported extensions. Useful for AI agents that
    /// need complete visibility to use their own tools for non-indexed files.
    Files {
        /// Filter by path prefix (e.g., "src/" or "Views/")
        #[arg(long)]
        path: Option<String>,
        /// Show only skipped/unsupported files
        #[arg(long)]
        skipped_only: bool,
        /// Show only indexed files
        #[arg(long)]
        indexed_only: bool,
    },

    /// Verify index integrity and report issues.
    ///
    /// Checks for:
    /// - `SQLite` database integrity
    /// - Orphan chunks (`file_id` points to deleted file)
    /// - Orphan refs (`chunk_id` points to deleted chunk)
    /// - Indexed files that no longer exist on disk
    Verify {
        /// Auto-fix recoverable issues (delete orphans, remove missing files)
        #[arg(long)]
        fix: bool,
    },

    /// List all supported file extensions and their parser types.
    Supported,
}
