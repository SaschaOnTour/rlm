//! YAML semantic parser for rlm.
//!
//! Extracts semantic structure from YAML files including:
//! - Top-level keys as sections
//! - Nested structures with dot-notation paths
//! - Arrays and sequences
//! - Special handling for common patterns (K8s, Docker Compose, GitHub Actions)
//!
//! Value formatting and kind-detection helpers live in `yaml_helpers`.

use serde_yaml_ng::Value;

use crate::error::Result;
use crate::ingest::text::yaml_helpers::{
    determine_yaml_kind, find_key_lines, is_important_key, yaml_key_to_string, yaml_type_name,
    yaml_value_to_string,
};
use crate::ingest::text::{create_fallback_chunk, TextParser};
use crate::models::chunk::Chunk;

/// Maximum nesting depth for recursive YAML chunk extraction.
const MAX_NESTING_DEPTH: usize = 3;

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

    // qual:allow(iosp) reason: "if-dispatch: parse valid YAML or return fallback chunk"
    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let mut chunks = Vec::new();

        // Parse the YAML
        let value: Value = match serde_yaml_ng::from_str(source) {
            Ok(v) => v,
            Err(_) => {
                // If parsing fails, create a single chunk for the whole file
                return Ok(vec![create_fallback_chunk(source, file_id, "yaml")]);
            }
        };

        // Extract chunks from the parsed YAML
        let cfg = YamlChunkConfig { source, file_id };
        extract_yaml_chunks(&value, "", &mut chunks, 0, &cfg);

        // If no chunks were created, create a fallback
        if chunks.is_empty() {
            chunks.push(create_fallback_chunk(source, file_id, "yaml"));
        }

        Ok(chunks)
    }
}

/// Configuration bundle for YAML chunk extraction, reducing parameter count.
struct YamlChunkConfig<'a> {
    source: &'a str,
    file_id: i64,
}

/// Per-entry context for YAML mapping chunk processing, reducing parameter count.
struct YamlEntryContext<'a> {
    /// Full dot-notation path for this entry.
    full_path: &'a str,
    /// Parent path (dot-notation) for hierarchy.
    parent_path: &'a str,
    /// Current nesting depth.
    depth: usize,
}

/// An entry extracted from a YAML mapping for processing.
struct YamlEntry {
    key_str: String,
    full_path: String,
}

/// Collect entries and recursion targets from a YAML value (operation: logic only).
fn collect_yaml_entries(
    value: &Value,
    path: &str,
    depth: usize,
) -> Option<(Vec<YamlEntry>, Vec<String>)> {
    if depth > MAX_NESTING_DEPTH {
        return None;
    }

    match value {
        Value::Mapping(map) => {
            let mut entries = Vec::new();
            let mut recurse_paths = Vec::new();

            for (key, _val) in map {
                let key_str = match yaml_key_to_string(key) {
                    Some(s) => s,
                    None => continue,
                };

                let full_path = if path.is_empty() {
                    key_str.clone()
                } else {
                    format!("{path}.{key_str}")
                };

                recurse_paths.push(full_path.clone());
                entries.push(YamlEntry { key_str, full_path });
            }

            Some((entries, recurse_paths))
        }
        Value::Sequence(seq) if !seq.is_empty() && depth == 0 => {
            let recurse_paths: Vec<String> =
                (0..seq.len()).map(|i| format!("{path}[{i}]")).collect();
            Some((Vec::new(), recurse_paths))
        }
        _ => None,
    }
}

/// Extract chunks from a parsed YAML value tree (integration: calls only).
// qual:recursive
// qual:allow(iosp) reason: "recursive tree traversal inherently mixes iteration with delegation"
fn extract_yaml_chunks(
    value: &Value,
    path: &str,
    chunks: &mut Vec<Chunk>,
    depth: usize,
    cfg: &YamlChunkConfig<'_>,
) {
    let (entries, recurse_paths) = match collect_yaml_entries(value, path, depth) {
        Some(pair) => pair,
        None => return,
    };

    // Process mapping entries
    if let Value::Mapping(map) = value {
        for entry in &entries {
            let val = match map.get(Value::String(entry.key_str.clone())) {
                Some(v) => v,
                None => continue,
            };
            let entry_ctx = YamlEntryContext {
                full_path: &entry.full_path,
                parent_path: path,
                depth,
            };
            process_yaml_mapping_entry(&entry.key_str, val, &entry_ctx, chunks, cfg);
        }
    }

    // Recurse into nested structures
    for recurse_path in &recurse_paths {
        let child_val = resolve_yaml_child(value, path, recurse_path);
        let child_val = match child_val {
            Some(v) => v,
            None => continue,
        };
        extract_yaml_chunks(child_val, recurse_path, chunks, depth + 1, cfg);
    }
}

/// Resolve a child value from a YAML value by its full path (operation: logic only).
fn resolve_yaml_child<'a>(
    value: &'a Value,
    parent_path: &str,
    child_path: &str,
) -> Option<&'a Value> {
    let relative = if parent_path.is_empty() {
        child_path
    } else if let Some(stripped) = child_path.strip_prefix(parent_path) {
        stripped.strip_prefix('.').unwrap_or(stripped)
    } else {
        child_path
    };

    // Handle array index: "[N]"
    if let Some(idx_str) = relative.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let idx: usize = idx_str.parse().ok()?;
        return value.as_sequence()?.get(idx);
    }

    // Handle mapping key
    value.as_mapping()?.get(Value::String(relative.to_string()))
}

/// Process a single YAML mapping entry, creating a chunk if appropriate (operation: logic only).
fn process_yaml_mapping_entry(
    key_str: &str,
    val: &Value,
    entry_ctx: &YamlEntryContext<'_>,
    chunks: &mut Vec<Chunk>,
    cfg: &YamlChunkConfig<'_>,
) {
    let kind = determine_yaml_kind(key_str, val);
    let (start_line, end_line) = find_key_lines(cfg.source, key_str, entry_ctx.depth);
    let content = yaml_value_to_string(val, 0);

    if entry_ctx.depth >= 2 && !is_important_key(key_str) {
        return;
    }

    chunks.push(Chunk {
        id: 0,
        file_id: cfg.file_id,
        start_line,
        end_line,
        start_byte: 0,
        end_byte: 0,
        kind,
        ident: entry_ctx.full_path.to_string(),
        parent: if entry_ctx.parent_path.is_empty() {
            None
        } else {
            Some(entry_ctx.parent_path.to_string())
        },
        signature: Some(format!("{}: {}", key_str, yaml_type_name(val))),
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::text::yaml_helpers::yaml_type_name;

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

    #[test]
    fn test_yaml_type_name() {
        use serde_yaml_ng::Value;
        assert_eq!(yaml_type_name(&Value::Null), "null");
        assert_eq!(yaml_type_name(&Value::Bool(true)), "bool");
        assert_eq!(yaml_type_name(&Value::Bool(false)), "bool");
        assert_eq!(
            yaml_type_name(&Value::Number(serde_yaml_ng::Number::from(42))),
            "number"
        );
        assert_eq!(yaml_type_name(&Value::String("hello".into())), "string");
        assert_eq!(yaml_type_name(&Value::Sequence(vec![])), "array");
        assert_eq!(
            yaml_type_name(&Value::Mapping(serde_yaml_ng::Mapping::new())),
            "object"
        );
        // Tagged values
        assert_eq!(
            yaml_type_name(&Value::Tagged(Box::new(serde_yaml_ng::value::TaggedValue {
                tag: serde_yaml_ng::value::Tag::new("!custom"),
                value: Value::Null,
            }))),
            "tagged"
        );
    }
}
