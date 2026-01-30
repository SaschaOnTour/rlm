pub mod base;
pub mod csharp;
pub mod css;
pub mod go;
pub mod html;
pub mod java;
pub mod javascript;
pub mod php;
pub mod python;
pub mod quality_log;
pub mod rust;
pub mod typescript;

#[cfg(test)]
pub mod test_utils;

use serde::Serialize;

use crate::error::Result;
use crate::models::chunk::{Chunk, Reference};

/// Indicates the quality/completeness of a parse result.
///
/// This is critical for the fallback mechanism: when tree-sitter cannot fully
/// parse modern language features (e.g., Java records, Python match statements),
/// the agent needs to know so it can fall back to standard tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ParseQuality {
    /// Fully parsed without any syntax errors.
    Complete,
    /// Parsed but with syntax errors (tree-sitter `has_error` was true).
    /// The parser still extracted what it could, but some constructs may be missing.
    Partial {
        /// Number of error nodes found in the AST.
        error_count: usize,
        /// Line numbers where errors were detected (1-based).
        error_lines: Vec<u32>,
    },
    /// Parsing completely failed - no usable AST produced.
    Failed {
        /// Human-readable reason for the failure.
        reason: String,
    },
}

impl ParseQuality {
    /// Returns true if the parse was complete without errors.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete)
    }

    /// Returns true if fallback to standard tools is recommended.
    #[must_use]
    pub fn fallback_recommended(&self) -> bool {
        !matches!(self, Self::Complete)
    }
}

/// Extended parse result that includes quality information.
///
/// Use this when you need to know whether the parse was clean or had issues.
#[derive(Debug, Clone, Serialize)]
pub struct ParseResult {
    /// Extracted code chunks.
    pub chunks: Vec<Chunk>,
    /// Extracted references.
    pub refs: Vec<Reference>,
    /// Quality indicator for the parse.
    pub quality: ParseQuality,
}

/// Trait for AST-aware code parsers.
pub trait CodeParser: Send + Sync {
    /// Language identifier (e.g. "rust", "go").
    fn language(&self) -> &str;

    /// Parse source code and extract chunks (functions, structs, classes, etc.).
    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>>;

    /// Extract references (call sites, imports, type usages) from source code.
    fn extract_refs(&self, source: &str, chunks: &[Chunk]) -> Result<Vec<Reference>>;

    /// Parse chunks and extract references in a single pass (avoids double-parsing).
    fn parse_chunks_and_refs(
        &self,
        source: &str,
        file_id: i64,
    ) -> Result<(Vec<Chunk>, Vec<Reference>)> {
        let chunks = self.parse_chunks(source, file_id)?;
        let refs = self.extract_refs(source, &chunks)?;
        Ok((chunks, refs))
    }

    /// Validate syntax: returns true if the source parses without errors.
    fn validate_syntax(&self, source: &str) -> bool;

    /// Parse with quality information for fallback decisions.
    ///
    /// This method provides additional metadata about the parse quality,
    /// enabling agents to detect when they should fall back to standard
    /// tools (grep, read) instead of relying on potentially incomplete AST data.
    ///
    /// Default implementation calls `parse_chunks_and_refs` and uses
    /// `validate_syntax` to determine quality.
    fn parse_with_quality(&self, source: &str, file_id: i64) -> Result<ParseResult> {
        let (chunks, refs) = self.parse_chunks_and_refs(source, file_id)?;
        let quality = if self.validate_syntax(source) {
            ParseQuality::Complete
        } else {
            // Default: just mark as partial with no specific error info
            ParseQuality::Partial {
                error_count: 1,
                error_lines: vec![],
            }
        };
        Ok(ParseResult {
            chunks,
            refs,
            quality,
        })
    }
}

/// Helper to find error nodes in a tree-sitter tree.
/// Returns 1-based line numbers of all ERROR nodes.
#[must_use]
pub fn find_error_lines(root: tree_sitter::Node) -> Vec<u32> {
    let mut errors = Vec::new();
    let mut cursor = root.walk();

    fn visit(cursor: &mut tree_sitter::TreeCursor, errors: &mut Vec<u32>) {
        loop {
            let node = cursor.node();
            if node.is_error() || node.is_missing() {
                let line = node.start_position().row as u32 + 1;
                if !errors.contains(&line) {
                    errors.push(line);
                }
            }
            if cursor.goto_first_child() {
                visit(cursor, errors);
                cursor.goto_parent();
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }

    visit(&mut cursor, &mut errors);
    errors.sort_unstable();
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quality_complete_is_complete() {
        assert!(ParseQuality::Complete.is_complete());
        assert!(!ParseQuality::Complete.fallback_recommended());
    }

    #[test]
    fn parse_quality_partial_recommends_fallback() {
        let partial = ParseQuality::Partial {
            error_count: 2,
            error_lines: vec![5, 10],
        };
        assert!(!partial.is_complete());
        assert!(partial.fallback_recommended());
    }

    #[test]
    fn parse_quality_failed_recommends_fallback() {
        let failed = ParseQuality::Failed {
            reason: "unknown syntax".into(),
        };
        assert!(!failed.is_complete());
        assert!(failed.fallback_recommended());
    }
}
