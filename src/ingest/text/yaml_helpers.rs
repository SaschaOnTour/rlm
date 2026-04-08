//! YAML-specific helper functions for chunk extraction.
//!
//! Extracted from `yaml.rs` for SRP compliance. Contains kind determination,
//! importance checks, line finding, type naming, and value formatting.

use serde_yaml::Value;

use crate::models::chunk::ChunkKind;

/// Maximum string length before truncation in YAML value previews.
const MAX_PREVIEW_LENGTH: usize = 100;
/// Maximum number of sequence items shown in a YAML value preview.
const ARRAY_PREVIEW_ITEMS: usize = 5;
/// Maximum number of mapping keys shown in a YAML value preview.
const OBJECT_PREVIEW_KEYS: usize = 5;

pub fn determine_yaml_kind(key: &str, value: &Value) -> ChunkKind {
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

pub fn is_important_key(key: &str) -> bool {
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

/// Check whether a YAML line starts a given key.
fn is_yaml_key_start(trimmed: &str, search_key: &str, quoted_key: &str) -> bool {
    trimmed.starts_with(search_key) || trimmed.starts_with(quoted_key)
}

/// Find the end line of a YAML block that starts at `start_line` (1-based).
fn find_yaml_block_end(lines: &[&str], start_line: u32) -> u32 {
    let start_indent = lines
        .get((start_line - 1) as usize)
        .map_or(0, |l| l.len() - l.trim_start().len());

    for (i, line) in lines.iter().enumerate().skip(start_line as usize) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let current_indent = line.len() - line.trim_start().len();
        if current_indent <= start_indent {
            return i as u32;
        }
    }
    lines.len() as u32
}

pub fn find_key_lines(source: &str, key: &str, _depth: usize) -> (u32, u32) {
    let lines: Vec<&str> = source.lines().collect();
    let search_key = format!("{key}:");
    let quoted_key = format!("\"{key}\":");

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if is_yaml_key_start(trimmed, &search_key, &quoted_key) {
            let start_line = (i + 1) as u32;
            let end_line = find_yaml_block_end(&lines, start_line);
            return (start_line, end_line);
        }
    }

    // Not found: span entire file
    (1, lines.len() as u32)
}

pub fn yaml_type_name(value: &Value) -> &'static str {
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

pub fn yaml_value_to_string(value: &Value, indent: usize) -> String {
    let prefix = "  ".repeat(indent);
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => {
            if s.len() > MAX_PREVIEW_LENGTH {
                format!("\"{}...\"", &s[..MAX_PREVIEW_LENGTH - 3])
            } else {
                format!("\"{s}\"")
            }
        }
        Value::Sequence(seq) => {
            if seq.len() > ARRAY_PREVIEW_ITEMS {
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
            if map.len() > OBJECT_PREVIEW_KEYS {
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

/// Convert a YAML key value to a string (operation: logic only).
pub fn yaml_key_to_string(key: &Value) -> Option<String> {
    match key {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}
