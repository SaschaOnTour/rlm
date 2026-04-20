//! Tests for `helpers.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "helpers_tests.rs"] mod tests;`.

use super::{extract_signature_to_brace, extract_signature_to_colon, extract_type_signature};
#[test]
fn test_extract_signature_to_brace() {
    assert_eq!(
        extract_signature_to_brace("fn main() { }"),
        Some("fn main()".to_string())
    );
    assert_eq!(extract_signature_to_brace("fn main()"), None);
}

#[test]
fn test_extract_type_signature() {
    assert_eq!(
        extract_type_signature("pub struct Foo { }"),
        Some("pub struct Foo".to_string())
    );
    assert_eq!(
        extract_type_signature("pub struct Foo;"),
        Some("pub struct Foo;".to_string())
    );
}

#[test]
fn test_extract_signature_to_colon() {
    assert_eq!(
        extract_signature_to_colon("def foo(x):\n    pass"),
        Some("def foo(x)".to_string())
    );
}
