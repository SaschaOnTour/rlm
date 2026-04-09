# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.1] - 2026-04-09

### Fixed

- **Replace race condition**: `replace_symbol` now verifies file content matches
  indexed chunk before overwriting, preventing silent corruption when files change
  between indexing and replace
- **Path resolution**: `insert_code` and `replace_symbol` now resolve relative paths
  against `project_root` internally (both MCP and CLI), eliminating TOCTOU race
  and lossy path conversion
- **MCP error signaling**: `error_text()` now uses `CallToolResult::error()` with
  proper JSON escaping, setting `is_error=true` for correct failure detection
- Safe byte slicing in replacer (`.get()` instead of direct indexing)
- `SyntaxGuard` deferred until after file validation
- TempDir properly managed in tests (no more `.keep()` leak)

## [0.2.0] - 2026-04-08

### Added

- `overview` MCP tool: consolidated `peek`/`map`/`tree` into one tool with `detail` parameter
  (`minimal`, `standard`, `tree`) and optional `path` prefix filter
- `read(metadata=true)`: enriched symbol reads with type info, signature, visibility, and call count
  (consolidates `type_info` and `signature` tools)
- `context(graph=true)`: full callgraph (caller/callee names) inline with context
  (consolidates `callgraph` tool)
- `refs` now includes full impact analysis (consolidates `impact` tool)
- `build_tree()` accepts optional path prefix filter for scoped tree views
- `rustqual.toml` configuration for code quality analysis
- Shared helper infrastructure: `BaseParser<LanguageConfig>` for all 10 language parsers,
  `Chunk::stub()`, `Partition::new()`, `TreeNode::new()` constructors,
  `ChunkCaptureResult::name()`/`definition()`/`named_definition()` builders
- Savings recording helpers: `record_file_op`, `record_symbol_op`, `record_scoped_op`

### Changed

- Both MCP and CLI consolidated from 27 to 18 tools for better agent tool selection
  - Removed: `grep`, `batch`, `patterns`, `peek`, `map`, `tree`, `type_info`/`type`,
    `signature`, `callgraph`, `impact`
  - CLI retains `mcp` and `quality` as utility-only commands (20 total)
- `read` now requires `--symbol` or `--section`; full-file and line-range reads should
  use Claude Code's native Read tool
- MCP server instructions rewritten for 18-tool surface with 4-tier organization
  (Orient → Search → Analyze → Edit)
- **Code quality: rustqual 100.0% (0 findings)**
  - All functions comply with IOSP (Integration/Operation Segregation Principle)
  - All 10 language parsers migrated to shared `BaseParser<LanguageConfig>` (~1,300 lines of duplication removed)
  - Database queries split into 6 domain modules (files, chunks, refs, search, stats, savings)
  - MCP server split: tool handlers extracted to `tool_handlers.rs`/`tool_handlers_util.rs`
  - CLI handlers split into exploration (`handlers.rs`) + utility (`handlers_util.rs`)
  - All magic numbers replaced with named constants
  - All dead code removed, all struct boilerplate eliminated
  - `SyntaxGuard::validate_and_write` extracted as free function (SRP)

### Removed

- `src/rlm/grep.rs` — redundant with Claude Code's Grep tool
- `src/rlm/batch.rs` — redundant with Claude Code's concurrent tool calls
- `src/operations/patterns.rs` — low-value, search covers this use case
- `cli/output::format_with_tokens` — unused after savings helper refactoring

### Previous

- Token savings tracking: measures how many tokens rlm saves vs Claude Code's native tools
  - New `savings` SQLite table for background logging (best-effort, no perf impact)
  - `rlm stats --savings [--since DATE]` CLI command for savings reports
  - `savings` MCP tool for AI agent access to savings data
  - Per-operation tracking for 22 commands (read_symbol, peek, refs, callgraph, etc.)
  - Comparison logic: single-file, scoped-files, and symbol-files alternatives
- 8 new tests for `InsertPosition` parsing (6 `FromStr` + 2 serde deserialization)

### Changed

- MCP error messages now say "Call the 'index' tool first" instead of referencing CLI commands
- `InsertPosition` is now a first-class type with `FromStr`, `TryFrom<String>`, and `serde::Deserialize` (replaces stringly-typed `position` parameter in CLI and MCP)

### Removed

- `operations::position` module (`parse_position`, `PositionError`) — replaced by `InsertPosition::FromStr`

## [0.1.0] - 2025

### Added

- Initial release of rlm (The Context Broker)
- Progressive disclosure workflow: `overview` → `search` → `read`
- 18 CLI commands + 2 utilities for semantic code exploration
- MCP server integration via `rlm mcp`
- AST-based parsing for 15+ languages using tree-sitter:
  - Rust, Go, Java, C#, Python, PHP
  - JavaScript, TypeScript, TSX
  - HTML, CSS
  - YAML, TOML, JSON
  - Markdown, PDF
- SQLite + FTS5 full-text search with trigram matching
- Surgical code editing with Syntax Guard validation
- Incremental indexing with SHA-256 change detection
- Token budget tracking for all operations
- Call graph and impact analysis
- Reference tracking (calls, imports, type usage)
- Parse quality detection with fallback recommendations

### Architecture

- Single Source of Truth: 16 operations modules
- Shared logic between CLI and MCP server
- ~430 tests with comprehensive coverage
- Zero Clippy warnings

### Documentation

- CLAUDE.md with complete project overview
- Inline documentation for all public APIs
