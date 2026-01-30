//! Index output operations shared between CLI and MCP.
//!
//! Provides consistent serialization for index results with all fields.

use serde::Serialize;

use crate::indexer::IndexResult;

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
mod tests {
    use super::*;

    #[test]
    fn index_output_from_result() {
        let result = IndexResult {
            files_scanned: 100,
            files_indexed: 80,
            files_skipped: 20,
            chunks_created: 500,
            refs_created: 200,
            skipped_unsupported: 10,
            skipped_too_large: 2,
            skipped_non_utf8: 3,
            skipped_io_error: 1,
            skipped_unchanged: 4,
            deleted_from_index: 5,
        };

        let output: IndexOutput = result.into();
        assert_eq!(output.files_scanned, 100);
        assert_eq!(output.files_indexed, 80);
        assert_eq!(output.skipped_unsupported, 10);
    }

    #[test]
    fn index_output_serialization_skips_zeros() {
        let result = IndexResult {
            files_scanned: 10,
            files_indexed: 10,
            files_skipped: 0,
            chunks_created: 50,
            refs_created: 20,
            skipped_unsupported: 0,
            skipped_too_large: 0,
            skipped_non_utf8: 0,
            skipped_io_error: 0,
            skipped_unchanged: 0,
            deleted_from_index: 0,
        };

        let output: IndexOutput = result.into();
        let json = serde_json::to_string(&output).unwrap();

        // Zero fields should not appear in JSON
        assert!(!json.contains("skipped_unsupported"));
        assert!(!json.contains("skipped_too_large"));
        assert!(!json.contains("deleted_from_index"));
    }
}
