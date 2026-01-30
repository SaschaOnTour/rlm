//! TOML semantic parser for rlm.
//!
//! Extracts semantic structure from TOML files including:
//! - Top-level tables as sections
//! - Nested tables with dot-notation paths
//! - Arrays of tables
//! - Special handling for Cargo.toml, pyproject.toml patterns

use toml::{Table, Value};

use crate::error::Result;
use crate::ingest::text::TextParser;
use crate::models::chunk::{Chunk, ChunkKind};

pub struct TomlParser;

impl TomlParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for TomlParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TextParser for TomlParser {
    fn format(&self) -> &'static str {
        "toml"
    }

    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let mut chunks = Vec::new();

        // Parse the TOML document into a Table (toml 0.9 changed Value::from_str to parse single values only)
        let table: Table = match toml::from_str(source) {
            Ok(t) => t,
            Err(_) => {
                // If parsing fails, create a single chunk for the whole file
                return Ok(vec![create_fallback_chunk(source, file_id)]);
            }
        };

        // Convert Table to Value::Table for extraction
        let value = Value::Table(table);

        // Extract chunks from the parsed TOML
        extract_toml_chunks(&value, "", source, file_id, &mut chunks, 0);

        // If no chunks were created, create a fallback
        if chunks.is_empty() {
            chunks.push(create_fallback_chunk(source, file_id));
        }

        Ok(chunks)
    }
}

fn create_fallback_chunk(source: &str, file_id: i64) -> Chunk {
    let line_count = source.lines().count() as u32;
    Chunk {
        id: 0,
        file_id,
        start_line: 1,
        end_line: line_count.max(1),
        start_byte: 0,
        end_byte: source.len() as u32,
        kind: ChunkKind::Other("toml".into()),
        ident: "_root".to_string(),
        parent: None,
        signature: None,
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content: source.to_string(),
    }
}

fn extract_toml_chunks(
    value: &Value,
    path: &str,
    source: &str,
    file_id: i64,
    chunks: &mut Vec<Chunk>,
    depth: usize,
) {
    // Limit depth to avoid excessive chunking
    if depth > 3 {
        return;
    }

    if let Value::Table(table) = value {
        for (key, val) in table {
            let full_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };

            // Determine chunk kind based on key name
            let kind = determine_toml_kind(key, val);

            // Find line range for this key/table
            let (start_line, end_line) = find_toml_key_lines(source, key, &full_path);

            // Create content representation
            let content = toml_value_to_string(val, 0);

            // Create chunk for tables and important keys
            let should_chunk = matches!(val, Value::Table(_) | Value::Array(_))
                || depth < 2
                || is_important_toml_key(key);

            if should_chunk {
                chunks.push(Chunk {
                    id: 0,
                    file_id,
                    start_line,
                    end_line,
                    start_byte: 0,
                    end_byte: 0,
                    kind: kind.clone(),
                    ident: full_path.clone(),
                    parent: if path.is_empty() {
                        None
                    } else {
                        Some(path.to_string())
                    },
                    signature: Some(format!("{} = {}", key, toml_type_name(val))),
                    visibility: None,
                    ui_ctx: None,
                    doc_comment: None,
                    attributes: None,
                    content,
                });
            }

            // Recurse into nested tables
            if matches!(val, Value::Table(_)) {
                extract_toml_chunks(val, &full_path, source, file_id, chunks, depth + 1);
            }

            // Handle arrays of tables
            if let Value::Array(arr) = val {
                for (i, item) in arr.iter().enumerate() {
                    if matches!(item, Value::Table(_)) {
                        let item_path = format!("{full_path}[{i}]");
                        extract_toml_chunks(item, &item_path, source, file_id, chunks, depth + 1);
                    }
                }
            }
        }
    }
}

fn determine_toml_kind(key: &str, value: &Value) -> ChunkKind {
    // Special patterns for common TOML file types
    match key.to_lowercase().as_str() {
        // Cargo.toml
        "package" | "lib" | "bin" => ChunkKind::Other("cargo".into()),
        "dependencies" | "dev-dependencies" | "build-dependencies" => {
            ChunkKind::Other("deps".into())
        }
        "features" => ChunkKind::Other("features".into()),
        "workspace" => ChunkKind::Other("workspace".into()),
        // pyproject.toml
        "project" | "tool" => ChunkKind::Other("pyproject".into()),
        "build-system" => ChunkKind::Other("build".into()),
        // Generic
        "scripts" | "commands" => ChunkKind::Other("scripts".into()),
        "env" | "environment" => ChunkKind::Other("env".into()),
        "settings" | "config" | "options" => ChunkKind::Other("config".into()),
        // Default based on value type
        _ => match value {
            Value::Table(_) => ChunkKind::Other("table".into()),
            Value::Array(_) => ChunkKind::Other("array".into()),
            Value::String(_) => ChunkKind::Other("string".into()),
            Value::Integer(_) => ChunkKind::Other("integer".into()),
            Value::Float(_) => ChunkKind::Other("float".into()),
            Value::Boolean(_) => ChunkKind::Other("bool".into()),
            Value::Datetime(_) => ChunkKind::Other("datetime".into()),
        },
    }
}

