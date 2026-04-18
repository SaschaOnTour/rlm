//! Output formatting — `Formatter` context object.
//!
//! CLI and MCP adapters each carry a `Formatter` instance which determines
//! how responses are encoded (minified JSON, pretty-printed JSON, or TOON).
//! Nothing in this module is global: instantiate a `Formatter` per adapter
//! and thread it through the call path.

use std::borrow::Cow;

use serde::Serialize;

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
    /// Accepted values: "json" (default), "pretty", "toon". "minified" is kept as a
    /// backward-compatible alias for "json" (older configs).
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "toon" => Self::Toon,
            "pretty" => Self::Pretty,
            "json" | "minified" => Self::Json,
            _ => Self::Json,
        }
    }
}

/// Per-adapter output formatter.
///
/// `Formatter` is `Copy` and small (a single enum discriminant), so call sites
/// can pass it by value freely. Each adapter constructs one instance from its
/// configuration and threads it through handlers.
#[derive(Debug, Clone, Copy)]
pub struct Formatter {
    format: OutputFormat,
}

impl Default for Formatter {
    fn default() -> Self {
        Self::new(OutputFormat::Json)
    }
}

impl Formatter {
    #[must_use]
    pub const fn new(format: OutputFormat) -> Self {
        Self { format }
    }

    /// Construct a formatter from a config-style format string.
    #[must_use]
    pub fn from_str_loose(s: &str) -> Self {
        Self::new(OutputFormat::from_str_loose(s))
    }

    #[must_use]
    pub const fn format(self) -> OutputFormat {
        self.format
    }

    /// Serialize a result in the configured format.
    pub fn serialize<T: Serialize>(self, result: &T) -> String {
        match self.format {
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
    ///
    /// If the configured formatter fails (rare — `serde_json::Value` is
    /// effectively infallible), falls back to minified JSON so the error
    /// payload is never silently dropped.
    pub fn serialize_error(self, err: &dyn std::fmt::Display) -> String {
        let error_obj = serde_json::json!({"error": err.to_string()});
        match self.format {
            OutputFormat::Json => error_obj.to_string(),
            OutputFormat::Pretty => {
                serde_json::to_string_pretty(&error_obj).unwrap_or_else(|_| error_obj.to_string())
            }
            OutputFormat::Toon => toon_encode::encode_toon(&error_obj, 0),
        }
    }

    /// Re-format a pre-serialized JSON string in the configured format.
    ///
    /// Borrows for JSON (zero-cost); owns for TOON/Pretty. Used by MCP
    /// `success_text` and CLI `print_str`.
    pub fn reformat_str<'a>(self, json: &'a str) -> Cow<'a, str> {
        reformat(json, self.format)
    }

    /// Print a `Serialize` result to stdout.
    pub fn print<T: Serialize>(self, result: &T) {
        println!("{}", self.serialize(result));
    }

    /// Print a pre-serialized JSON string, reformatted as needed.
    pub fn print_str(self, json: &str) {
        println!("{}", self.reformat_str(json));
    }
}

/// Serialize a result as minified JSON. Format-independent; used for internal
/// savings recording and fallback paths.
pub(crate) fn to_json<T: Serialize>(result: &T) -> String {
    serde_json::to_string(result)
        .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
}

/// Re-format a JSON string according to the given output format.
///
/// Returns `Cow::Borrowed` for JSON (no allocation). For TOON/Pretty,
/// parses and re-encodes.
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
    fn default_formatter_is_json() {
        let f = Formatter::default();
        assert_eq!(f.format(), OutputFormat::Json);
    }

    #[test]
    fn formatter_serialize_json_is_minified() {
        let f = Formatter::new(OutputFormat::Json);
        let data = TestData {
            name: "test".into(),
            value: TEST_VALUE,
        };
        let json = f.serialize(&data);
        assert!(!json.contains('\n'));
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn formatter_serialize_pretty_indents() {
        let f = Formatter::new(OutputFormat::Pretty);
        let data = TestData {
            name: "test".into(),
            value: TEST_VALUE,
        };
        let out = f.serialize(&data);
        assert!(out.contains('\n'));
    }

    #[test]
    fn serialize_error_produces_json() {
        let f = Formatter::new(OutputFormat::Json);
        let err = "something went wrong";
        let json = f.serialize_error(&err);
        assert!(json.contains("error"));
        assert!(json.contains("something went wrong"));
    }

    #[test]
    fn serialize_error_escapes_quotes_and_newlines() {
        let f = Formatter::new(OutputFormat::Json);
        let msg = "error with \"quotes\" and\nnewlines";
        let output = f.serialize_error(&msg);
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(parsed["error"].as_str().unwrap(), msg);
    }

    #[test]
    fn format_from_str_loose_is_case_insensitive_and_permissive() {
        assert_eq!(
            Formatter::from_str_loose("TOON").format(),
            OutputFormat::Toon
        );
        assert_eq!(
            Formatter::from_str_loose("Pretty").format(),
            OutputFormat::Pretty
        );
        assert_eq!(
            Formatter::from_str_loose("json").format(),
            OutputFormat::Json
        );
        // Legacy alias + unknown input both fall through to JSON.
        assert_eq!(
            Formatter::from_str_loose("minified").format(),
            OutputFormat::Json
        );
        assert_eq!(
            Formatter::from_str_loose("banana").format(),
            OutputFormat::Json
        );
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
            cow.contains('\n'),
            "pretty should have newlines, got: {cow}"
        );
    }
}
