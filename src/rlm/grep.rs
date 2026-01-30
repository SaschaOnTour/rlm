use regex::Regex;
use serde::Serialize;

use crate::db::Database;
use crate::error::{Result, RlmError};
use crate::models::token_estimate::{estimate_tokens, TokenEstimate};

/// A grep match result.
#[derive(Debug, Clone, Serialize)]
pub struct GrepResult {
    #[serde(rename = "m")]
    pub matches: Vec<GrepMatch>,
    #[serde(rename = "t")]
    pub tokens: TokenEstimate,
}

#[derive(Debug, Clone, Serialize)]
pub struct GrepMatch {
    /// File path.
    #[serde(rename = "f")]
    pub file: String,
    /// Line number.
    #[serde(rename = "l")]
    pub line: u32,
    /// The matching line content.
    #[serde(rename = "c")]
    pub content: String,
    /// Context lines before.
    #[serde(rename = "b", skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    /// Context lines after.
    #[serde(rename = "a", skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
}

/// Grep across indexed files for a pattern.
pub fn grep(
    db: &Database,
    pattern: &str,
    context_lines: usize,
    path_filter: Option<&str>,
    project_root: &std::path::Path,
) -> Result<GrepResult> {
    let re = Regex::new(pattern).map_err(|e| RlmError::Other(format!("invalid regex: {e}")))?;

    let files = db.get_all_files()?;
    let mut matches = Vec::new();

    for file in &files {
        if let Some(filter) = path_filter {
            if !file.path.starts_with(filter) {
                continue;
            }
        }

        let full_path = project_root.join(&file.path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines: Vec<&str> = source.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            if re.is_match(line) {
                let before: Vec<String> = if context_lines > 0 {
                    let start = i.saturating_sub(context_lines);
                    lines[start..i]
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect()
                } else {
                    Vec::new()
                };

                let after: Vec<String> = if context_lines > 0 {
                    let end = (i + 1 + context_lines).min(lines.len());
                    lines[i + 1..end]
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect()
                } else {
                    Vec::new()
                };

                matches.push(GrepMatch {
                    file: file.path.clone(),
                    line: i as u32 + 1,
                    content: line.to_string(),
                    before,
                    after,
                });
            }
        }
    }

    let output_str = serde_json::to_string(&matches).unwrap_or_default();
    let out_tokens = estimate_tokens(output_str.len());

    Ok(GrepResult {
        matches,
        tokens: TokenEstimate::new(0, out_tokens),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_db_with_file(tmp: &TempDir) -> (Database, String) {
        let db = Database::open_in_memory().unwrap();
        let file_path = "test.rs";
        let full_path = tmp.path().join(file_path);
        fs::write(
            &full_path,
            "fn main() {\n    // TODO: fix this\n    println!(\"hello\");\n}\n",
        )
        .unwrap();
        let f =
            crate::models::file::FileRecord::new(file_path.into(), "h".into(), "rust".into(), 100);
        db.upsert_file(&f).unwrap();
        (db, file_path.to_string())
    }

    #[test]
    fn grep_finds_pattern() {
        let tmp = TempDir::new().unwrap();
        let (db, _) = setup_db_with_file(&tmp);
        let result = grep(&db, "TODO", 0, None, tmp.path()).unwrap();
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches[0].content.contains("TODO"));
        assert_eq!(result.matches[0].line, 2);
    }

    #[test]
    fn grep_with_context() {
        let tmp = TempDir::new().unwrap();
        let (db, _) = setup_db_with_file(&tmp);
        let result = grep(&db, "TODO", 1, None, tmp.path()).unwrap();
        assert_eq!(result.matches.len(), 1);
        assert!(!result.matches[0].before.is_empty());
        assert!(!result.matches[0].after.is_empty());
    }

    #[test]
    fn grep_no_match() {
        let tmp = TempDir::new().unwrap();
        let (db, _) = setup_db_with_file(&tmp);
        let result = grep(&db, "NONEXISTENT", 0, None, tmp.path()).unwrap();
        assert!(result.matches.is_empty());
    }

    #[test]
    fn grep_with_path_filter() {
        let tmp = TempDir::new().unwrap();
        let (db, _) = setup_db_with_file(&tmp);
        let result = grep(&db, "TODO", 0, Some("other/"), tmp.path()).unwrap();
        assert!(result.matches.is_empty());
    }
}
