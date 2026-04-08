//! TOML semantic parser for rlm.
//!
//! Extracts semantic structure from TOML files including:
//! - Top-level tables as sections
//! - Nested tables with dot-notation paths
//! - Arrays of tables
//! - Special handling for Cargo.toml, pyproject.toml patterns

use toml::Table;

use crate::error::Result;
use crate::ingest::text::{create_fallback_chunk, extract_structured_chunks, value_preview_string, TextParser};
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

        // Parse the TOML document into a Table
        let table: Table = match toml::from_str(source) {
            Ok(t) => t,
            Err(_) => {
                return Ok(vec![create_fallback_chunk(source, file_id, "toml")]);
            }
        };

        // Convert TOML Table -> serde_json::Value for shared extraction
        let json_value = toml_table_to_json(&table);

        extract_structured_chunks(
            &json_value, "", source, file_id, &mut chunks, 0,
            &determine_toml_kind,
            &|key, val| format!("{} = {}", key, toml_json_type_name(val)),
            &find_toml_key_lines,
            &|v, indent| value_preview_string(v, indent, false, " = "),
            &is_important_toml_key,
        );

        if chunks.is_empty() {
            chunks.push(create_fallback_chunk(source, file_id, "toml"));
        }

        Ok(chunks)
    }
}

/// Convert a TOML `Table` to a `serde_json::Value::Object` for shared extraction.
fn toml_table_to_json(table: &Table) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = table
        .iter()
        .map(|(k, v)| (k.clone(), toml_val_to_json(v)))
        .collect();
    serde_json::Value::Object(map)
}

fn toml_val_to_json(val: &toml::Value) -> serde_json::Value {
    match val {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(n) => serde_json::json!(n),
        toml::Value::Float(f) => serde_json::json!(f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(toml_val_to_json).collect())
        }
        toml::Value::Table(t) => toml_table_to_json(t),
    }
}

fn determine_toml_kind(key: &str, value: &serde_json::Value) -> ChunkKind {
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
            serde_json::Value::Object(_) => ChunkKind::Other("table".into()),
            serde_json::Value::Array(_) => ChunkKind::Other("array".into()),
            serde_json::Value::String(_) => ChunkKind::Other("string".into()),
            serde_json::Value::Number(n) => {
                if n.is_f64() && !n.is_i64() && !n.is_u64() {
                    ChunkKind::Other("float".into())
                } else {
                    ChunkKind::Other("integer".into())
                }
            }
            serde_json::Value::Bool(_) => ChunkKind::Other("bool".into()),
            serde_json::Value::Null => ChunkKind::Other("string".into()), // datetime -> string in JSON
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

fn toml_json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Object(_) => "table",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Number(n) => {
            if n.is_f64() && !n.is_i64() && !n.is_u64() {
                "float"
            } else {
                "integer"
            }
        }
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Null => "null",
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
