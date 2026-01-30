use crate::edit::syntax_guard::SyntaxGuard;
use crate::error::{Result, RlmError};
use crate::ingest::scanner::ext_to_lang;

/// Position for code insertion.
#[derive(Debug, Clone, PartialEq)]
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

/// Insert code at a specific position in a file.
pub fn insert_code(
    file_path: &str,
    position: &InsertPosition,
    code: &str,
    guard: &SyntaxGuard,
) -> Result<String> {
    let path = std::path::Path::new(file_path);
    let source = std::fs::read_to_string(path).map_err(|_| RlmError::FileNotFound {
        path: file_path.into(),
    })?;

    let modified = apply_insertion(&source, position, code)?;

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = ext_to_lang(ext);

    guard.validate_and_write(lang, &modified, path)?;

    Ok(modified)
}

/// Apply insertion to source string without writing to disk.
pub fn apply_insertion(source: &str, position: &InsertPosition, code: &str) -> Result<String> {
    let lines: Vec<&str> = source.lines().collect();

    let mut result = Vec::new();

    match position {
        InsertPosition::Top => {
            result.push(code.to_string());
            if !source.is_empty() {
                result.push(source.to_string());
            }
        }
        InsertPosition::Bottom => {
            result.push(source.to_string());
            if !source.ends_with('\n') && !source.is_empty() {
                result.push(String::new()); // add newline separator
            }
            result.push(code.to_string());
        }
        InsertPosition::BeforeLine(line) => {
            let idx = (*line as usize).saturating_sub(1);
            if idx > lines.len() {
                return Err(RlmError::Other(format!(
                    "line {line} is beyond file length ({})",
                    lines.len()
                )));
            }
            for (i, l) in lines.iter().enumerate() {
                if i == idx {
                    result.push(code.to_string());
                }
                result.push(l.to_string());
            }
        }
        InsertPosition::AfterLine(line) => {
            let idx = (*line as usize).saturating_sub(1);
            if idx >= lines.len() {
                return Err(RlmError::Other(format!(
                    "line {line} is beyond file length ({})",
                    lines.len()
                )));
            }
            for (i, l) in lines.iter().enumerate() {
                result.push(l.to_string());
                if i == idx {
                    result.push(code.to_string());
                }
            }
        }
    }

    Ok(result.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let result = apply_insertion(source, &InsertPosition::AfterLine(10), "// nope");
        assert!(result.is_err());
    }
}
