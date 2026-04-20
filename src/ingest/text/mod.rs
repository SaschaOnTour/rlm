pub mod json_semantic;
pub mod markdown;
pub mod pdf;
pub mod plaintext;
pub mod toml_parser;
pub mod yaml;
pub mod yaml_helpers;

use crate::domain::chunk::{Chunk, ChunkKind};
use crate::error::Result;

/// Trait for structure-aware text parsers (non-code).
pub trait TextParser: Send + Sync {
    /// Language/format identifier.
    fn format(&self) -> &str;

    /// Parse text content and extract structured chunks (sections, pages).
    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>>;
}

/// Maximum nesting depth for recursive structured-data chunk extraction.
const MAX_NESTING_DEPTH: usize = 3;

/// Configuration bundle for structured-chunk extraction, reducing parameter count.
pub struct StructuredChunkConfig<'a, FK, FS, FL, FV, FI>
where
    FK: Fn(&str, &serde_json::Value) -> ChunkKind,
    FS: Fn(&str, &serde_json::Value) -> String,
    FL: Fn(&str, &str, &str) -> (u32, u32),
    FV: Fn(&serde_json::Value, usize) -> String,
    FI: Fn(&str) -> bool,
{
    pub source: &'a str,
    pub file_id: i64,
    pub determine_kind: FK,
    pub format_signature: FS,
    pub find_lines: FL,
    pub value_to_string: FV,
    pub is_important_key: FI,
}

/// Per-entry context for structured chunk processing, reducing parameter count.
pub struct EntryContext<'a> {
    /// Full dot-notation path for this entry.
    pub full_path: &'a str,
    /// Parent path (dot-notation) for hierarchy.
    pub parent_path: &'a str,
    /// Current nesting depth.
    pub depth: usize,
}

/// Return type for `collect_structured_entries`: entries paired with recurse targets.
type StructuredEntries = (Vec<(String, String)>, Vec<RecurseTarget>);

/// Describes a child entry that should be recursed into.
struct RecurseTarget {
    /// The `serde_json::Value` index inside the parent object.
    key: String,
    /// Full dot-notation path for this entry.
    full_path: String,
}

/// Identify which children of an object value need recursive extraction (operation: logic only).
///
/// Returns `(entries, recurse_targets)` where `entries` are `(key, full_path)` pairs
/// for every object member, and `recurse_targets` lists members that should be recursed into
/// (nested objects and arrays containing objects).
/// Build the full dot-notation path for a key.
fn build_full_path(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// Collect recurse targets from a single value entry.
fn collect_recurse_targets(
    key: &str,
    val: &serde_json::Value,
    full_path: &str,
    targets: &mut Vec<RecurseTarget>,
) {
    if matches!(val, serde_json::Value::Object(_)) {
        targets.push(RecurseTarget {
            key: key.to_string(),
            full_path: full_path.to_string(),
        });
    } else if let serde_json::Value::Array(arr) = val {
        for (i, item) in arr.iter().enumerate() {
            if matches!(item, serde_json::Value::Object(_)) {
                targets.push(RecurseTarget {
                    key: format!("{key}[{i}]"),
                    full_path: format!("{full_path}[{i}]"),
                });
            }
        }
    }
}

fn collect_structured_entries(
    value: &serde_json::Value,
    path: &str,
    depth: usize,
) -> Option<StructuredEntries> {
    if depth > MAX_NESTING_DEPTH {
        return None;
    }

    let obj = match value {
        serde_json::Value::Object(obj) => obj,
        _ => return None,
    };

    let mut entries = Vec::new();
    let mut targets = Vec::new();

    for (key, val) in obj {
        let full_path = build_full_path(path, key);
        entries.push((key.clone(), full_path.clone()));
        collect_recurse_targets(key, val, &full_path, &mut targets);
    }

    Some((entries, targets))
}

/// Shared recursive chunk extractor for JSON-like structured data (integration: calls only).
///
/// Both the JSON and TOML parsers convert their parsed data into
/// `serde_json::Value` and then call this function.  Delegates entry analysis
/// to `collect_structured_entries` and per-entry processing to `process_structured_entry`.
// qual:recursive
pub fn extract_structured_chunks<FK, FS, FL, FV, FI>(
    value: &serde_json::Value,
    path: &str,
    chunks: &mut Vec<Chunk>,
    depth: usize,
    cfg: &StructuredChunkConfig<'_, FK, FS, FL, FV, FI>,
) where
    FK: Fn(&str, &serde_json::Value) -> ChunkKind,
    FS: Fn(&str, &serde_json::Value) -> String,
    FL: Fn(&str, &str, &str) -> (u32, u32),
    FV: Fn(&serde_json::Value, usize) -> String,
    FI: Fn(&str) -> bool,
{
    let (entries, targets) = match collect_structured_entries(value, path, depth) {
        Some(pair) => pair,
        None => return,
    };

    let obj = match value {
        serde_json::Value::Object(obj) => obj,
        _ => return,
    };

    for (key, full_path) in &entries {
        let val = match obj.get(key) {
            Some(v) => v,
            None => continue,
        };
        let entry_ctx = EntryContext {
            full_path,
            parent_path: path,
            depth,
        };
        process_structured_entry(key, val, &entry_ctx, chunks, cfg);
    }

    for target in &targets {
        // Navigate to the target value via the path
        let val = resolve_json_path(value, &target.key);
        let val = match val {
            Some(v) => v,
            None => continue,
        };
        extract_structured_chunks(val, &target.full_path, chunks, depth + 1, cfg);
    }
}

/// Resolve a JSON path segment to a value (operation: logic only).
///
/// Handles both direct keys ("foo") and array-indexed keys ("foo[2]").
fn resolve_json_path<'a>(
    value: &'a serde_json::Value,
    path_segment: &str,
) -> Option<&'a serde_json::Value> {
    if let Some(bracket_pos) = path_segment.find('[') {
        let key = &path_segment[..bracket_pos];
        let idx_str = &path_segment[bracket_pos + 1..path_segment.len() - 1];
        let idx: usize = idx_str.parse().ok()?;
        value.get(key)?.as_array()?.get(idx)
    } else {
        value.get(path_segment)
    }
}

/// Process a single key-value entry from structured data (operation: logic only).
///
/// Determines whether the entry warrants a chunk and, if so, builds and
/// pushes the `Chunk`.  No calls to own functions.
fn process_structured_entry<FK, FS, FL, FV, FI>(
    key: &str,
    val: &serde_json::Value,
    entry_ctx: &EntryContext<'_>,
    chunks: &mut Vec<Chunk>,
    cfg: &StructuredChunkConfig<'_, FK, FS, FL, FV, FI>,
) where
    FK: Fn(&str, &serde_json::Value) -> ChunkKind,
    FS: Fn(&str, &serde_json::Value) -> String,
    FL: Fn(&str, &str, &str) -> (u32, u32),
    FV: Fn(&serde_json::Value, usize) -> String,
    FI: Fn(&str) -> bool,
{
    let kind = (cfg.determine_kind)(key, val);
    let (start_line, end_line) = (cfg.find_lines)(cfg.source, key, entry_ctx.full_path);
    let content = (cfg.value_to_string)(val, 0);

    let should_chunk = matches!(
        val,
        serde_json::Value::Object(_) | serde_json::Value::Array(_)
    ) || entry_ctx.depth < 2
        || (cfg.is_important_key)(key);

    if !should_chunk {
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
        signature: Some((cfg.format_signature)(key, val)),
        visibility: None,
        ui_ctx: None,
        doc_comment: None,
        attributes: None,
        content,
    });
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
