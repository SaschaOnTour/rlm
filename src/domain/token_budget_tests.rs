//! Tests for `token_budget.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "token_budget_tests.rs"] mod tests;`.

use super::{
    estimate_json_tokens, estimate_output_tokens, estimate_tokens, estimate_tokens_from_bytes,
    estimate_tokens_str, TokenEstimate,
};
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

#[test]
fn estimate_output_tokens_accounts_for_its_own_digit_length() {
    // Regression test: a naive single-pass estimate undercounts because
    // tokens.output's digit length (e.g., "0" → "275") changes the JSON
    // payload size after the caller writes the estimate back.
    const PAYLOAD_CHARS: usize = 500;

    #[derive(serde::Serialize)]
    struct Wrapper {
        data: String,
        tokens: TokenEstimate,
    }

    let w = Wrapper {
        data: "x".repeat(PAYLOAD_CHARS),
        tokens: TokenEstimate::default(),
    };
    let estimate = estimate_output_tokens(&w);

    // Simulate the final output the caller will emit: write the estimate
    // back into tokens and re-serialize. The estimate must exactly match
    // the token count of that final payload.
    let final_w = Wrapper {
        data: w.data,
        tokens: TokenEstimate::new(estimate.input, estimate.output),
    };
    let final_json = serde_json::to_string(&final_w).unwrap();
    let actual_tokens = estimate_json_tokens(final_json.len());

    assert_eq!(estimate.output, actual_tokens);
}
