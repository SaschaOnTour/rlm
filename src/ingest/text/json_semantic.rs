//! JSON semantic parser for rlm.
//!
//! Extracts semantic structure from JSON files including:
//! - Top-level keys as sections
//! - Nested objects with dot-notation paths
//! - Arrays
//! - Special handling for package.json, tsconfig.json, etc.

use serde_json::Value;

use crate::domain::chunk::{Chunk, ChunkKind};
use crate::error::Result;
use crate::ingest::text::{
    create_fallback_chunk, extract_structured_chunks, value_preview_string, StructuredChunkConfig,
    TextParser,
};

pub struct JsonSemanticParser;

impl JsonSemanticParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonSemanticParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TextParser for JsonSemanticParser {
    fn format(&self) -> &'static str {
        "json"
    }

    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let mut chunks = Vec::new();

        // Parse the JSON
        let value: Value = match serde_json::from_str(source) {
            Ok(v) => v,
            Err(_) => {
                return Ok(vec![create_fallback_chunk(source, file_id, "json")]);
            }
        };

        let cfg = StructuredChunkConfig {
            source,
            file_id,
            determine_kind: determine_json_kind,
            format_signature: |key, val| format!("\"{}\": {}", key, json_type_name(val)),
            find_lines: |src, key, _full_path| find_json_key_lines(src, key),
            value_to_string: |v, indent| value_preview_string(v, indent, true, ": "),
            is_important_key: is_important_json_key,
        };
        extract_structured_chunks(&value, "", &mut chunks, 0, &cfg);

        if chunks.is_empty() {
            chunks.push(create_fallback_chunk(source, file_id, "json"));
        }

        Ok(chunks)
    }
}

fn determine_json_kind(key: &str, value: &Value) -> ChunkKind {
    // Special patterns for common JSON file types
    match key.to_lowercase().as_str() {
        // package.json
        "name" | "version" | "main" | "module" | "types" => ChunkKind::Other("package".into()),
        "scripts" => ChunkKind::Other("scripts".into()),
        "dependencies" | "devdependencies" | "peerdependencies" | "optionaldependencies" => {
            ChunkKind::Other("deps".into())
        }
        "engines" | "browserslist" => ChunkKind::Other("compat".into()),
        // tsconfig.json
        "compileroptions" | "compilerOptions" => ChunkKind::Other("tsconfig".into()),
        "include" | "exclude" | "files" => ChunkKind::Other("files".into()),
        "extends" => ChunkKind::Other("extends".into()),
        // ESLint, Prettier
        "rules" => ChunkKind::Other("rules".into()),
        "plugins" => ChunkKind::Other("plugins".into()),
        "env" | "environment" | "globals" => ChunkKind::Other("env".into()),
        // Generic
        "config" | "settings" | "options" => ChunkKind::Other("config".into()),
        // Default based on value type
        _ => match value {
            Value::Object(_) => ChunkKind::Other("object".into()),
            Value::Array(_) => ChunkKind::Other("array".into()),
            Value::String(_) => ChunkKind::Other("string".into()),
            Value::Number(_) => ChunkKind::Other("number".into()),
            Value::Bool(_) => ChunkKind::Other("bool".into()),
            Value::Null => ChunkKind::Other("null".into()),
        },
    }
}

fn is_important_json_key(key: &str) -> bool {
    matches!(
        key.to_lowercase().as_str(),
        "name"
            | "version"
            | "description"
            | "main"
            | "module"
            | "types"
            | "scripts"
            | "dependencies"
            | "devdependencies"
            | "compileroptions"
            | "compilerOptions"
            | "rules"
            | "extends"
            | "plugins"
            | "repository"
            | "author"
            | "license"
    )
}

/// Count brace/bracket balance deltas on a single line.
fn count_delimiters(line: &str) -> (i32, i32) {
    let mut braces = 0i32;
    let mut brackets = 0i32;
    for ch in line.chars() {
        match ch {
            '{' => braces += 1,
            '}' => braces -= 1,
            '[' => brackets += 1,
            ']' => brackets -= 1,
            _ => {}
        }
    }
    (braces, brackets)
}

fn find_json_key_lines(source: &str, key: &str) -> (u32, u32) {
    let lines: Vec<&str> = source.lines().collect();
    let search_pattern = format!("\"{key}\"");

    let mut start_line = 1u32;
    let mut end_line = lines.len() as u32;
    let mut found = false;
    let mut brace_count = 0i32;
    let mut bracket_count = 0i32;

    for (i, line) in lines.iter().enumerate() {
        if !found && line.contains(&search_pattern) {
            start_line = (i + 1) as u32;
            found = true;
        }

        if !found {
            continue;
        }

        let (bd, kd) = count_delimiters(line);
        brace_count += bd;
        bracket_count += kd;

        // When we're back to balanced, we've found the end
        if brace_count <= 0 && bracket_count <= 0 {
            end_line = (i + 1) as u32;
            break;
        }
    }

    (start_line, end_line.max(start_line))
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
        Value::Null => "null",
    }
}

#[cfg(test)]
#[path = "json_semantic_tests.rs"]
mod tests;
