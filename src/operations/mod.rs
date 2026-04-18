//! Shared operations used by both CLI and MCP server.
//!
//! This module extracts common business logic to avoid code duplication
//! between `main.rs` (CLI) and `mcp/server.rs`.

pub mod callgraph;
pub mod context;
pub mod deps;
pub mod diff;
pub mod impact;
pub mod index;
pub mod refs;
pub mod savings;
pub mod scope;
pub mod signature;
pub mod type_info;

pub use callgraph::{build_callgraph, CallgraphResult};
pub use context::{build_context, ContextResult};
pub use deps::{get_deps, DepsResult};
pub use diff::{diff_file, diff_symbol, FileDiffResult, SymbolDiffResult};
pub use impact::{analyze_impact, ImpactEntry, ImpactResult};
pub use index::IndexOutput;
pub use refs::{get_refs, RefHit, RefsResult};
pub use savings::get_savings_report;
pub use scope::{get_scope, ScopeResult};
pub use signature::{get_signature, SignatureResult};
pub use type_info::{get_type_info, TypeInfoResult};

// Slice 3.2 moved these into `crate::application::query::*`. Re-export
// the previous public API here so adapters that still use the
// `operations::*` path keep compiling; later slices update adapters to
// import directly from `application::query`.
pub use crate::application::query::files::{list_files, FilesFilter, FilesResult, FilesSummary};
pub use crate::application::query::map::{build_map, MapEntry};
pub use crate::application::query::search::{search_chunks, SearchHit, SearchResult};
pub use crate::application::query::stats::{get_quality_info, get_stats, QualityInfo, StatsResult};
pub use crate::application::query::supported::{list_supported, ExtensionInfo, SupportedResult};
pub use crate::application::query::verify::{fix_integrity, verify_index, FixResult};
