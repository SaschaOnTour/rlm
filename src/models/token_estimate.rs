use serde::Serialize;

/// Approximate token count using the ~4 chars per token heuristic.
const CHARS_PER_TOKEN: f64 = 4.0;

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

    #[must_use]
    pub fn total(&self) -> u64 {
        self.input + self.output
    }
}

/// Estimate tokens from a character count.
#[must_use]
pub fn estimate_tokens(char_count: usize) -> u64 {
    (char_count as f64 / CHARS_PER_TOKEN).ceil() as u64
}

/// Estimate tokens from a string.
#[must_use]
pub fn estimate_tokens_str(s: &str) -> u64 {
    estimate_tokens(s.len())
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
        let te = TokenEstimate::new(100, 50);
        assert_eq!(te.total(), 150);
    }

    #[test]
    fn estimate_tokens_str_works() {
        let s = "hello world"; // 11 chars
        assert_eq!(estimate_tokens_str(s), 3); // ceil(11/4) = 3
    }
}
