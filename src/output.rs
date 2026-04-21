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
    /// `success_text` and the free `print_str` helper.
    pub fn reformat_str<'a>(self, json: &'a str) -> Cow<'a, str> {
        reformat(json, self.format)
    }
}

/// Print a `Serialize` result to stdout using the given formatter.
pub fn print<T: Serialize>(formatter: Formatter, result: &T) {
    println!("{}", formatter.serialize(result));
}

/// Print a pre-serialized JSON string to stdout, reformatted as needed.
pub fn print_str(formatter: Formatter, json: &str) {
    println!("{}", formatter.reformat_str(json));
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
#[path = "output_format_tests.rs"]
mod format_tests;
#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
