use clap::{Parser, Subcommand, ValueEnum};

use crate::application::edit::inserter::InsertPosition;

/// Output format for CLI commands.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FormatArg {
    /// Minified JSON (default)
    Json,
    /// Pretty-printed JSON
    Pretty,
    /// Token-Oriented Object Notation — compact tabular format
    Toon,
}

/// Projection mode for `rlm search` hits. Mirrors the application-layer
/// [`crate::application::query::search::FieldsMode`] so clap can parse
/// `--fields <mode>` without dragging clap into the application layer.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum FieldsArg {
    /// Every hit includes the full chunk content (default). Best when
    /// the agent plans to read at least one of the hits.
    Full,
    /// Every hit drops `content`, keeping only id/kind/name/lines. Best
    /// for "does X exist?" / "which files?" where identifiers are
    /// enough; saves ~5k tokens per call vs `full`.
    Minimal,
}

/// Detail level for `rlm overview`. Mirrors the application-layer
/// [`crate::application::query::DetailLevel`] so clap can parse
/// `--detail <level>` without dragging clap into the application layer.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DetailArg {
    /// Symbol names / kinds / lines only (~50 tokens).
    Minimal,
    /// File map: language, line count, public symbols, descriptions.
    Standard,
    /// Directory hierarchy with symbol annotations.
    Tree,
}

#[derive(Parser)]
#[command(
    name = "rlm",
    version,
    about = "The Context Broker - semantic code exploration for AI agents",
    after_help = "CONCURRENCY: Commands marked [read-only] can be run concurrently via parallel \
                  Bash calls once the index exists. If the index is missing, the first read-only \
                  command will create it (a write operation). Commands marked [write] modify files \
                  or the index and must run sequentially.\n\n\
                  NOTE: Most commands only show files with supported extensions. To see ALL files \
                  including skipped ones, use 'rlm files'."
)]
pub struct Cli {
    /// Output format (default: from config, or json)
    #[arg(long, global = true, value_enum)]
    pub format: Option<FormatArg>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// [write] Scan and index the codebase into .rlm/index.db.
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

    /// [read-only] Full-text search across indexed symbols and content
    Search {
        /// Search query
        query: String,
        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        limit: usize,
        /// Which fields to include on each hit. `full` (default) returns
        /// the matched chunk body so the agent doesn't need a follow-up
        /// `rlm read`. `minimal` drops `content` and returns just
        /// id/kind/name/lines — use it when you only need to know
        /// whether a symbol exists or in which files.
        #[arg(long, value_enum, default_value = "full")]
        fields: FieldsArg,
    },

    /// [read-only] Read a specific symbol or markdown section from a file.
    ///
    /// Requires --symbol or --section. For full-file or line-range reads,
    /// use Claude Code's native Read tool.
    /// Use --metadata with --symbol to include type info, signature, visibility, and call count.
    Read {
        /// File path (project-relative)
        path: String,
        /// Read a specific symbol (function, struct, class)
        #[arg(short, long)]
        symbol: Option<String>,
        /// Parent container (enum / struct / impl name) to disambiguate
        /// symbols with identical idents in the same file.
        #[arg(long)]
        parent: Option<String>,
        /// Read a specific markdown section (heading text)
        #[arg(long)]
        section: Option<String>,
        /// Include enriched metadata (kind, signature, visibility, call count)
        #[arg(long)]
        metadata: bool,
    },

    /// [read-only] Project structure overview at three detail levels.
    ///
    /// 'minimal': symbol names/kinds/lines only (~50 tokens).
    /// 'standard' (default): file map with language, line count, public symbols, descriptions.
    /// 'tree': directory hierarchy with symbol annotations.
    Overview {
        /// Detail level.
        #[arg(long, value_enum, default_value = "standard")]
        detail: DetailArg,
        /// Optional path prefix filter (e.g. "src/")
        #[arg(long)]
        path: Option<String>,
    },

    /// [read-only] Find all usages of a symbol and analyze impact.
    ///
    /// Shows every location that would need updating if the symbol changes.
    /// Returns file, containing symbol, line, and reference kind.
    Refs {
        /// Symbol name to find references for
        symbol: String,
    },

    /// [write] Replace an AST node by identifier
    Replace {
        /// File path
        path: String,
        /// Symbol to replace
        #[arg(short, long)]
        symbol: String,
        /// Parent container (enum / struct / impl name) to disambiguate
        /// symbols with identical idents in the same file.
        #[arg(long)]
        parent: Option<String>,
        /// New code (inline). Prefer `--code-stdin` or `--code-file` for
        /// bodies containing apostrophes / byte literals / lifetimes.
        #[arg(short, long, group = "replace_code_src")]
        code: Option<String>,
        /// Read the new code from stdin. Typical: `cat patch.rs | rlm replace …`.
        #[arg(long, group = "replace_code_src")]
        code_stdin: bool,
        /// Read the new code from a file.
        #[arg(long, value_name = "PATH", group = "replace_code_src")]
        code_file: Option<String>,
        /// Preview only (don't write)
        #[arg(long)]
        preview: bool,
    },

