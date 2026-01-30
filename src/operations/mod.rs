//! Shared operations used by both CLI and MCP server.
//!
//! This module extracts common business logic to avoid code duplication
//! between `main.rs` (CLI) and `mcp/server.rs`.

pub mod callgraph;
pub mod context;
pub mod deps;
pub mod diff;
pub mod files;
pub mod impact;
pub mod index;
pub mod map;
pub mod patterns;
pub mod position;
pub mod refs;
pub mod scope;
pub mod search;
pub mod signature;
pub mod stats;
pub mod supported;
pub mod type_info;
pub mod verify;

pub use callgraph::{build_callgraph, CallgraphResult};
pub use context::{build_context, ContextResult};
pub use deps::{get_deps, DepsResult};
pub use diff::{diff_file, diff_symbol, FileDiffResult, SymbolDiffResult};
pub use files::{list_files, FilesFilter, FilesResult, FilesSummary};
pub use impact::{analyze_impact, ImpactEntry, ImpactResult};
pub use index::IndexOutput;
pub use map::{build_map, MapEntry};
pub use patterns::{find_patterns, PatternHit, PatternsResult};
pub use position::{parse_position, PositionError};
pub use refs::{get_refs, RefHit, RefsResult};
pub use scope::{get_scope, ScopeResult};
pub use search::{search_chunks, SearchHit, SearchResult};
pub use signature::{get_signature, SignatureResult};
pub use stats::{get_quality_info, get_stats, QualityInfo, StatsResult};
pub use supported::{list_supported, ExtensionInfo, SupportedResult};
pub use type_info::{get_type_info, TypeInfoResult};
pub use verify::{fix_integrity, verify_index, FixResult};
