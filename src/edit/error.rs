//! Edit-specific error variants.

use thiserror::Error;

/// Failures that can occur while applying a code edit.
#[derive(Error, Debug)]
pub enum EditError {
    /// The target line is beyond the end of the file.
    #[error("line {line} is beyond file length ({max})")]
    LineOutOfBounds { line: usize, max: usize },
}
