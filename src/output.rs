use std::borrow::Cow;
use std::sync::OnceLock;

use serde::Serialize;

/// Global output format, set once at startup.
static FORMAT: OnceLock<OutputFormat> = OnceLock::new();

/// Emit progress every N files to avoid flooding output (shared by CLI + MCP).
pub const PROGRESS_INTERVAL: usize = 50;

/// Output format for CLI and MCP responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Minified JSON (default).
    Json,
    /// Pretty-printed JSON.
    Pretty,
    /// Token-Oriented Object Notation — compact tabular format for LLM output.
    Toon,
}

impl OutputFormat {
    /// Parse from a string (config value). Case-insensitive, unknown values default to JSON.
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "toon" => Self::Toon,
            "pretty" => Self::Pretty,
            _ => Self::Json,
        }
    }
}

/// Serialize a result as minified JSON (internal use for savings recording + fallback).
pub(crate) fn to_json<T: Serialize>(result: &T) -> String {
    serde_json::to_string(result)
        .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
}

/// Serialize a result in the configured format.
pub fn serialize<T: Serialize>(result: &T) -> String {
    match current_format() {
        OutputFormat::Json => to_json(result),
        OutputFormat::Pretty => {
            serde_json::to_string_pretty(result).unwrap_or_else(|_| to_json(result))
        }
        OutputFormat::Toon => {
            toon_encode::to_toon_string(result).unwrap_or_else(|_| to_json(result))
        }
    }
}

/// Serialize an error in the configured format.
pub fn serialize_error(err: &dyn std::fmt::Display) -> String {
    let error_obj = serde_json::json!({"error": err.to_string()});
    match current_format() {
        OutputFormat::Json => error_obj.to_string(),
        OutputFormat::Pretty => serde_json::to_string_pretty(&error_obj).unwrap_or_default(),
        OutputFormat::Toon => toon_encode::encode_toon(&error_obj, 0),
    }
}

/// Set the output format once at startup (CLI --format flag or config).
pub fn init(format: OutputFormat) {
    let _ = FORMAT.set(format);
}

/// Get the currently configured output format.
fn current_format() -> OutputFormat {
    FORMAT.get().copied().unwrap_or(OutputFormat::Json)
}

/// Print any Serialize result in the configured format to stdout.
pub fn print<T: Serialize>(result: &T) {
    println!("{}", serialize(result));
}

/// Print a pre-serialized JSON string in the configured format to stdout.
///
/// For JSON, passes through unchanged. For TOON/Pretty, re-parses and re-encodes.
/// Used by handlers that receive JSON from savings recording functions.
pub fn print_str(json: &str) {
    println!("{}", reformat(json, current_format()));
}

/// Re-format a pre-serialized JSON string according to the configured format.
///
/// Returns a `Cow` — borrows for JSON (zero-cost), owned for TOON/Pretty.
/// Used by MCP `success_text` and CLI `print_str`.
pub fn reformat_str(json: &str) -> Cow<'_, str> {
    reformat(json, current_format())
}

/// Re-format a JSON string according to the given output format.
///
/// Returns `Cow::Borrowed` for JSON (no allocation). For TOON/Pretty, parses and re-encodes.
fn reformat<'a>(json: &'a str, format: OutputFormat) -> Cow<'a, str> {
    match format {
        OutputFormat::Json => Cow::Borrowed(json),
        OutputFormat::Toon => Cow::Owned(
            serde_json::from_str::<serde_json::Value>(json)
                .map(|v| toon_encode::encode_toon(&v, 0))
                .unwrap_or_else(|_| json.to_string()),
        ),
        OutputFormat::Pretty => Cow::Owned(
            serde_json::from_str::<serde_json::Value>(json)
                .and_then(|v| serde_json::to_string_pretty(&v))
                .unwrap_or_else(|_| json.to_string()),
        ),
    }
}

/// Quality warning for parse results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct QualityWarning {
    pub fallback_recommended: bool,
    pub error_lines: Vec<u32>,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_VALUE: i32 = 42;

    #[derive(Serialize)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[test]
    fn serialize_default_json() {
        let data = TestData {
            name: "test".into(),
            value: TEST_VALUE,
        };
        let json = serialize(&data);
        assert!(!json.contains('\n'));
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn serialize_error_produces_json() {
        let err = "something went wrong";
        let json = serialize_error(&err);
        assert!(json.contains("error"));
        assert!(json.contains("something went wrong"));
    }

    #[test]
    fn serialize_error_escapes_quotes_and_newlines() {
        let msg = "error with \"quotes\" and\nnewlines";
        // Default format is JSON (OnceLock not set in tests)
        let output = serialize_error(&msg);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["error"].as_str().unwrap(), msg);
    }

    #[test]
    fn reformat_json_borrows() {
        let json = r#"{"a":1}"#;
        let cow = reformat(json, OutputFormat::Json);
        assert!(matches!(cow, std::borrow::Cow::Borrowed(_)));
        assert_eq!(&*cow, json);
    }

    #[test]
    fn reformat_toon_produces_toon() {
        let json = r#"[{"name":"a","value":1},{"name":"b","value":2}]"#;
        let cow = reformat(json, OutputFormat::Toon);
        assert!(matches!(cow, std::borrow::Cow::Owned(_)));
        assert!(cow.contains("[2]{name,value}:"), "got: {cow}");
    }

    #[test]
    fn reformat_pretty_indents() {
        let json = r#"{"a":1}"#;
        let cow = reformat(json, OutputFormat::Pretty);
        assert!(
            cow.contains("\n"),
            "pretty should have newlines, got: {cow}"
        );
    }
}
