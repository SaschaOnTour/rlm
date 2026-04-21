//! [`RlmSession`] — the single application-layer entry point every
//! adapter (CLI, MCP) routes through.
//!
//! ## Why this module exists
//!
//! Pre-0.5.0 the CLI and MCP handlers each:
//!
//! - imported `crate::db::Database` and kept a raw handle,
//! - called `ensure_index` + `staleness::ensure_index_fresh` themselves,
//! - parsed partition / overview strategy strings inline,
//! - built their own envelopes via `record_operation` / `reindex_with_result`.
//!
//! The rustqual rule `adapters_no_direct_infrastructure` flagged each
//! of those as a layer leak: adapters were doing application-layer
//! work. `RlmSession` owns the DB handle + config and exposes one
//! method per tool. The adapter's job is "parse args → call session
//! method → emit result" — nothing else.
//!
//! ## Method shape
//!
//! Read-side queries that already go through the savings middleware
//! return [`OperationResponse`] (pre-serialised body + token count);
//! adapters reformat with their own [`Formatter`] and write to their
//! output channel.
//!
//! Write-side operations return either a typed diff/outcome (preview)
//! or a pre-serialised JSON envelope (apply, delete, insert, extract)
//! as produced by [`index::reindex_with_result`] + splicers in
//! [`edit::write_dispatch`].
//!
//! Typed dispatchers (`stats`, `quality`, `read_symbol`, `read_section`,
//! `verify`) return domain structs; adapters serialise via formatter.

use std::path::Path;

use crate::application::content::{
    deps::DepsQuery,
    diff::{DiffFileQuery, DiffSymbolQuery},
    partition::{self, PartitionQuery},
    summarize::SummarizeQuery,
};
use crate::application::edit::replacer::ReplaceDiff;
use crate::application::edit::write_dispatch::{
    self, DeleteInput, ExtractInput, InsertInput, ReplaceInput,
};
use crate::application::index;
use crate::application::middleware::{
    record_file_query, record_operation, record_symbol_query, AlternativeCost, OperationMeta,
    OperationResponse,
};
use crate::application::query::{
    files::{self, FilesFilter, FilesResult},
    peek, read as read_query, search as search_query, stats as stats_query, supported, tree,
    verify,
};
use crate::application::symbol::{ContextQuery, ContextWithGraphQuery, RefsQuery, ScopeQuery};
use crate::config::Config;
use crate::db::Database;
use crate::error::{Result, RlmError};

use serde::Serialize;

// ─── Lifecycle ───────────────────────────────────────────────────────

/// A live rlm session — owns the SQLite handle and the project
/// [`Config`]. Every adapter method routes through one of these; no
/// adapter keeps its own [`Database`] reference.
pub struct RlmSession {
    db: Database,
    config: Config,
}

impl RlmSession {
    /// Open a session rooted at the current working directory. Used by
    /// the CLI. Runs `ensure_index` + staleness-refresh so the caller
    /// gets a session whose index is current.
    pub fn open_cwd() -> Result<Self> {
        let config = Config::from_cwd()?;
        Self::open_with_config(config)
    }

    /// Open a session rooted at `project_root`. Used by the MCP server
    /// where the project root is fixed at startup. Runs `ensure_index`
    /// so the index exists (creates on demand) and refreshes
    /// staleness before returning.
    pub fn open(project_root: &Path) -> Result<Self> {
        Self::open_with_config(Config::new(project_root))
    }

    fn open_with_config(config: Config) -> Result<Self> {
        let db = index::ensure_index(&config)?;
        // Self-healing: pick up external edits (CC-native, vim, git
        // pull, …) before the caller uses the index. Set
        // RLM_SKIP_REFRESH=1 to skip.
        index::staleness::ensure_index_fresh(&db, &config)?;
        Ok(Self { db, config })
    }