fn is_important_toml_key(key: &str) -> bool {
    matches!(
        key.to_lowercase().as_str(),
        "name"
            | "version"
            | "description"
            | "authors"
            | "license"
            | "edition"
            | "rust-version"
            | "repository"
            | "homepage"
            | "readme"
            | "keywords"
            | "categories"
            | "entry-points"
            | "requires-python"
    )
}

fn find_toml_key_lines(source: &str, key: &str, full_path: &str) -> (u32, u32) {
    let lines: Vec<&str> = source.lines().collect();

    // Look for table header [path] or [[path]] first
    let table_header = format!("[{full_path}]");
    let array_header = format!("[[{full_path}]]");

    let mut start_line = 1u32;
    let mut end_line = lines.len() as u32;
    let mut found = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check for table header
        if !found && (trimmed == table_header || trimmed == array_header) {
            start_line = (i + 1) as u32;
            found = true;
            continue;
        }

        // Check for key = value
        if !found && trimmed.starts_with(&format!("{key} "))
            || trimmed.starts_with(&format!("{key}="))
        {
            start_line = (i + 1) as u32;
            found = true;
            continue;
        }

        // Find end: next table header or section
        if found && trimmed.starts_with('[') && !trimmed.starts_with("[[") {
            end_line = i as u32;
            break;
        }
        if found && trimmed.starts_with("[[") {
            end_line = i as u32;
            break;
        }
    }

    if !found {
        // Fallback: search for just the key
        for (i, line) in lines.iter().enumerate() {
            if line.contains(key) {
                start_line = (i + 1) as u32;
                end_line = start_line;
                break;
            }
        }
    }

    (start_line, end_line.max(start_line))
}

fn toml_type_name(value: &Value) -> &'static str {
    match value {
        Value::Table(_) => "table",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Integer(_) => "integer",
        Value::Float(_) => "float",
        Value::Boolean(_) => "bool",
        Value::Datetime(_) => "datetime",
    }
}

fn toml_value_to_string(value: &Value, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    match value {
        Value::String(s) => {
            if s.len() > 100 {
                format!("\"{}...\"", &s[..97])
            } else {
                format!("\"{s}\"")
            }
        }
        Value::Integer(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Datetime(dt) => dt.to_string(),
        Value::Array(arr) => {
            if arr.len() > 5 {
                format!("[...{} items]", arr.len())
            } else {
                let items: Vec<String> = arr
                    .iter()
                    .map(|v| toml_value_to_string(v, indent + 1))
                    .collect();
                format!("[{}]", items.join(", "))
            }
        }
        Value::Table(table) => {
            if table.len() > 5 {
                format!("{{...{} keys}}", table.len())
            } else {
                let items: Vec<String> = table
                    .iter()
                    .map(|(k, v)| {
                        format!("{}{} = {}", prefix, k, toml_value_to_string(v, indent + 1))
                    })
                    .collect();
                format!(
                    "{{\n{}\n{}}}",
                    items.join(",\n"),
                    "  ".repeat(indent.saturating_sub(1))
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> TomlParser {
        TomlParser::new()
    }

    #[test]
    fn parse_simple_toml() {
        let source = r#"
name = "my-project"
version = "1.0.0"
description = "A test project"
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.ident == "name"));
        assert!(chunks.iter().any(|c| c.ident == "version"));
    }

    #[test]
    fn parse_cargo_toml() {
        let source = r#"
[package]
name = "my-crate"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1.0"
tokio = { version = "1", features = ["full"] }

[dev-dependencies]
tempfile = "3"
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "package"));
        assert!(chunks.iter().any(|c| c.ident == "dependencies"));
        assert!(chunks.iter().any(|c| c.ident == "dev-dependencies"));
    }

    #[test]
    fn parse_nested_tables() {
        let source = r#"
[tool.poetry]
name = "my-package"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.9"
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "tool"));
    }

    #[test]
    fn parse_array_of_tables() {
        let source = r#"
[[bin]]
name = "app"
path = "src/main.rs"

[[bin]]
name = "cli"
path = "src/cli.rs"
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "bin"));
    }

    #[test]
    fn parse_invalid_toml_fallback() {
        let source = "[invalid toml content";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ident, "_root");
    }

    #[test]
    fn empty_toml() {
        let chunks = parser().parse_chunks("", 1).unwrap();
        assert_eq!(chunks.len(), 1);
    }
}
