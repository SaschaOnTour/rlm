use serde::Serialize;

/// Bytes per token for plain text (~4 bytes/token heuristic).
const BYTES_PER_TOKEN: f64 = 4.0;

/// Bytes per token for JSON content (Claude Code uses denser tokenization for JSON).
const JSON_BYTES_PER_TOKEN: f64 = 2.0;

/// Token usage estimate for an operation.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TokenEstimate {
    /// Estimated input tokens consumed.
    #[serde(rename = "in")]
    pub input: u64,
    /// Estimated output tokens produced.
    #[serde(rename = "out")]
    pub output: u64,
}

impl TokenEstimate {
    #[must_use]
    pub fn new(input: u64, output: u64) -> Self {
        Self { input, output }
    }
}

#[cfg(test)]
impl TokenEstimate {
    #[must_use]
    fn total(&self) -> u64 {
        self.input + self.output
    }
}

/// Estimate tokens from a byte count (plain text at ~4 bytes/token).
///
/// In Rust, `String::len()` and `str::len()` return byte counts, not char counts.
/// For ASCII-dominated source code, bytes ≈ chars.
#[must_use]
pub fn estimate_tokens(byte_count: usize) -> u64 {
    (byte_count as f64 / BYTES_PER_TOKEN).ceil() as u64
}

/// Estimate tokens from a string's byte length.
#[must_use]
pub fn estimate_tokens_str(s: &str) -> u64 {
    estimate_tokens(s.len())
}

/// Estimate tokens for JSON content (CC uses 2 bytes/token for JSON).
///
/// Takes a byte count (e.g., `String::len()`) — in Rust, `len()` returns bytes, not chars.
#[must_use]
pub fn estimate_json_tokens(byte_count: usize) -> u64 {
    (byte_count as f64 / JSON_BYTES_PER_TOKEN).ceil() as u64
}

/// Estimate tokens from a byte count (file size).
///
/// Uses the same ~4 chars/token heuristic as `estimate_tokens`.
#[must_use]
pub fn estimate_tokens_from_bytes(size_bytes: u64) -> u64 {
    (size_bytes as f64 / BYTES_PER_TOKEN).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_basic() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(5), 2);
        assert_eq!(estimate_tokens(100), 25);
    }

    #[test]
    fn token_estimate_total() {
        const INPUT_TOKENS: u64 = 100;
        const OUTPUT_TOKENS: u64 = 50;

        let te = TokenEstimate::new(INPUT_TOKENS, OUTPUT_TOKENS);
        assert_eq!(te.total(), INPUT_TOKENS + OUTPUT_TOKENS);
    }

    #[test]
    fn estimate_tokens_str_works() {
        let s = "hello world"; // 11 chars
        assert_eq!(estimate_tokens_str(s), 3); // ceil(11/4) = 3
    }

    #[test]
    fn estimate_tokens_from_bytes_works() {
        assert_eq!(estimate_tokens_from_bytes(0), 0);
        assert_eq!(estimate_tokens_from_bytes(4), 1);
        assert_eq!(estimate_tokens_from_bytes(1024), 256);
        assert_eq!(estimate_tokens_from_bytes(5), 2); // ceil(5/4) = 2
    }

    #[test]
    fn estimate_json_tokens_uses_tighter_ratio() {
        // CC tokenizes JSON at 2 bytes/token (denser than plain text at 4)
        assert_eq!(estimate_json_tokens(0), 0);
        assert_eq!(estimate_json_tokens(2), 1);
        assert_eq!(estimate_json_tokens(400), 200);
        assert_eq!(estimate_json_tokens(1), 1); // ceil(1/2)
    }
}
