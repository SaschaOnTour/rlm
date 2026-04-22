//! Tests for `response.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "response_tests.rs"] mod tests;`.

use super::OperationResponse;
#[test]
fn response_preserves_body_and_tokens() {
    let r = OperationResponse::new("{\"result\":42}".into(), 5);
    assert_eq!(r.body, "{\"result\":42}");
    assert_eq!(r.tokens_out, 5);
}

#[test]
fn response_is_cloneable() {
    let r = OperationResponse::new("payload".into(), 1);
    let cloned = r.clone();
    assert_eq!(cloned.body, r.body);
    assert_eq!(cloned.tokens_out, r.tokens_out);
}
