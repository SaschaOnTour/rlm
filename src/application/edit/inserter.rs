use serde::Deserialize;

use super::error::EditError;
use super::validator::{validate_and_write, SyntaxGuard};
use crate::error::{Result, RlmError};
use crate::ingest::scanner::ext_to_lang;

/// Position for code insertion.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(try_from = "String")]
pub enum InsertPosition {
    /// Insert at the top of the file.
    Top,
    /// Insert at the bottom of the file.
    Bottom,
    /// Insert before a specific line (1-based).
    BeforeLine(u32),
    /// Insert after a specific line (1-based).
    AfterLine(u32),
}

impl std::str::FromStr for InsertPosition {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            s if s.starts_with("before:") => {
                let n: u32 = s[7..]
                    .parse()
                    .map_err(|e| format!("invalid line number: {e}"))?;
                if n == 0 {
                    return Err("line number must be >= 1 (1-based)".into());
                }
                Ok(Self::BeforeLine(n))
            }
            s if s.starts_with("after:") => {
                let n: u32 = s[6..]
                    .parse()
                    .map_err(|e| format!("invalid line number: {e}"))?;
                if n == 0 {
                    return Err("line number must be >= 1 (1-based)".into());
                }
                Ok(Self::AfterLine(n))
            }
            _ => Err("position must be: top, bottom, before:N, or after:N".into()),
        }
    }
}

impl InsertPosition {
    /// Approximate target line (1-based) for preview lookup after insertion.
    ///
    /// Returns `None` for `Bottom` since the exact line depends on file length.
    #[must_use]
    pub fn target_line(&self) -> Option<u32> {
        match self {
            Self::Top => Some(1),
            Self::Bottom => None,
            Self::BeforeLine(n) => Some(*n),
            Self::AfterLine(n) => Some(n.saturating_add(1)),
        }
    }

    /// Build the appropriate preview source for this insert position.
    #[must_use]
    pub fn preview_source(&self) -> crate::application::index::PreviewSource<'static> {
        match self.target_line() {
            Some(line) => crate::application::index::PreviewSource::Line(line),
            None => crate::application::index::PreviewSource::Last,
        }
    }
}

impl TryFrom<String> for InsertPosition {
    type Error = String;
    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        s.parse()
    }
}

/// Insert code at a specific position in a file.
///
/// `project_root` is used to resolve `rel_path` into an absolute path for disk I/O.
pub fn insert_code(
    project_root: &std::path::Path,
    rel_path: &str,
    position: &InsertPosition,
    code: &str,
    guard: &SyntaxGuard,
) -> Result<String> {
    let path = crate::error::validate_relative_path(rel_path, project_root)?;
    let source = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            RlmError::FileNotFound {
                path: rel_path.into(),
            }
        } else {
            RlmError::from(e)
        }
    })?;

    let modified = apply_insertion(&source, position, code)?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = ext_to_lang(ext);

    validate_and_write(guard, lang, &modified, &path)?;

    Ok(modified)
}

/// Insert code at the top of the source.
fn insert_at_top(source: &str, code: &str) -> String {
    let mut parts = Vec::new();
    parts.push(code.to_string());
    if !source.is_empty() {
        parts.push(source.to_string());
    }
    parts.join("\n")
}

/// Insert code at the bottom of the source.
fn insert_at_bottom(source: &str, code: &str) -> String {
    let mut parts = Vec::new();
    parts.push(source.to_string());
    if !source.ends_with('\n') && !source.is_empty() {
        parts.push(String::new());
    }
    parts.push(code.to_string());
    parts.join("\n")
}

/// Insert code at a specific 1-based line number, either before or after the target line.
fn insert_at_line(lines: &[&str], code: &str, line_idx: usize, after: bool) -> String {
    let mut result = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if i == line_idx && !after {
            result.push(code.to_string());
        }
        result.push(l.to_string());
        if i == line_idx && after {
            result.push(code.to_string());
        }
    }
    result.join("\n")
}

/// Insert code before or after a specific 1-based line number.
fn insert_relative(source: &str, code: &str, line: u32, after: bool) -> Result<String> {
    let lines: Vec<&str> = source.lines().collect();
    let idx = (line as usize).saturating_sub(1);
    // "before" allows idx == lines.len() (appending); "after" requires idx < lines.len()
    let out_of_bounds = if after {
        idx >= lines.len()
    } else {
        idx > lines.len()
    };
    if out_of_bounds {
        return Err(EditError::LineOutOfBounds {
            line: line as usize,
            max: lines.len(),
        }
        .into());
    }
    Ok(insert_at_line(&lines, code, idx, after))
}

/// Apply insertion to source string without writing to disk.
pub fn apply_insertion(source: &str, position: &InsertPosition, code: &str) -> Result<String> {
    match position {
        InsertPosition::Top => Ok(insert_at_top(source, code)),
        InsertPosition::Bottom => Ok(insert_at_bottom(source, code)),
        InsertPosition::BeforeLine(line) => insert_relative(source, code, *line, false),
        InsertPosition::AfterLine(line) => insert_relative(source, code, *line, true),
    }
}

#[cfg(test)]
#[path = "inserter_position_tests.rs"]
mod position_tests;
#[cfg(test)]
#[path = "inserter_tests.rs"]
mod tests;
