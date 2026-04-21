//! Index output operations shared between CLI and MCP.
//!
//! Provides consistent serialization for index results with all fields.

use serde::Serialize;

use crate::application::index::IndexResult;

/// Serializable index output with all diagnostic fields.
///
/// Ensures CLI and MCP output the same fields.
#[derive(Debug, Clone, Serialize)]
pub struct IndexOutput {
    pub files_scanned: usize,
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub chunks_created: usize,
    pub refs_created: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_unsupported: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_too_large: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_non_utf8: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_io_error: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_unchanged: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub deleted_from_index: usize,
}

// qual:api
#[allow(clippy::trivially_copy_pass_by_ref)] // Required by serde's skip_serializing_if
fn is_zero(v: &usize) -> bool {
    *v == 0
}

impl From<IndexResult> for IndexOutput {
    fn from(result: IndexResult) -> Self {
        Self {
            files_scanned: result.files_scanned,
            files_indexed: result.files_indexed,
            files_skipped: result.files_skipped,
            chunks_created: result.chunks_created,
            refs_created: result.refs_created,
            skipped_unsupported: result.skipped_unsupported,
            skipped_too_large: result.skipped_too_large,
            skipped_non_utf8: result.skipped_non_utf8,
            skipped_io_error: result.skipped_io_error,
            skipped_unchanged: result.skipped_unchanged,
            deleted_from_index: result.deleted_from_index,
        }
    }
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
