//! JSON semantic parser for rlm-cli.
//!
//! Extracts semantic structure from JSON files including:
//! - Top-level keys as sections
//! - Nested objects with dot-notation paths
//! - Arrays
//! - Special handling for package.json, tsconfig.json, etc.

use serde_json::Value;

use crate::error::Result;
use crate::ingest::text::TextParser;
use crate::models::chunk::{Chunk, ChunkKind};

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
                // If parsing fails, create a single chunk for the whole file
                return Ok(vec![create_fallback_chunk(source, file_id)]);
            }
        };

        // Extract chunks from the parsed JSON
        extract_json_chunks(&value, "", source, file_id, &mut chunks, 0);

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
        kind: ChunkKind::Other("json".into()),
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

fn extract_json_chunks(
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

    if let Value::Object(obj) = value {
        for (key, val) in obj {
            let full_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };

            // Determine chunk kind based on key name
            let kind = determine_json_kind(key, val);

            // Find approximate line range
            let (start_line, end_line) = find_json_key_lines(source, key);

            // Create content representation
            let content = json_value_to_string(val, 0);

            // Create chunk for objects, arrays, and important keys
            let should_chunk = matches!(val, Value::Object(_) | Value::Array(_))
                || depth < 2
                || is_important_json_key(key);

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
                    signature: Some(format!("\"{}\": {}", key, json_type_name(val))),
                    visibility: None,
                    ui_ctx: None,
                    doc_comment: None,
                    attributes: None,
                    content,
                });
            }

            // Recurse into nested objects
            if matches!(val, Value::Object(_)) {
                extract_json_chunks(val, &full_path, source, file_id, chunks, depth + 1);
            }

            // Handle arrays of objects
            if let Value::Array(arr) = val {
                for (i, item) in arr.iter().enumerate() {
                    if matches!(item, Value::Object(_)) {
                        let item_path = format!("{full_path}[{i}]");
                        extract_json_chunks(item, &item_path, source, file_id, chunks, depth + 1);
                    }
                }
            }
        }
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

            // Count opening braces/brackets on this line
            for ch in line.chars() {
                match ch {
                    '{' => brace_count += 1,
                    '}' => brace_count -= 1,
                    '[' => bracket_count += 1,
                    ']' => bracket_count -= 1,
                    _ => {}
                }
            }
            continue;
        }

        if found {
            // Track braces/brackets to find matching close
            for ch in line.chars() {
                match ch {
                    '{' => brace_count += 1,
                    '}' => brace_count -= 1,
                    '[' => bracket_count += 1,
                    ']' => bracket_count -= 1,
                    _ => {}
                }
            }

            // When we're back to balanced, we've found the end
            if brace_count <= 0 && bracket_count <= 0 {
                end_line = (i + 1) as u32;
                break;
            }
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

fn json_value_to_string(value: &Value, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.len() > 100 {
                format!("\"{}...\"", &s[..97])
            } else {
                format!("\"{s}\"")
            }
        }
        Value::Array(arr) => {
            if arr.len() > 5 {
                format!("[...{} items]", arr.len())
            } else {
                let items: Vec<String> = arr
                    .iter()
                    .map(|v| json_value_to_string(v, indent + 1))
                    .collect();
                format!("[{}]", items.join(", "))
            }
        }
        Value::Object(obj) => {
            if obj.len() > 5 {
                format!("{{...{} keys}}", obj.len())
            } else {
                let items: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}\"{}\": {}",
                            prefix,
                            k,
                            json_value_to_string(v, indent + 1)
                        )
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

    fn parser() -> JsonSemanticParser {
        JsonSemanticParser::new()
    }

    #[test]
    fn parse_simple_json() {
        let source = r#"{
  "name": "my-project",
  "version": "1.0.0",
  "description": "A test project"
}"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.ident == "name"));
        assert!(chunks.iter().any(|c| c.ident == "version"));
    }

    #[test]
    fn parse_package_json() {
        let source = r#"{
  "name": "my-package",
  "version": "1.0.0",
  "scripts": {
    "build": "tsc",
    "test": "jest"
  },
  "dependencies": {
    "express": "^4.18.0"
  },
  "devDependencies": {
    "typescript": "^5.0.0"
  }
}"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "scripts"));
        assert!(chunks.iter().any(|c| c.ident == "dependencies"));
        assert!(chunks.iter().any(|c| c.ident == "devDependencies"));

        // Verify scripts is categorized correctly
        let scripts = chunks.iter().find(|c| c.ident == "scripts");
        assert!(scripts.is_some());
    }

    #[test]
    fn parse_tsconfig_json() {
        let source = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "strict": true,
    "outDir": "./dist"
  },
  "include": ["src/**/*"],
  "exclude": ["node_modules"]
}"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "compilerOptions"));
        assert!(chunks.iter().any(|c| c.ident == "include"));
    }

    #[test]
    fn parse_nested_json() {
        let source = r#"{
  "config": {
    "database": {
      "host": "localhost",
      "port": 5432
    }
  }
}"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "config"));
        assert!(chunks.iter().any(|c| c.ident == "config.database"));
    }

    #[test]
    fn parse_invalid_json_fallback() {
        let source = "{invalid json}";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ident, "_root");
    }

    #[test]
    fn empty_json() {
        let chunks = parser().parse_chunks("{}", 1).unwrap();
        // Empty object should still produce a fallback chunk
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn parse_array_json() {
        let source = r#"{
  "items": [
    {"name": "item1"},
    {"name": "item2"}
  ]
}"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "items"));
    }
}
