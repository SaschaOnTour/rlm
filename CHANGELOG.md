# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.1] - 2026-04-21

Quality-focused follow-up to the 0.4.0 architecture refactor. No user-facing
behavior changes — every CLI command and MCP tool produces byte-identical
output. Score under rustqual 1.0.1 is 100.0% across all seven dimensions;
`cargo nextest run` reports 652 passing tests.

### Added

- **`ChunkDto<'a>` / `ReferenceDto<'a>` / `ChunkKindDto<'a>` / `RefKindDto`**
  in `application/dto/chunk_dto.rs`: serde-facing wire-format mirrors of
  the domain types. Adapters convert at the serialization boundary so the
  domain layer stays format-free (`no_serde_derive_in_domain_entities`).
  The DTOs **borrow** every string field (`&'a str` / `Option<&'a str>`)
  from the source `Chunk`/`Reference`, so JSON emission is zero-copy on
  the payload — no per-`Chunk` clone on the read path.
- **`Chunk::stub(file_id)` / `Reference::stub(chunk_id)` constructors**:
  zero/default initializers for struct-update syntax in tests
  (`Chunk { kind, ident, content, ..Chunk::stub(file_id) }`). Replaces the
  14-field boilerplate patterns (BP-009) the duplicate detector was flagging.

### Changed

- **Error types centralised in `error.rs`**: `EditError`, `SetupError`,
  `AtomicWriteError` now live alongside `RlmError` instead of in the
  `application::edit`, `interface::cli::setup`, and
  `infrastructure::filesystem::atomic_writer` submodules. Breaks the
  `error → infrastructure → ... → error` dependency cycle rustqual's
  coupling analyzer flagged. The original modules re-export the types so
  existing call sites stay unchanged.
- **MCP `tool_handlers` split by concern** into four sibling modules
  (`tool_handlers_index`, `tool_handlers_query`, `tool_handlers_read`,
  `tool_handlers_edit`), with `tool_handlers.rs` now a thin re-export facade.
  `InsertInput` groups the former `handle_insert` args to stay under the
  SRP-param ceiling.
- **CLI dispatch unified**: `run_symbol_pipeline::<Q>` and `run_file_pipeline`
  helpers in `cli::helpers` collapse the open-config / open-db / record-op /
  print-body boilerplate that `cmd_refs` / `cmd_summarize` / `cmd_deps` /
  `cmd_scope` each repeated.
- **`Formatter::print` / `Formatter::print_str` → free functions**
  (`output::print` / `output::print_str`). Fixes the `SLM` structural warning
  and the `Formatter` LCOM4 cohesion finding — the struct now only holds
  format + serialize + reformat concerns, no stdout side-effects.
- **`detect_changes` in `application::index::staleness` refactored**: the
  per-file classification body is extracted to `classify_scanned_file`, the
  deletion phase to `detect_deleted_ids`. The orchestrator drops below the
  function-length threshold and each helper is testable in isolation.
- **Test fixtures deduplicated** per-directory in `fixtures_tests.rs` modules
  (`application/symbol/`, `application/query/`, `application/content/`,
  `application/index/`, `interface/shared/`, `db/queries/`, `operations/`).
  Split-sibling test companions (e.g. `impact_tests.rs` +
  `impact_ref_kind_tests.rs`) share their `setup_test_db` /
  `setup_test_db_and_dir` / `setup_indexed` helpers instead of duplicating them.
- **Test files use explicit imports** — all `use super::*;` in `_tests.rs`
  companion files replaced with named imports (wildcard detector).
- **Migration version literals** replaced with `MIGRATIONS[idx].version /
  .name` references in `db::migrations::bootstrap_existing_schema` to drop
  the magic-number warning.

### Fixed

- **Test-companion `#[path]`-wired files misclassified as production code**
  by rustqual's SRP-module analyzer — resolved by upgrading the dev-loop to
  rustqual 1.0.1 (its `ChildPathResolver` now follows `#[path]` overrides
  and picks up inner `#![cfg(test)]` attributes).
- **Stale `// qual:allow(iosp)` markers** on 26 integration-class functions
  that rustqual 1.0.1's IOSP analyzer no longer misclassifies. Removed to
  keep the orphan-suppression count at zero.

## [0.4.0] - 2026-04-19

### Added

