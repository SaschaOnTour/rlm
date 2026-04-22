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

use crate::error::RlmError;
use std::str::FromStr;

/// Detail level for `rlm overview`. Three fixed levels rather than a
/// free-form `&str`, so each adapter (clap, rmcp) validates at the
/// edge and the session gets a typed input.
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

impl DetailLevel {
    /// Canonical `&str` for this level — the same token the CLI flag
    /// and the MCP JSON schema advertise.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Standard => "standard",
            Self::Tree => "tree",
        }
    }

    /// Parse from optional `&str`, defaulting to `Standard` when the
    /// adapter didn't pass one.
    pub fn from_optional(s: Option<&str>) -> Result<Self, RlmError> {
        match s {
            None => Ok(Self::default()),
            Some(raw) => Self::from_str(raw),
        }
    }
}

impl FromStr for DetailLevel {
    type Err = RlmError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "minimal" => Ok(Self::Minimal),
            "standard" => Ok(Self::Standard),
            "tree" => Ok(Self::Tree),
            other => Err(RlmError::InvalidPattern {
                pattern: other.to_string(),
                reason: "unknown detail level — use 'minimal', 'standard', or 'tree'".into(),
            }),
        }
    }
}

#[cfg(test)]
#[path = "fixtures_tests.rs"]
mod fixtures;
