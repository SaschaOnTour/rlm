//! Tests for `validator.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "validator_tests.rs"] mod tests;`.

use super::{validate_and_write, SyntaxGuard};
use tempfile::TempDir;

#[test]
fn validate_valid_rust() {
    let guard = SyntaxGuard::new();
    assert!(guard.validate("rust", "fn main() {}").is_ok());
}

#[test]
fn validate_invalid_rust_rejects() {
    let guard = SyntaxGuard::new();
    let result = guard.validate("rust", "fn main() {");
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("syntax"));
}

#[test]
fn validate_markdown_always_passes() {
    let guard = SyntaxGuard::new();
    assert!(guard.validate("markdown", "any content").is_ok());
}

#[test]
fn validate_and_write_valid() {
    let guard = SyntaxGuard::new();
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.rs");
    validate_and_write(&guard, "rust", "fn main() {}", &path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "fn main() {}");
}

#[test]
fn validate_and_write_invalid_rejects() {
    let guard = SyntaxGuard::new();
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("test.rs");
    let result = validate_and_write(&guard, "rust", "fn main() {", &path);
    assert!(result.is_err());
    assert!(!path.exists()); // File should NOT be written
}
