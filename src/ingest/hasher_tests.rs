//! Tests for `hasher.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "hasher_tests.rs"] mod tests;`.

use super::{hash_bytes, hash_file};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn hash_bytes_deterministic() {
    let h1 = hash_bytes(b"hello world");
    let h2 = hash_bytes(b"hello world");
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
}

#[test]
fn hash_bytes_different_for_different_input() {
    let h1 = hash_bytes(b"hello");
    let h2 = hash_bytes(b"world");
    assert_ne!(h1, h2);
}

#[test]
fn hash_file_works() {
    let mut tmp = NamedTempFile::new().unwrap();
    write!(tmp, "test content").unwrap();
    let h = hash_file(tmp.path()).unwrap();
    assert_eq!(h.len(), 64);
    assert_eq!(h, hash_bytes(b"test content"));
}