    /// Open a session only if an index already exists, returning
    /// `None` when the project has not been indexed yet. Used by the
    /// MCP server for every tool call — MCP must not auto-index, but
    /// it **must** honour the same self-healing staleness contract as
    /// [`Self::open`] so every tool sees a current index.
    ///
    /// Regression: `try_open_existing` previously returned the raw
    /// handle without running the staleness refresh. Callers that
    /// relied on the docstring's "refreshes staleness" promise (CLI
    /// parity, external-edit tests) silently saw stale data. The
    /// refresh is now mandatory on this path and verified by
    /// `server_helpers_tests::ensure_session_runs_staleness_check_on_mcp_path`.
    pub fn try_open_existing(project_root: &Path) -> Result<Option<Self>> {
        let config = Config::new(project_root);
        match Database::open_required(&config.db_path) {
            Ok(db) => {
                // Self-healing: pick up external edits (CC-native,
                // vim, git pull, …) before the caller uses the index.
                // Set RLM_SKIP_REFRESH=1 to skip.
                index::staleness::ensure_index_fresh(&db, &config)?;
                Ok(Some(Self { db, config }))
            }
            Err(RlmError::IndexNotFound) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Read-only accessor for the project [`Config`]. Composition-root
    /// type — exposing it does not re-introduce an infrastructure leak.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Shorthand for `session.config().project_root.as_path()`.
    pub fn project_root(&self) -> &Path {
        &self.config.project_root
    }
}

// ─── Static project-level operations (no session required) ───────────

impl RlmSession {
    /// Build a fresh index for `path`. Intentionally a static method:
    /// callers may not yet have a session (indexing IS the act of
    /// building one). After `index_project` returns, callers open a
    /// regular session with [`RlmSession::open`] if they want to run
    /// queries against the new index.
    pub fn index_project(
        path: &Path,
        progress: Option<&index::ProgressCallback>,
    ) -> Result<index::IndexOutput> {
        let config = Config::new(path);
        let result = index::run_index(&config, progress)?;
        Ok(result.into())
    }

    /// List supported file extensions + parser types. Pure function —
    /// no index, no config needed.
    pub fn supported() -> crate::application::query::supported::SupportedResult {
        supported::list_supported()
    }
}

// ─── Read-side queries ───────────────────────────────────────────────

impl RlmSession {
    /// Full-text search with a projection mode (`Full` or `Minimal`).
    pub fn search(
        &self,
        query: &str,
        limit: usize,
        mode: search_query::FieldsMode,
    ) -> Result<OperationResponse> {
        let result = search_query::search_chunks_with_fields(&self.db, query, limit, mode)?;
        let meta = OperationMeta {
            command: "search",
            files_touched: result.file_count,
            alternative: AlternativeCost::AtLeastBody {
                base: result.tokens.output,
            },
        };
        Ok(record_operation(&self.db, &meta, &result))
    }

    /// Read a symbol — returns the pre-serialised body plus token
    /// count (symmetric to [`OperationResponse`]). Adapters emit
    /// `body` through their own formatter.
    pub fn read_symbol(
        &self,
        input: &read_query::ReadSymbolInput<'_>,
    ) -> Result<read_query::ReadSymbolOutput> {
        read_query::read_symbol(&self.db, input)
    }

    /// Read a Markdown section by heading.
    pub fn read_section(&self, path: &str, heading: &str) -> Result<read_query::ReadSectionResult> {
        read_query::read_section(&self.db, path, heading)
    }

    /// Overview at one of three detail levels: `"minimal"`,
    /// `"standard"`, `"tree"`. Invalid detail returns a user-facing
    /// [`RlmError::InvalidPattern`].
    pub fn overview(&self, detail: &str, path_filter: Option<&str>) -> Result<OperationResponse> {
        let meta = OperationMeta {
            command: "overview",
            files_touched: 0,
            alternative: AlternativeCost::ScopedFiles {
                prefix: path_filter.map(String::from),
            },
        };
        match detail {
            "minimal" => {
                let result = peek::peek(&self.db, path_filter)?;
                Ok(record_operation(&self.db, &meta, &result))
            }
            "standard" => {
                let entries = crate::application::query::map::build_map(&self.db, path_filter)?;
                Ok(record_operation(&self.db, &meta, &entries))
            }
            "tree" => {
                let nodes = tree::build_tree(&self.db, path_filter)?;
                Ok(record_operation(&self.db, &meta, &nodes))
            }
            other => Err(RlmError::InvalidPattern {
                pattern: other.to_string(),
                reason: "unknown detail level — use 'minimal', 'standard', or 'tree'".into(),
            }),
        }
    }

    /// Find all usages of a symbol (impact analysis).
    pub fn refs(&self, symbol: &str) -> Result<OperationResponse> {
        record_symbol_query::<RefsQuery>(&self.db, symbol)
    }

    /// Symbol context: body + callers + callees, optionally full
    /// callgraph (with graph = true).
    pub fn context(&self, symbol: &str, graph: bool) -> Result<OperationResponse> {
        if graph {
            record_symbol_query::<ContextWithGraphQuery>(&self.db, symbol)
        } else {
            record_symbol_query::<ContextQuery>(&self.db, symbol)
        }
    }

    /// File-scoped dependencies (imports / use statements).
    pub fn deps(&self, path: &str) -> Result<OperationResponse> {
        record_file_query(&self.db, &DepsQuery, path)
    }

    /// Symbols visible at a given line.
    pub fn scope(&self, path: &str, line: u32) -> Result<OperationResponse> {
        record_file_query(&self.db, &ScopeQuery { line }, path)
    }

    /// Partition a file using a strategy string (`"semantic"`,
    /// `"uniform:N"`, `"keyword:PATTERN"`).
    pub fn partition(&self, path: &str, strategy_str: &str) -> Result<OperationResponse> {
        let strategy = parse_partition_strategy(strategy_str)?;
        let query = PartitionQuery {
            strategy,
            project_root: self.config.project_root.clone(),
        };
        record_file_query(&self.db, &query, path)
    }

    /// Condensed file summary.
    pub fn summarize(&self, path: &str) -> Result<OperationResponse> {
        record_file_query(&self.db, &SummarizeQuery, path)
    }

    /// Diff a file (or single symbol if `symbol` is set) against the
    /// last-indexed version.
    pub fn diff(&self, path: &str, symbol: Option<&str>) -> Result<OperationResponse> {
        let project_root = self.config.project_root.clone();
        if let Some(sym) = symbol {
            let q = DiffSymbolQuery {
                symbol: sym.to_string(),
                project_root,
            };
            record_file_query(&self.db, &q, path)
        } else {
            let q = DiffFileQuery { project_root };
            record_file_query(&self.db, &q, path)
        }
    }

    /// List indexed + skipped files. Filter lives on [`FilesFilter`].
    pub fn files(&self, filter: FilesFilter) -> Result<FilesResult> {
        files::list_files(&self.config.project_root, filter)
    }

    /// Verify index integrity, optionally auto-fixing recoverable
    /// issues. The untagged return payload reflects whichever path
    /// was taken.
    pub fn verify(&self, fix: bool) -> Result<VerifyOutput> {
        let report = verify::verify_index(&self.db, &self.config.project_root)?;
        if fix && !report.is_ok() {
            let fixed = verify::fix_integrity(&self.db, &report)?;
            Ok(VerifyOutput::Fixed(fixed))
        } else {
            Ok(VerifyOutput::Report(report))
        }
    }

    /// Indexing stats or token-savings report.
    pub fn stats(
        &self,
        savings: bool,
        since: Option<&str>,
    ) -> Result<stats_query::StatsDispatchOutput> {
        stats_query::stats_dispatch(&self.db, savings, since)
    }

    /// Inspect parse-quality issues. The log path is derived from the
    /// session's config so adapters don't need to know its layout.
    pub fn quality(&self, flags: stats_query::QualityFlags) -> Result<stats_query::QualityBody> {
        stats_query::quality_dispatch(&self.config.get_quality_log_path(), flags)
    }
}

// ─── Write-side dispatchers ──────────────────────────────────────────

impl RlmSession {
    /// Preview a replace without touching disk.
    pub fn replace_preview(&self, input: &ReplaceInput<'_>) -> Result<ReplaceDiff> {
        write_dispatch::dispatch_replace_preview(&self.db, input)
    }

    /// Apply a replace + reindex + record savings.
    pub fn replace_apply(&self, input: &ReplaceInput<'_>) -> Result<String> {
        write_dispatch::dispatch_replace_apply(&self.db, &self.config, input)
    }

    /// Delete a symbol (+ sidecar) + reindex + record savings.
    pub fn delete(&self, input: &DeleteInput<'_>) -> Result<String> {
        write_dispatch::dispatch_delete(&self.db, &self.config, input)
    }

    /// Insert code + reindex + record savings.
    pub fn insert(&self, input: &InsertInput<'_>) -> Result<String> {
        write_dispatch::dispatch_insert(Some(&self.db), &self.config.project_root, input)
    }

    /// Extract symbols to another file + reindex both + record savings.
    pub fn extract(&self, input: &ExtractInput<'_>) -> Result<String> {
        write_dispatch::dispatch_extract(&self.db, &self.config, input)
    }

    /// Insert without a live session. Used by the MCP `insert` tool
    /// when no index exists yet — the insert still succeeds, the
    /// response advertises `reindexed: false` with a helpful hint.
    pub fn insert_without_index(project_root: &Path, input: &InsertInput<'_>) -> Result<String> {
        write_dispatch::dispatch_insert(None, project_root, input)
    }
}

// ─── Support types ───────────────────────────────────────────────────

/// Result of [`RlmSession::verify`]. Untagged so serde emits the
/// concrete variant (report vs fixed counts) directly.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum VerifyOutput {
    /// `fix = false` or the index was already clean.
    Report(crate::db::queries::VerifyReport),
    /// `fix = true` and issues were fixed.
    Fixed(verify::FixResult),
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Parse the partition strategy DSL into a [`partition::Strategy`].
/// Recognises `"semantic"`, `"uniform:N"`, `"keyword:PATTERN"`.
fn parse_partition_strategy(s: &str) -> Result<partition::Strategy> {
    if s == "semantic" {
        return Ok(partition::Strategy::Semantic);
    }
    if let Some(rest) = s.strip_prefix("uniform:") {
        let n: usize = rest.parse().map_err(|_| RlmError::InvalidPattern {
            pattern: s.to_string(),
            reason: "uniform expects a usize after the colon (e.g. 'uniform:50')".into(),
        })?;
        if n == 0 {
            return Err(RlmError::InvalidPattern {
                pattern: s.to_string(),
                reason: "uniform chunk size must be >= 1".into(),
            });
        }
        return Ok(partition::Strategy::Uniform(n));
    }
    if let Some(rest) = s.strip_prefix("keyword:") {
        return Ok(partition::Strategy::Keyword(rest.to_string()));
    }
    Err(RlmError::InvalidPattern {
        pattern: s.to_string(),
        reason: "strategy must be one of: 'semantic', 'uniform:N', 'keyword:PATTERN'".into(),
    })
}

/// Re-export of the progress-callback type so adapters building an
/// indexer callback don't reach into `crate::application::index::`.
pub use crate::application::index::ProgressCallback;

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
