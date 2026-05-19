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
use crate::application::edit::write_dispatch::{
    self, DeleteInput, ExtractInput, InsertInput, ReplaceInput, ReplaceMode, ReplaceOutput,
};
use crate::application::index;
use crate::application::middleware::{
    record_file_query, record_operation, record_symbol_query, AlternativeCost, OperationMeta,
    OperationResponse,
};
use crate::application::query::{
    peek, read as read_query, search as search_query, stats as stats_query, supported, tree,
    verify, DetailLevel,
};
use crate::application::symbol::{ContextQuery, ContextWithGraphQuery, ScopeQuery};
use crate::config::Config;
use crate::db::Database;
use crate::error::Result;

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
    /// Open a session rooted at `project_root`. The single entry point
    /// for both adapters: MCP passes the project root captured at
    /// server startup; CLI passes the cwd-discovered root via
    /// `cli::helpers::cwd_project_root`. Runs `ensure_index`
    /// (auto-creates if missing) and the staleness-refresh seam in
    /// `from_db`, so the caller always sees a current index.
    pub fn open(project_root: &Path) -> Result<Self> {
        let config = Config::new(project_root);
        let db = index::ensure_index(&config)?;
        Self::from_db(db, config)
    }

    /// Wrap a freshly-opened DB + config in a session and refresh
    /// staleness. Single seam for both constructors — ensures every
    /// session handed out, regardless of how the DB was acquired,
    /// has been reconciled against the filesystem.
    fn from_db(db: Database, config: Config) -> Result<Self> {
        // Self-healing: pick up external edits (CC-native, vim, git
        // pull, …) before the caller uses the index. Set
        // `RLM_SKIP_REFRESH=1` to skip.
        index::staleness::ensure_index_fresh(&db, &config)?;
        Ok(Self { db, config })
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

    /// Unified `read` entry point: dispatches on
    /// [`read_query::ReadRequest`] to `read_symbol` or `read_section`.
    /// Both branches return a [`read_query::ReadOutput`] with the
    /// pre-serialised body and its token count; not-found cases
    /// bubble up as typed [`RlmError`](crate::error::RlmError)
    /// variants (`SymbolNotFound`, `SectionNotFound`,
    /// `FileNotFound`).
    pub fn read(&self, request: &read_query::ReadRequest<'_>) -> Result<read_query::ReadOutput> {
        match request {
            read_query::ReadRequest::Symbol(input) => read_query::read_symbol(&self.db, input),
            read_query::ReadRequest::Section { path, heading } => {
                read_query::read_section(&self.db, path, heading)
            }
        }
    }

    /// Project-structure overview at one of three detail levels.
    /// Adapters parse the user input into `DetailLevel` at the edge
    /// (clap `ValueEnum` for CLI, `parse_detail_level` for MCP), so
    /// the session itself never sees invalid tokens.
    pub fn overview(
        &self,
        detail: DetailLevel,
        path_filter: Option<&str>,
    ) -> Result<OperationResponse> {
        let meta = OperationMeta {
            command: "overview",
            files_touched: 0,
            alternative: AlternativeCost::ScopedFiles {
                prefix: path_filter.map(String::from),
            },
        };
        match detail {
            DetailLevel::Minimal => {
                let result = peek::peek(&self.db, path_filter)?;
                Ok(record_operation(&self.db, &meta, &result))
            }
            DetailLevel::Standard => {
                let entries = crate::application::query::map::build_map(&self.db, path_filter)?;
                Ok(record_operation(&self.db, &meta, &entries))
            }
            DetailLevel::Tree => {
                let nodes = tree::build_tree(&self.db, path_filter)?;
                Ok(record_operation(&self.db, &meta, &nodes))
            }
        }
    }

    /// Find all usages of a symbol (impact analysis).
    ///
    /// `parent` disambiguates polysemous idents — `rlm refs new` is
    /// noise across an entire codebase, `rlm refs new --parent
    /// OperationResponse` returns just the calls that are
    /// path-qualified to that type. Unfiltered (`parent = None`),
    /// the response always carries the full `target_candidates`
    /// list so the caller can see the polysemy at a glance.
    pub fn refs(&self, symbol: &str, parent: Option<&str>) -> Result<OperationResponse> {
        let mut output = crate::application::symbol::impact::analyze_impact(&self.db, symbol)?;
        if let Some(p) = parent {
            crate::application::symbol::impact::filter_impacted_by_parent(
                &mut output,
                p,
                &self.config.project_root,
            )?;
        }
        let meta = OperationMeta {
            command: "refs",
            files_touched: output.file_count(),
            alternative: AlternativeCost::SymbolFiles {
                symbol: symbol.to_string(),
            },
        };
        Ok(record_operation(&self.db, &meta, &output))
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

    /// Partition a file using a typed strategy. Adapters parse the
    /// DSL (`"semantic"` / `"uniform:N"` / `"keyword:PATTERN"`) at the
    /// edge via `Strategy::from_str`, so the session receives a typed
    /// value.
    pub fn partition(
        &self,
        path: &str,
        strategy: partition::Strategy,
    ) -> Result<OperationResponse> {
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

    /// Truncate the quality log. Companion to [`Self::quality`] —
    /// split out from the read path so the destructive call is a
    /// separate, explicitly invoked operation (MCP exposes it as the
    /// dedicated `quality_clear` tool without the read-only hint).
    pub fn quality_clear(&self) -> Result<stats_query::QualityClearedAck> {
        stats_query::clear_quality_log(&self.config.get_quality_log_path())
    }
}

// ─── Write-side dispatchers ──────────────────────────────────────────

impl RlmSession {
    /// Unified `replace` entry point: branches on [`ReplaceMode`] so
    /// adapters reach exactly one application function. `Preview`
    /// returns the typed [`ReplaceOutput::Preview`] for adapter-side
    /// serialisation; `Apply` returns the pre-serialised envelope
    /// in [`ReplaceOutput::Applied`].
    pub fn replace(&self, input: &ReplaceInput<'_>, mode: ReplaceMode) -> Result<ReplaceOutput> {
        write_dispatch::dispatch_replace(&self.db, &self.config, input, mode)
    }

    /// Delete a symbol (+ sidecar) + reindex + record savings.
    pub fn delete(&self, input: &DeleteInput<'_>) -> Result<String> {
        write_dispatch::dispatch_delete(&self.db, &self.config, input)
    }

    /// Insert code + reindex + record savings.
    pub fn insert(&self, input: &InsertInput<'_>) -> Result<String> {
        write_dispatch::dispatch_insert(&self.db, &self.config.project_root, input)
    }

    /// Extract symbols to another file + reindex both + record savings.
    pub fn extract(&self, input: &ExtractInput<'_>) -> Result<String> {
        write_dispatch::dispatch_extract(&self.db, &self.config, input)
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

/// Re-export of the progress-callback type so adapters building an
/// indexer callback don't reach into `crate::application::index::`.
pub use crate::application::index::ProgressCallback;

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
