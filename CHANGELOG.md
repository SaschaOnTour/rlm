# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2025

### Added

- Initial release of rlm (The Context Broker)
- Progressive disclosure workflow: `peek` → `grep` → `partition` → `read`
- 27 CLI commands for semantic code exploration
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
