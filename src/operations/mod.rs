//! Shared operations used by both CLI and MCP server.
//!
//! Transitional module: during Phase 3 the content of this module is
//! migrating to `crate::application::*`. Re-exports keep the old paths
//! compilable until adapters import directly from the new layer.

pub mod index;
pub mod refs;
pub mod savings;

pub use index::IndexOutput;
pub use refs::{get_refs, RefHit, RefsResult};
pub use savings::get_savings_report;

// Slice 3.2 moved these into `crate::application::query::*`.
pub use crate::application::query::files::{list_files, FilesFilter, FilesResult, FilesSummary};
pub use crate::application::query::map::{build_map, MapEntry};
pub use crate::application::query::search::{search_chunks, SearchHit, SearchResult};
pub use crate::application::query::stats::{get_quality_info, get_stats, QualityInfo, StatsResult};
pub use crate::application::query::supported::{list_supported, ExtensionInfo, SupportedResult};
pub use crate::application::query::verify::{fix_integrity, verify_index, FixResult};

// Slice 3.3 moved these into `crate::application::content::*`.
pub use crate::application::content::deps::{get_deps, DepsResult};
pub use crate::application::content::diff::{
    diff_file, diff_symbol, FileDiffResult, SymbolDiffResult,
};

// Slice 3.6 moved these into `crate::application::symbol::*`.
pub use crate::application::symbol::callgraph::{build_callgraph, CallgraphResult};
pub use crate::application::symbol::context::{build_context, ContextResult};
pub use crate::application::symbol::impact::{analyze_impact, ImpactEntry, ImpactResult};
pub use crate::application::symbol::scope::{get_scope, ScopeResult};
pub use crate::application::symbol::signature::{get_signature, SignatureResult};
pub use crate::application::symbol::type_info::{get_type_info, TypeInfoResult};