    /// [write] Delete an AST node by identifier
    Delete {
        /// File path
        path: String,
        /// Symbol to delete
        #[arg(short, long)]
        symbol: String,
        /// Parent container (enum / struct / impl name) to disambiguate
        /// symbols with identical idents in the same file.
        #[arg(long)]
        parent: Option<String>,
        /// Preserve the doc-comment / attribute sidecar above the
        /// symbol. Default (off): the sidecar is removed alongside
        /// the symbol so orphan comments don't linger.
        #[arg(long)]
        keep_docs: bool,
    },

    /// [write] Extract symbols from one file into a new (or existing) file
    Extract {
        /// Source file path (project-relative)
        path: String,
        /// Comma-separated list of symbol names to move
        #[arg(long, value_delimiter = ',')]
        symbols: Vec<String>,
        /// Destination file path. Created if it doesn't exist;
        /// appended to otherwise.
        #[arg(long)]
        to: String,
        /// Parent container for disambiguation when a symbol name
        /// is shared across multiple chunks in the source file.
        #[arg(long)]
        parent: Option<String>,
    },

    /// [write] Insert code at a position in a file
    Insert {
        /// File path
        path: String,
        /// Code to insert (inline). Prefer `--code-stdin` or
        /// `--code-file` for non-trivial bodies.
        #[arg(short, long, group = "insert_code_src")]
        code: Option<String>,
        /// Read the code from stdin.
        #[arg(long, group = "insert_code_src")]
        code_stdin: bool,
        /// Read the code from a file.
        #[arg(long, value_name = "PATH", group = "insert_code_src")]
        code_file: Option<String>,
        /// Position: top, bottom, before:N, after:N
        #[arg(short, long, default_value = "bottom")]
        position: InsertPosition,
    },

    /// [read-only] Show indexing statistics (files, chunks, refs, languages, parse quality warnings)
    Stats {
        /// Show token savings report
        #[arg(long)]
        savings: bool,
        /// Filter savings since date (ISO 8601, e.g. "2026-03-14")
        #[arg(long)]
        since: Option<String>,
    },

    /// [read-only] Partition a file into chunks
    Partition {
        /// File path
        path: String,
        /// Strategy: uniform:N, semantic, keyword:PATTERN
        #[arg(short, long, default_value = "semantic")]
        strategy: String,
    },

    /// [read-only] Condensed file summary (symbols + description)
    Summarize {
        /// File path
        path: String,
    },

    /// [read-only] Show diff between indexed and current content
    Diff {
        /// File path
        path: String,
        /// Symbol to diff
        #[arg(short, long)]
        symbol: Option<String>,
    },

    /// [read-only] Complete understanding of a symbol: body + callers + callees + types.
    ///
    /// Use --graph to include full callgraph with caller/callee names.
    Context {
        /// Symbol name
        symbol: String,
        /// Include full callgraph (caller + callee names)
        #[arg(long)]
        graph: bool,
    },

    /// [read-only] File/module dependency graph
    Deps {
        /// File path
        path: String,
    },

    /// [read-only] What's visible at a location
    Scope {
        /// File path
        path: String,
        /// Line number
        #[arg(short, long)]
        line: u32,
    },

    /// Start MCP server (stdio transport)
    Mcp,

    /// [read-only, write with --clear] Inspect parse quality issues
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

    /// [read-only] List ALL files in the project (indexed + skipped).
    ///
    /// Unlike `overview`, this shows files that were skipped during
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

    /// [read-only, write with --fix] Verify index integrity and report issues.
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

    /// [read-only] List all supported file extensions and their parser types.
    Supported,

    /// [write] Configure Claude Code integration for this project.
    ///
    /// Creates/updates `.claude/settings.json` with rlm permissions and the
    /// `mcpServers.rlm` entry, and appends an rlm workflow block to
    /// `CLAUDE.local.md`. If no index exists, runs `rlm index` once.
    ///
    /// Existing user entries are preserved — permission arrays are dedup-
    /// merged and only the `mcpServers.rlm` key is overwritten. Re-running
    /// produces identical output (idempotent).
    Setup {
        /// Dry-run: show planned changes, write nothing to disk.
        #[arg(long, conflicts_with = "remove")]
        check: bool,
        /// Remove all rlm configuration entries.
        #[arg(long, conflicts_with = "check")]
        remove: bool,
    },
}