- **`rlm setup` command** (P07-01/02/04): automates Claude Code integration. Creates
  `.claude/settings.json` with rlm permissions (16 MCP tools — all except
  `replace` and `insert`, which stay under explicit user control — plus 3 Bash
  patterns) and the `mcpServers.rlm` entry, appends a marker-delimited workflow
  block to `CLAUDE.local.md`, and triggers the initial index. Existing user
  config is preserved via dedup-merge. Flags: `--check` (dry-run), `--remove`
  (clean removal). Idempotent — repeat runs produce byte-identical output.
- **Self-healing index** (P07-05): rlm detects external file changes (CC native
  Edit/Write, vim, `git pull`, ...) automatically at the canonical DB-open seam
  (`cli::helpers::get_db` + `mcp::server_helpers::ensure_db`). Modified, added,
  and deleted files are reconciled before each CLI command / MCP tool call. Set
  `RLM_SKIP_REFRESH=1` to bypass for performance-sensitive scripts.

### Changed

- **Architecture refactoring (P1–P5)**: migrated from a flat module layout to a
  four-layer hexagonal-lite architecture. Breaking change for library
  consumers — see below.
  - **`domain/`** now holds pure entities (`Chunk`, `File`, `Reference`) with
    newtype IDs (`ChunkId`, `FileId`, `ReferenceId`), plus the token-budget
    calculator and savings formulas. No `rusqlite` / `tree_sitter` imports.
  - **`application/`** owns the use-case logic split by concern: `query/`
    (peek/grep/search/map/tree/files/stats/supported/verify), `symbol/`
    (refs/signature/callgraph/impact/context/type_info/scope), `content/`
    (partition/summarize/deps/diff), `edit/` (replace/insert/validator), and
    `index/` (scan/parse/insert pipeline). Three query-trait patterns coexist:
    `SymbolQuery`, `FileQuery`, and the inline `read_section` path.
  - **`interface/`** is the CLI + MCP adapter layer. Shared DTOs
    (`OperationMeta`, `OperationResponse`, `SavingsMiddleware::record_operation`)
    centralise the previous CLI↔MCP duplication. `Formatter` is a copy-by-value
    context object; the `OnceLock<OutputFormat>` singleton is gone.
  - **`infrastructure/`** holds external-system adapters: six `*Repo` traits
    backed by `Database` (`FileRepo`, `ChunkRepo`, `RefRepo`, `SearchRepo`,
    `SavingsRepo`, `StatsRepo`), the tree-sitter primitives (`tree_walker`,
    `query_runner` with `iter_matches`), a consolidated `filesystem::atomic_writer`
    (shared by `setup` and `edit::validator`), and a numbered SQL migration
    runner (`001_base.sql`, `002_savings_v2.sql`, `003_mtime.sql`) that
    replaces the inline `CREATE_SCHEMA` plus probe-and-alter logic.
- **Parser layer** (P4): every per-language parser (rust, typescript, python,
  java, csharp, php, javascript, go, html, css) migrated onto shared
  `tree_walker` helpers and moved its test module into a companion
  `<lang>_tests.rs` file via `#[cfg(test)] #[path] mod tests;`. Per-parser
  production code dropped from 650–1000 lines to 150–310 lines. Tree-sitter
  query sources live as external `.scm` files pulled in via `include_str!`.
- **Error handling**: `RlmError::Other(String)` retired in favour of typed
  variants (`Setup`, `Edit`, `InvalidPattern`, `Mcp`) and a new
  `AtomicWriteError` wired through `#[from]`. `SetupError::AtomicWriteExhausted`
  folded into `AtomicWriteError::Exhausted`.
- **N+1 query elimination** (P3.7): `analyze_impact` and `build_callgraph` now
  pull ref rows with chunk + file context via a single three-way JOIN
  (`Database::get_refs_with_context`) instead of one chunk-lookup and one
  files-list per ref.
- **DB-open consolidation**: extracted `Database::open_required` for the
  "existing-index-only" path (used by `verify`). Canonical read paths now funnel
  through `get_db` / `ensure_db`, giving a single seam for future concerns
  (schema migration, health checks, ...).
- **CLAUDE.local.md EOL handling**: `rlm setup` now detects the file's EOL
  style (CRLF vs LF) and renders / appends / normalises using the matching
  sequence, so Windows-authored files stay all-CRLF after repeat runs.

### Removed

