//! Token budget tracking: estimates for operation input/output size.

use serde::Serialize;

/// Bytes per token for plain text (~4 bytes/token heuristic).
const BYTES_PER_TOKEN: f64 = 4.0;

/// Bytes per token for JSON content (Claude Code uses denser tokenization for JSON).
const JSON_BYTES_PER_TOKEN: f64 = 2.0;

/// Token usage estimate for an operation.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TokenEstimate {
    /// Estimated input tokens consumed.
    pub input: u64,
    /// Estimated output tokens produced.
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

/// Estimate output tokens for a Serialize result by iterating to a fixed point.
///
/// The result's own `tokens.output` field contributes to the JSON payload
/// length, but its value is derived from that length — a circular dependency
/// that causes a systematic undercount when callers overwrite a default
/// (1-digit "0") placeholder with a multi-digit estimate. A simple two-pass
/// also drifts by one at digit boundaries (e.g., 99 → 100 adds a digit).
///
/// Solution: substitute the running estimate into `tokens.output` and remeasure
/// until the estimate stabilizes. Digit counts change only at 10x boundaries so
/// convergence is fast — 2-3 iterations in practice. `MAX_ITERATIONS` guards
/// against pathological inputs.
#[must_use]
pub fn estimate_output_tokens<T: serde::Serialize>(result: &T) -> TokenEstimate {
    const MAX_ITERATIONS: u32 = 5;

    let mut value = serde_json::to_value(result).unwrap_or(serde_json::Value::Null);
    let mut estimate: u64 = 0;
    for _ in 0..MAX_ITERATIONS {
        if let Some(tokens) = value.get_mut("tokens").and_then(|v| v.as_object_mut()) {
            tokens.insert("output".to_string(), serde_json::json!(estimate));
        }
        let bytes = serde_json::to_string(&value).map(|s| s.len()).unwrap_or(0);
        let next = estimate_json_tokens(bytes);
        if next == estimate {
            break;
        }
        estimate = next;
    }
    TokenEstimate::new(0, estimate)
}

#[cfg(test)]
#[path = "token_budget_tests.rs"]
mod tests;
