pub mod json_semantic;
pub mod markdown;
pub mod pdf;
pub mod plaintext;
pub mod toml_parser;
pub mod yaml;

use crate::error::Result;
use crate::models::chunk::{Chunk, ChunkKind};

/// Trait for structure-aware text parsers (non-code).
pub trait TextParser: Send + Sync {
    /// Language/format identifier.
    fn format(&self) -> &str;

    /// Parse text content and extract structured chunks (sections, pages).
    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>>;
}

/// Maximum nesting depth for recursive structured-data chunk extraction.
const MAX_NESTING_DEPTH: usize = 3;

/// Shared recursive chunk extractor for JSON-like structured data.
///
/// Both the JSON and TOML parsers convert their parsed data into
/// `serde_json::Value` and then call this function.  The two closures
/// customise the parts that differ between formats:
///
/// * `determine_kind` -- maps (key, value) to a [`ChunkKind`].
/// * `format_signature` -- produces the human-readable signature string.
/// * `find_lines` -- locates the source line range for a key/path.
/// * `value_to_string` -- renders a value as a preview string.
pub fn extract_structured_chunks(
    value: &serde_json::Value,
    path: &str,
    source: &str,
    file_id: i64,
    chunks: &mut Vec<Chunk>,
    depth: usize,
    determine_kind: &impl Fn(&str, &serde_json::Value) -> ChunkKind,
    format_signature: &impl Fn(&str, &serde_json::Value) -> String,
    find_lines: &impl Fn(&str, &str, &str) -> (u32, u32),
    value_to_string: &impl Fn(&serde_json::Value, usize) -> String,
    is_important_key: &impl Fn(&str) -> bool,
) {
    if depth > MAX_NESTING_DEPTH {
        return;
    }

    if let serde_json::Value::Object(obj) = value {
        for (key, val) in obj {
            let full_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };

            let kind = determine_kind(key, val);
            let (start_line, end_line) = find_lines(source, key, &full_path);
            let content = value_to_string(val, 0);

            let should_chunk = matches!(val, serde_json::Value::Object(_) | serde_json::Value::Array(_))
                || depth < 2
                || is_important_key(key);

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
                    signature: Some(format_signature(key, val)),
                    visibility: None,
                    ui_ctx: None,
                    doc_comment: None,
                    attributes: None,
                    content,
                });
            }

            // Recurse into nested objects
            if matches!(val, serde_json::Value::Object(_)) {
                extract_structured_chunks(
                    val, &full_path, source, file_id, chunks, depth + 1,
                    determine_kind, format_signature, find_lines, value_to_string,
                    is_important_key,
                );
            }

            // Handle arrays of objects
            if let serde_json::Value::Array(arr) = val {
                for (i, item) in arr.iter().enumerate() {
                    if matches!(item, serde_json::Value::Object(_)) {
                        let item_path = format!("{full_path}[{i}]");
                        extract_structured_chunks(
                            item, &item_path, source, file_id, chunks, depth + 1,
                            determine_kind, format_signature, find_lines, value_to_string,
                            is_important_key,
                        );
                    }
                }
            }
        }
    }
}

/// Maximum string length before truncation in value previews.
const MAX_PREVIEW_LENGTH: usize = 100;
/// Maximum number of array items shown in a value preview.
const ARRAY_PREVIEW_ITEMS: usize = 5;
/// Maximum number of object keys shown in a value preview.
const OBJECT_PREVIEW_KEYS: usize = 5;

/// Render a `serde_json::Value` as a compact preview string.
///
/// Shared by the JSON and TOML parsers (TOML converts its values to
/// `serde_json::Value` before calling this).
///
/// * `quote_keys` -- if true, object keys are wrapped in `"…"` (JSON style).
/// * `key_sep`    -- separator between key and value (`": "` for JSON, `" = "` for TOML).
pub fn value_preview_string(
    value: &serde_json::Value,
    indent: usize,
    quote_keys: bool,
    key_sep: &str,
) -> String {
    let prefix = "  ".repeat(indent);
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => {
            if s.len() > MAX_PREVIEW_LENGTH {
                format!("\"{}...\"", &s[..MAX_PREVIEW_LENGTH - 3])
            } else {
                format!("\"{s}\"")
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.len() > ARRAY_PREVIEW_ITEMS {
                format!("[...{} items]", arr.len())
            } else {
                let items: Vec<String> = arr
                    .iter()
                    .map(|v| value_preview_string(v, indent + 1, quote_keys, key_sep))
                    .collect();
                format!("[{}]", items.join(", "))
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.len() > OBJECT_PREVIEW_KEYS {
                format!("{{...{} keys}}", obj.len())
            } else {
                let items: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| {
                        let key_str = if quote_keys {
                            format!("\"{}\"", k)
                        } else {
                            k.clone()
                        };
                        format!(
                            "{}{}{}{}",
                            prefix,
                            key_str,
                            key_sep,
                            value_preview_string(v, indent + 1, quote_keys, key_sep)
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

/// Create a single fallback chunk covering the entire file.
///
/// Used by JSON, TOML, and YAML parsers when parsing fails or produces no chunks.
#[must_use]
pub fn create_fallback_chunk(source: &str, file_id: i64, format_kind: &str) -> Chunk {
    let line_count = source.lines().count() as u32;
    Chunk {
        id: 0,
        file_id,
        start_line: 1,
        end_line: line_count.max(1),
        start_byte: 0,
        end_byte: source.len() as u32,
        kind: ChunkKind::Other(format_kind.into()),
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
