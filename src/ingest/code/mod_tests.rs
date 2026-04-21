//! Tests for `mod.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "mod_tests.rs"] mod tests;`.

use super::ParseQuality;
#[test]
fn parse_quality_complete_is_complete() {
    assert!(ParseQuality::Complete.is_complete());
    assert!(!ParseQuality::Complete.fallback_recommended());
}

#[test]
fn parse_quality_partial_recommends_fallback() {
    let partial = ParseQuality::Partial {
        error_count: 2,
        error_lines: vec![5, 10],
    };
    assert!(!partial.is_complete());
    assert!(partial.fallback_recommended());
}

#[test]
fn parse_quality_failed_recommends_fallback() {
    let failed = ParseQuality::Failed {
        reason: "unknown syntax".into(),
    };
    assert!(!failed.is_complete());
    assert!(failed.fallback_recommended());
}
