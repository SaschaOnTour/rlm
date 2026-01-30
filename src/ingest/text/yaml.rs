//! YAML semantic parser for rlm-cli.
//!
//! Extracts semantic structure from YAML files including:
//! - Top-level keys as sections
//! - Nested structures with dot-notation paths
//! - Arrays and sequences
//! - Special handling for common patterns (K8s, Docker Compose, GitHub Actions)

use serde_yaml::Value;

use crate::error::Result;
use crate::ingest::text::TextParser;
use crate::models::chunk::{Chunk, ChunkKind};

pub struct YamlParser;

impl YamlParser {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for YamlParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TextParser for YamlParser {
    fn format(&self) -> &'static str {
        "yaml"
    }

    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let mut chunks = Vec::new();

        // Parse the YAML
        let value: Value = match serde_yaml::from_str(source) {
            Ok(v) => v,
            Err(_) => {
                // If parsing fails, create a single chunk for the whole file
                return Ok(vec![create_fallback_chunk(source, file_id)]);
            }
        };

        // Extract chunks from the parsed YAML
        extract_yaml_chunks(&value, "", source, file_id, &mut chunks, 0);

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
        kind: ChunkKind::Other("yaml".into()),
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

fn extract_yaml_chunks(
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

    match value {
        Value::Mapping(map) => {
            for (key, val) in map {
                let key_str = match key {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => continue,
                };

                let full_path = if path.is_empty() {
                    key_str.clone()
                } else {
                    format!("{path}.{key_str}")
                };

                // Determine chunk kind based on key name or content
                let kind = determine_yaml_kind(&key_str, val);

                // Find approximate line range for this key
                let (start_line, end_line) = find_key_lines(source, &key_str, depth);

                // Create content representation
                let content = yaml_value_to_string(val, 0);

                // Create chunk for top-level or important keys
                if depth < 2 || is_important_key(&key_str) {
                    chunks.push(Chunk {
                        id: 0,
                        file_id,
                        start_line,
                        end_line,
                        start_byte: 0, // Approximate
                        end_byte: 0,
                        kind: kind.clone(),
                        ident: full_path.clone(),
                        parent: if path.is_empty() {
                            None
                        } else {
                            Some(path.to_string())
                        },
                        signature: Some(format!("{}: {}", key_str, yaml_type_name(val))),
                        visibility: None,
                        ui_ctx: None,
                        doc_comment: None,
                        attributes: None,
                        content,
                    });
                }

                // Recurse into nested structures
                extract_yaml_chunks(val, &full_path, source, file_id, chunks, depth + 1);
            }
        }
        Value::Sequence(seq) if !seq.is_empty() && depth == 0 => {
            // Handle root-level arrays (uncommon but possible)
            for (i, item) in seq.iter().enumerate() {
                let item_path = format!("{path}[{i}]");
                extract_yaml_chunks(item, &item_path, source, file_id, chunks, depth + 1);
            }
        }
        _ => {}
    }
}

fn determine_yaml_kind(key: &str, value: &Value) -> ChunkKind {
    // Special patterns for common YAML file types
    match key.to_lowercase().as_str() {
        // Kubernetes
        "apiversion" | "kind" | "metadata" | "spec" | "status" => ChunkKind::Other("k8s".into()),
        // Docker Compose
        "services" | "volumes" | "networks" => ChunkKind::Other("compose".into()),
        // GitHub Actions
        "jobs" | "steps" | "runs-on" | "uses" => ChunkKind::Other("actions".into()),
        // CI/CD
        "stages" | "pipeline" | "build" | "test" | "deploy" => ChunkKind::Other("ci".into()),
        // Dependencies
        "dependencies" | "devdependencies" | "peerdependencies" => ChunkKind::Other("deps".into()),
        // Scripts
        "scripts" | "commands" => ChunkKind::Other("scripts".into()),
        // Default based on value type
        _ => match value {
            Value::Mapping(_) => ChunkKind::Other("object".into()),
            Value::Sequence(_) => ChunkKind::Other("array".into()),
            Value::String(_) => ChunkKind::Other("string".into()),
            Value::Number(_) => ChunkKind::Other("number".into()),
            Value::Bool(_) => ChunkKind::Other("bool".into()),
            Value::Null => ChunkKind::Other("null".into()),
            Value::Tagged(_) => ChunkKind::Other("tagged".into()),
        },
    }
}

fn is_important_key(key: &str) -> bool {
    // Keys that should always be chunked
    matches!(
        key.to_lowercase().as_str(),
        "name"
            | "version"
            | "description"
            | "main"
            | "scripts"
            | "dependencies"
            | "services"
            | "jobs"
            | "env"
            | "environment"
            | "config"
            | "settings"
            | "apiversion"
            | "kind"
            | "metadata"
            | "spec"
    )
}

fn find_key_lines(source: &str, key: &str, _depth: usize) -> (u32, u32) {
    // Simple heuristic: find the key in the source
    let lines: Vec<&str> = source.lines().collect();
    let search_key = format!("{key}:");
    let quoted_key = format!("\"{key}\":");

    let mut start_line = 1u32;
    let mut end_line = 1u32;
    let mut found = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !found && (trimmed.starts_with(&search_key) || trimmed.starts_with(&quoted_key)) {
            start_line = (i + 1) as u32;
            found = true;
        } else if found {
            // Find the end: next line at same or lower indentation
            let current_indent = line.len() - line.trim_start().len();
            let start_indent = lines
                .get((start_line - 1) as usize)
                .map_or(0, |l| l.len() - l.trim_start().len());

            if !trimmed.is_empty() && current_indent <= start_indent && i > start_line as usize - 1
            {
                end_line = i as u32;
                break;
            }
        }
    }

