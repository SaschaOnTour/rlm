use serde::Deserialize;

use crate::edit::syntax_guard::{validate_and_write, SyntaxGuard};
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
                Ok(Self::BeforeLine(n))
            }
            s if s.starts_with("after:") => {
                let n: u32 = s[6..]
                    .parse()
                    .map_err(|e| format!("invalid line number: {e}"))?;
                Ok(Self::AfterLine(n))
            }
            _ => Err("position must be: top, bottom, before:N, or after:N".into()),
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
    let path = project_root.join(rel_path);
    let source = std::fs::read_to_string(&path).map_err(|_| RlmError::FileNotFound {
        path: rel_path.into(),
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
        return Err(RlmError::Other(format!(
            "line {line} is beyond file length ({})",
            lines.len()
        )));
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
mod tests {
    use super::*;

    /// Line number used to test out-of-bounds insertion.
    const BEYOND_FILE_LINE: u32 = 10;

    #[test]
    fn insert_at_top() {
        let source = "line1\nline2\nline3";
        let result = apply_insertion(source, &InsertPosition::Top, "// header").unwrap();
        assert!(result.starts_with("// header"));
        assert!(result.contains("line1"));
    }

    #[test]
    fn insert_at_bottom() {
        let source = "line1\nline2";
        let result = apply_insertion(source, &InsertPosition::Bottom, "// footer").unwrap();
        assert!(result.ends_with("// footer"));
    }

    #[test]
    fn insert_before_line() {
        let source = "line1\nline2\nline3";
        let result =
            apply_insertion(source, &InsertPosition::BeforeLine(2), "// inserted").unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[1], "// inserted");
        assert_eq!(lines[2], "line2");
    }

    #[test]
    fn insert_after_line() {
        let source = "line1\nline2\nline3";
        let result = apply_insertion(source, &InsertPosition::AfterLine(1), "// inserted").unwrap();
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[0], "line1");
        assert_eq!(lines[1], "// inserted");
        assert_eq!(lines[2], "line2");
    }

    #[test]
    fn insert_beyond_file_errors() {
        let source = "line1\nline2";
        let result = apply_insertion(
            source,
            &InsertPosition::AfterLine(BEYOND_FILE_LINE),
            "// nope",
        );
        assert!(result.is_err());
    }

    #[test]
    fn parse_position_top() {
        assert_eq!(
            "top".parse::<InsertPosition>().unwrap(),
            InsertPosition::Top
        );
    }

    #[test]
    fn parse_position_bottom() {
        assert_eq!(
            "bottom".parse::<InsertPosition>().unwrap(),
            InsertPosition::Bottom
        );
    }

    #[test]
    fn parse_position_before_line() {
        assert_eq!(
            "before:5".parse::<InsertPosition>().unwrap(),
            InsertPosition::BeforeLine(5)
        );
    }

    #[test]
    fn parse_position_after_line() {
        assert_eq!(
            "after:10".parse::<InsertPosition>().unwrap(),
            InsertPosition::AfterLine(10)
        );
    }

    #[test]
    fn parse_position_invalid_format() {
        assert!("middle".parse::<InsertPosition>().is_err());
    }

    #[test]
    fn parse_position_invalid_line_number() {
        assert!("before:abc".parse::<InsertPosition>().is_err());
    }

    #[test]
    fn deserialize_position_from_json_string() {
        let pos: InsertPosition = serde_json::from_str("\"before:5\"").unwrap();
        assert_eq!(pos, InsertPosition::BeforeLine(5));
    }

    #[test]
    fn deserialize_position_invalid() {
        assert!(serde_json::from_str::<InsertPosition>("\"invalid\"").is_err());
    }
}
