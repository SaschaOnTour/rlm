//! Format parsing / reformat tests for `output.rs`.
//!
//! Split out of `output_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Serialize / error-envelope
//! tests stay in `output_tests.rs`; this file covers `from_str_loose`
//! format resolution and the `reformat` JSON→TOON/Pretty/JSON pipeline.

use super::{reformat, Formatter, OutputFormat};

#[test]
fn format_from_str_loose_is_case_insensitive_and_permissive() {
    assert_eq!(
        Formatter::from_str_loose("TOON").format(),
        OutputFormat::Toon
    );
    assert_eq!(
        Formatter::from_str_loose("Pretty").format(),
        OutputFormat::Pretty
    );
    assert_eq!(
        Formatter::from_str_loose("json").format(),
        OutputFormat::Json
    );
    // Legacy alias + unknown input both fall through to JSON.
    assert_eq!(
        Formatter::from_str_loose("minified").format(),
        OutputFormat::Json
    );
    assert_eq!(
        Formatter::from_str_loose("banana").format(),
        OutputFormat::Json
    );
}

#[test]
fn reformat_json_borrows() {
    let json = r#"{"a":1}"#;
    let cow = reformat(json, OutputFormat::Json);
    assert!(matches!(cow, std::borrow::Cow::Borrowed(_)));
    assert_eq!(&*cow, json);
}

#[test]
fn reformat_toon_produces_toon() {
    let json = r#"[{"name":"a","value":1},{"name":"b","value":2}]"#;
    let cow = reformat(json, OutputFormat::Toon);
    assert!(matches!(cow, std::borrow::Cow::Owned(_)));
    assert!(cow.contains("[2]{name,value}:"), "got: {cow}");
}

#[test]
fn reformat_pretty_indents() {
    let json = r#"{"a":1}"#;
    let cow = reformat(json, OutputFormat::Pretty);
    assert!(
        cow.contains('\n'),
        "pretty should have newlines, got: {cow}"
    );
}