    if found && end_line <= start_line {
        end_line = lines.len() as u32;
    }
    if !found {
        end_line = lines.len() as u32;
    }

    (start_line, end_line)
}

fn yaml_type_name(value: &Value) -> &'static str {
    match value {
        Value::Mapping(_) => "object",
        Value::Sequence(_) => "array",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "bool",
        Value::Null => "null",
        Value::Tagged(_) => "tagged",
    }
}

fn yaml_value_to_string(value: &Value, indent: usize) -> String {
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
        Value::Sequence(seq) => {
            if seq.len() > 5 {
                format!("[...{} items]", seq.len())
            } else {
                let items: Vec<String> = seq
                    .iter()
                    .map(|v| yaml_value_to_string(v, indent + 1))
                    .collect();
                format!("[{}]", items.join(", "))
            }
        }
        Value::Mapping(map) => {
            if map.len() > 5 {
                format!("{{...{} keys}}", map.len())
            } else {
                let items: Vec<String> = map
                    .iter()
                    .map(|(k, v)| {
                        let key_str = match k {
                            Value::String(s) => s.clone(),
                            _ => format!("{k:?}"),
                        };
                        format!(
                            "{}{}: {}",
                            prefix,
                            key_str,
                            yaml_value_to_string(v, indent + 1)
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
        Value::Tagged(tagged) => format!(
            "!{} {}",
            tagged.tag,
            yaml_value_to_string(&tagged.value, indent)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> YamlParser {
        YamlParser::new()
    }

    #[test]
    fn parse_simple_yaml() {
        let source = r#"
name: my-project
version: 1.0.0
description: A test project
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.ident == "name"));
        assert!(chunks.iter().any(|c| c.ident == "version"));
    }

    #[test]
    fn parse_nested_yaml() {
        let source = r#"
services:
  web:
    image: nginx
    ports:
      - "80:80"
  db:
    image: postgres
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks.iter().any(|c| c.ident == "services"));
    }

    #[test]
    fn parse_github_actions() {
        let source = r#"
name: CI
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      - run: npm test
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "jobs"));
        // Should detect GitHub Actions patterns
        let jobs_chunk = chunks.iter().find(|c| c.ident == "jobs");
        assert!(jobs_chunk.is_some());
    }

    #[test]
    fn parse_kubernetes_manifest() {
        let source = r#"
apiVersion: v1
kind: Service
metadata:
  name: my-service
spec:
  selector:
    app: my-app
  ports:
    - port: 80
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.iter().any(|c| c.ident == "apiVersion"));
        assert!(chunks.iter().any(|c| c.ident == "kind"));
        assert!(chunks.iter().any(|c| c.ident == "metadata"));
    }

    #[test]
    fn parse_invalid_yaml_fallback() {
        let source = "this: is: not: valid: yaml: [";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ident, "_root");
    }

    #[test]
    fn empty_yaml() {
        let chunks = parser().parse_chunks("", 1).unwrap();
        assert_eq!(chunks.len(), 1);
    }
}