- Transitional `pub use` bridges left behind during the refactoring slices
  (`src/setup.rs`, `src/indexer.rs`, `src/rlm/mod.rs`, `src/search/mod.rs`,
  `src/edit/mod.rs`). Import paths now point directly at
  `crate::interface::cli::setup`, `crate::application::index`,
  `crate::application::query`, `crate::application::content`, and
  `crate::application::edit`. **Breaking for library consumers.** CLI binary
  unchanged. `src/operations/` stays — it still owns the `index`, `refs`, and
  `savings` output modules; a follow-up slice will fold them into the
  application layer.
- `src/db/schema.rs` and its `CREATE_SCHEMA` / `MIGRATE_SAVINGS_V2` /
  `MIGRATE_FILES_MTIME` constants — replaced by the migration runner.

### Performance

- Parser-independent test suite grew from ~528 to 658 tests. Full-reindex on
  self (rlm tree) holds at ~0.58 s user / ~1.85 s wall across the entire
  refactoring, within measurement noise of the pre-refactor baseline.

## [0.3.6] - 2026-04-15

### Added

- **Index progress display**: `rlm index` shows live progress on stderr
  (`Indexing... 342/1205 files`). MCP `index` tool sends `notifications/progress`
  to the client via rmcp for real-time progress tracking.
