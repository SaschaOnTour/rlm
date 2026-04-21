//! TOML semantic parser for rlm.
//!
//! Extracts semantic structure from TOML files including:
//! - Top-level tables as sections
//! - Nested tables with dot-notation paths
//! - Arrays of tables
//! - Special handling for Cargo.toml, pyproject.toml patterns

use toml::Table;

use crate::domain::chunk::{Chunk, ChunkKind};
use crate::error::Result;
use crate::ingest::text::{
    create_fallback_chunk, extract_structured_chunks, value_preview_string, StructuredChunkConfig,
    TextParser,
};

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

        let cfg = StructuredChunkConfig {
            source,
            file_id,
            determine_kind: determine_toml_kind,
            format_signature: |key, val| format!("{} = {}", key, toml_value_kind_label(val)),
            find_lines: find_toml_key_lines,
            value_to_string: |v, indent| value_preview_string(v, indent, false, " = "),
            is_important_key: is_important_toml_key,
        };
        extract_structured_chunks(&json_value, "", &mut chunks, 0, &cfg);

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

/// Map a TOML key to a known kind label, if it matches a well-known pattern (operation: logic only).
fn toml_key_kind_label(key_lower: &str) -> Option<&'static str> {
    match key_lower {
        // Cargo.toml
        "package" | "lib" | "bin" => Some("cargo"),
        "dependencies" | "dev-dependencies" | "build-dependencies" => Some("deps"),
        "features" => Some("features"),
        "workspace" => Some("workspace"),
        // pyproject.toml
        "project" | "tool" => Some("pyproject"),
        "build-system" => Some("build"),
        // Generic
        "scripts" | "commands" => Some("scripts"),
        "env" | "environment" => Some("env"),
        "settings" | "config" | "options" => Some("config"),
        _ => None,
    }
}

/// Map a JSON value type to a TOML kind label (operation: logic only).
fn toml_value_kind_label(value: &serde_json::Value) -> &'static str {
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

/// Determine the `ChunkKind` for a TOML key-value pair (integration: dispatches to operations).
fn determine_toml_kind(key: &str, value: &serde_json::Value) -> ChunkKind {
    let label =
        toml_key_kind_label(&key.to_lowercase()).unwrap_or_else(|| toml_value_kind_label(value));
    ChunkKind::Other(label.into())
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

/// Check whether a line matches a TOML table or array-of-tables header.
fn is_toml_header_match(trimmed: &str, table_header: &str, array_header: &str) -> bool {
    trimmed == table_header || trimmed == array_header
}

/// Check whether a line matches a TOML key assignment.
fn is_toml_key_match(trimmed: &str, key: &str) -> bool {
    trimmed.starts_with(&format!("{key} ")) || trimmed.starts_with(&format!("{key}="))
}

/// Check whether a line starts a new TOML section.
fn is_toml_section_start(trimmed: &str) -> bool {
    trimmed.starts_with('[')
}

fn find_toml_key_lines(source: &str, key: &str, full_path: &str) -> (u32, u32) {
    let lines: Vec<&str> = source.lines().collect();
    let table_header = format!("[{full_path}]");
    let array_header = format!("[[{full_path}]]");

    let mut start_line = 1u32;
    let mut end_line = lines.len() as u32;
    let mut found = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if !found
            && (is_toml_header_match(trimmed, &table_header, &array_header)
                || is_toml_key_match(trimmed, key))
        {
            start_line = (i + 1) as u32;
            found = true;
            continue;
        }

        if found && is_toml_section_start(trimmed) {
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

// toml_json_type_name was identical to toml_value_kind_label; consolidated.

#[cfg(test)]
#[path = "toml_parser_tests.rs"]
mod tests;
