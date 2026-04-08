use serde::Serialize;

use crate::models::token_estimate::{estimate_tokens, TokenEstimate};

/// Token-optimized JSON output wrapper.
#[derive(Debug, Serialize)]
pub struct Output<T: Serialize> {
    #[serde(rename = "r")]
    pub result: T,
    #[serde(rename = "t")]
    pub tokens: TokenEstimate,
}

/// Format a result as minified JSON with token estimates.
pub fn format_json<T: Serialize>(result: &T) -> String {
    serde_json::to_string(result).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}

/// Format a result with token estimates wrapper.
pub fn format_with_tokens<T: Serialize>(result: T) -> String {
    let json = serde_json::to_string(&result).unwrap_or_default();
    let out_tokens = estimate_tokens(json.len());
    let output = Output {
        result,
        tokens: TokenEstimate::new(0, out_tokens),
    };
    serde_json::to_string(&output).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}

/// Format an error as JSON.
pub fn format_error(err: &dyn std::fmt::Display) -> String {
    format!("{{\"error\":\"{}\"}}", err.to_string().replace('"', "\\\""))
}

/// Quality warning for parse results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct QualityWarning {
    pub fallback_recommended: bool,
    #[serde(rename = "el")]
    pub error_lines: Vec<u32>,
    #[serde(rename = "m")]
    pub message: String,
}

impl QualityWarning {
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Arbitrary test value used as a representative integer payload.
    const TEST_VALUE: i32 = 42;

    #[derive(Serialize)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[test]
    fn format_json_minified() {
        let data = TestData {
            name: "test".into(),
            value: TEST_VALUE,
        };
        let json = format_json(&data);
        assert!(!json.contains('\n'));
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn format_with_tokens_includes_estimates() {
        let data = TestData {
            name: "test".into(),
            value: TEST_VALUE,
        };
        let json = format_with_tokens(data);
        assert!(json.contains("\"t\":{"));
        assert!(json.contains("\"in\":"));
    }

    #[test]
    fn format_error_produces_json() {
        let err = "something went wrong";
        let json = format_error(&err);
        assert!(json.contains("\"error\""));
        assert!(json.contains("something went wrong"));
    }
}
