//! Query use cases — read-only retrievals across the indexed project.

pub mod files;
pub mod map;
pub mod peek;
pub mod read;
pub mod search;
pub mod stats;
pub mod supported;
pub mod tree;
pub mod verify;

/// Detail level for `rlm overview`. Three fixed levels rather than a
/// free-form `&str`, so each adapter (clap, rmcp) validates at the
/// edge and the session gets a typed input. Parsing from the user-
/// facing string happens in the adapter — `From<DetailArg>` on the
/// CLI side, `parse_detail_level` on the MCP side; the application
/// layer doesn't own a `FromStr` impl on purpose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DetailLevel {
    /// Symbol names / kinds / lines only (~50 tokens).
    Minimal,
    /// File map: language, line count, public symbols, descriptions.
    #[default]
    Standard,
    /// Directory hierarchy with symbol annotations.
    Tree,
}

#[cfg(test)]
#[path = "fixtures_tests.rs"]
mod fixtures;
