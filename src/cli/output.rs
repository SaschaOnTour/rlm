use serde::Serialize;

/// Format a result as minified JSON with token estimates.
pub fn format_json<T: Serialize>(result: &T) -> String {
    serde_json::to_string(result)
        .unwrap_or_else(|e| serde_json::json!({"error": e.to_string()}).to_string())
}

/// Format an error as JSON (guaranteed valid via serde).
pub fn format_error(err: &dyn std::fmt::Display) -> String {
    serde_json::json!({"error": err.to_string()}).to_string()
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
    fn format_error_produces_json() {
        let err = "something went wrong";
        let json = format_error(&err);
        assert!(json.contains("\"error\""));
        assert!(json.contains("something went wrong"));
    }

    #[test]
    fn format_error_escapes_quotes_and_newlines() {
        let msg = "error with \"quotes\" and\nnewlines";
        let json = format_error(&msg);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["error"].as_str().unwrap(), msg);
    }
}