- **TOON output format**: `--format toon` flag on all CLI commands produces
  Token-Oriented Object Notation — ~30-50% fewer tokens than JSON for
  list-heavy responses. MCP supports TOON via `format = "toon"` in config.
  Uses the standalone [`toon-encode`](https://crates.io/crates/toon-encode)
  crate from crates.io (not an in-repo workspace crate).

### Changed

- **Wrapper standardization**: `build_map` and `build_tree` now return `MapResult` /
  `TreeResult` with token estimates, matching the `{"results": [...], "tokens": {...}}`
  pattern used by search and files
- **Readable keys**: All 102 short serde renames (`"f"`, `"k"`, `"n"`, `"t"`, etc.)
  replaced with readable field names (`"file"`, `"kind"`, `"name"`, `"tokens"`, etc.).
  JSON output is now self-documenting — no key legend needed.
- **Token metadata everywhere**: All operation result types now include `TokenEstimate`
  (refs, context, deps, scope, diff, type_info, signature, callgraph, impact)
- **Accurate token estimates**: PeekResult and Summary token estimates now computed from
  the full serialized response instead of partial serialization

## [0.3.5] - 2026-04-14

### Fixed

- **read --section**: Now correctly filters by ChunkKind — code symbols (structs,
  functions) are no longer returned when using `--section`
- **CLI error format**: `format_error` uses `serde_json::json!` for guaranteed valid JSON
  escaping (previously broke on quotes/newlines in error messages)

### Changed

- **Key unification**: Consistent serde renames across all Serialize structs — no more
  mixing of renamed and unrenamed fields within a struct
  - PeekFile/TreeNode: `"p"` → `"f"` (consistent file path key)
  - RefHit: `col` → `"co"`
  - DepsResult: `imports` → `"im"`
  - ScopeResult: `visible` → `"vis"`
  - ContextResult: `body`/`callers`/`callees` → `"b"`/`"cr"`/`"ce"`
  - CallgraphResult: `callers`/`callees` → `"cr"`/`"ce"`
  - SignatureResult: `refs` → `"rc"`
  - DiffResults: `changed` → `"ch"`

## [0.3.4] - 2026-04-10

### Fixed

- **Context savings accuracy**: `context` now tracks actual file count for `alt_calls`
  (was hardcoded to 0, undercounting CC alternative cost)
- **JSON token estimation**: rlm output tokens now estimated at 2 bytes/token
  (matching CC's JSON tokenization) instead of 4 bytes/token
- **MCP output guard**: Responses exceeding 25K tokens (~50K UTF-8 bytes) are
  now truncated with `{"truncated":true}` instead of being silently cut by CC

### Added

- **Parallel tool execution**: Read-only MCP tools now declare `read_only_hint`
  annotation, enabling Claude Code to run them in parallel
- **CLI concurrency info**: Commands show `[read-only]` / `[write]` in help text;
  users can document parallel-safe commands in their project's `CLAUDE.md`
- **Write preview**: Replace responses include a 10-line content preview of the
  modified symbol; insert responses preview the chunk containing the insertion point
- **Error recovery suggestions**: Error messages now include actionable next steps
  (available sections, search suggestions, index hints)

## [0.3.3] - 2026-04-10

### Changed

- **Savings accuracy**: Alternative token estimators now include ~10% line-number
  overhead (`N\t` prefix) matching Claude Code's actual Read output
- **Overview savings**: `record_scoped_op` now counts actual indexed files for
  `alt_calls` (Glob + Read×N) instead of hardcoded 1
- **CLI read_section**: Uses `record_file_op` (V2 tracking) instead of legacy `record()`

## [0.3.2] - 2026-04-10

### Changed

- **CI auto-release**: Merge to main automatically tags and builds release
  binaries when `Cargo.toml` version has no matching tag

## [0.3.1] - 2026-04-09

### Added

- **Full round-trip savings model**: `SavingsEntry` tracks input tokens, output tokens,
  and call counts for both rlm and Claude Code paths (was: output-only comparison)
- **Replace/Insert savings**: write operations now record token savings (biggest win —
  rlm replace saves ~1600 tokens vs Claude Code's Grep→Read→Edit)
- **Cost estimation in microdollars**: `SavingsEntry::cost_saved_microdollars()` weights
  savings by API pricing ($3/1M input, $15/1M output)
- **Enhanced savings report**: `stats --savings` now shows `rlm_total`, `alt_total`,
  `total_saved`, `input_saved`, `result_saved`, `calls_saved` breakdown

### Changed

- **Savings schema V2**: 4 new columns (`rlm_input_tokens`, `alt_input_tokens`,
  `rlm_calls`, `alt_calls`) with idempotent migration for existing databases
- **Accurate CC call counts**: `record_symbol_op` now models Grep+Read (2 calls),
  `record_file_op` models single Read (1 call), replace models 3 calls, insert models 2
- **`replace_symbol` returns `ReplaceOutcome`**: exposes `old_code_len` for savings
  calculation (callers previously discarded the return value)
- **`print_write_result` returns JSON**: CLI handlers use actual result length for
  savings instead of hardcoded stub
- **DRY refactor**: `record_file_op`, `record_symbol_op`, `record_scoped_op` unified
  via `serialize_and_record_entry` helper; `record_read_symbol` extracted for shared
  CLI/MCP use
- **Migration probe**: `migrate_savings_v2` checks column existence before running
  ALTER statements (avoids 4 failing ALTERs per `Database::open()`)

## [0.3.0] - 2026-04-09

### Changed

- **N+1 query fix**: `read --symbol` now uses single `get_file_by_path` lookup instead
  of loading all files per chunk (O(1) vs O(files × chunks))
- **Auto-reindex after writes**: `replace` and `insert` (MCP + CLI) automatically
  re-index the modified file. Response includes `{reindexed: true, chunks: N, refs: N}`
- **Shared index pipeline**: Extracted `index_source()` used by both bulk indexing and
  single-file reindex (DRY)
- **ReplaceDiff serializable**: `ReplaceDiff` implements `Serialize` manually (backward-compatible `old_lines` format) — eliminated
  duplicate `DiffOutput`/`Out` structs in CLI and MCP handlers
- **Indexer module split**: `indexer.rs` split into `indexer/mod.rs` + `file_processing.rs`
  + `db_insert.rs` (SRP)
- `reindex_single_file` runs in transaction with rollback on failure
- `hash_bytes()` for in-memory hashing (no double disk read)

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

### Security

- **Path traversal validation**: `validate_relative_path()` rejects absolute paths,
  `..` traversal, Windows prefix/drive components, and symlink escapes for all file
  operations (insert, replace, partition, diff, index). Returns canonical paths to
  minimize TOCTOU gap.
- **Partition path traversal**: `partition_file` now validates paths before disk read
- **Index path restriction**: MCP `index` tool now rejects paths outside project root
- **Diff validation order**: `diff_file`/`diff_symbol` now validate and query DB
  before reading from disk
- **DoS prevention**: `uniform:0` partition strategy now rejected (was panic)
- **PDF u32 overflow**: byte offset accumulator changed to u64 with guard
- **Temp file uniqueness**: atomic writes now include process ID in temp file name
- **Quality log path**: custom log paths with `..` or absolute paths rejected

### Changed

- Migrated `serde_yaml` → `serde_yaml_ng` 0.10 (RUSTSEC-2025-0068 — deprecated crate)
- Added StepSecurity Harden Runner to all CI/release workflow jobs
- Scoped GitHub Actions permissions: only release job has `contents: write`

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
