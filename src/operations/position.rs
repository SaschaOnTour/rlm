//! Position parsing shared between CLI and MCP.

use std::fmt;

use crate::edit::inserter::InsertPosition;

/// Error type for position parsing.
#[derive(Debug, Clone)]
pub enum PositionError {
    /// Invalid position format.
    InvalidFormat,
    /// Invalid line number.
    InvalidLineNumber(String),
}

impl fmt::Display for PositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFormat => {
                write!(f, "position must be: top, bottom, before:N, or after:N")
            }
            Self::InvalidLineNumber(e) => write!(f, "invalid line number: {e}"),
        }
    }
}

impl std::error::Error for PositionError {}

/// Parse a position string into an `InsertPosition`.
///
/// Valid formats:
/// - `"top"` - Insert at the beginning of the file
/// - `"bottom"` - Insert at the end of the file
/// - `"before:N"` - Insert before line N
/// - `"after:N"` - Insert after line N
pub fn parse_position(s: &str) -> Result<InsertPosition, PositionError> {
    match s {
        "top" => Ok(InsertPosition::Top),
        "bottom" => Ok(InsertPosition::Bottom),
        s if s.starts_with("before:") => {
            let n: u32 = s[7..].parse().map_err(|e: std::num::ParseIntError| {
                PositionError::InvalidLineNumber(e.to_string())
            })?;
            Ok(InsertPosition::BeforeLine(n))
        }
        s if s.starts_with("after:") => {
            let n: u32 = s[6..].parse().map_err(|e: std::num::ParseIntError| {
                PositionError::InvalidLineNumber(e.to_string())
            })?;
            Ok(InsertPosition::AfterLine(n))
        }
        _ => Err(PositionError::InvalidFormat),
    }
}
