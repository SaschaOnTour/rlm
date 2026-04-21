//! Serialize / error-envelope tests for `output.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "output_tests.rs"] mod tests;`.
//!
//! Format-parsing / reformat tests live in the sibling
//! `output_format_tests.rs`.

use super::{Formatter, OutputFormat};
use serde::Serialize;

const TEST_VALUE: i32 = 42;

#[derive(Serialize)]
struct TestData {
    name: String,
    value: i32,
}

#[test]
fn default_formatter_is_json() {
    let f = Formatter::default();
    assert_eq!(f.format(), OutputFormat::Json);
}

#[test]
fn formatter_serialize_json_is_minified() {
    let f = Formatter::new(OutputFormat::Json);
    let data = TestData {
        name: "test".into(),
        value: TEST_VALUE,
    };
    let json = f.serialize(&data);
    assert!(!json.contains('\n'));
    assert!(json.contains("\"name\":\"test\""));
}

#[test]
fn formatter_serialize_pretty_indents() {
    let f = Formatter::new(OutputFormat::Pretty);
    let data = TestData {
        name: "test".into(),
        value: TEST_VALUE,
    };
    let out = f.serialize(&data);
    assert!(out.contains('\n'));
}

#[test]
fn serialize_error_produces_json() {
    let f = Formatter::new(OutputFormat::Json);
    let err = "something went wrong";
    let json = f.serialize_error(&err);
    assert!(json.contains("error"));
    assert!(json.contains("something went wrong"));
}

#[test]
fn serialize_error_escapes_quotes_and_newlines() {
    let f = Formatter::new(OutputFormat::Json);
    let msg = "error with \"quotes\" and\nnewlines";
    let output = f.serialize_error(&msg);
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed["error"].as_str().unwrap(), msg);
}
