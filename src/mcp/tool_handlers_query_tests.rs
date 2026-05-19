//! Tests for `tool_handlers_query.rs`.
//!
//! Sidecar file per the codebase convention (matches `staleness_tests.rs`,
//! `inserter_tests.rs`, etc.). Wired in via
//! `#[cfg(test)] #[path = "tool_handlers_query_tests.rs"] mod tests;`.

use super::{parse_detail_level, parse_fields_mode};
use crate::application::query::search::FieldsMode;
use crate::application::query::DetailLevel;

#[test]
fn fields_mode_defaults_to_full_when_absent() {
    assert_eq!(parse_fields_mode(None).unwrap(), FieldsMode::Full);
}

#[test]
fn fields_mode_accepts_known_values() {
    assert_eq!(parse_fields_mode(Some("full")).unwrap(), FieldsMode::Full);
    assert_eq!(
        parse_fields_mode(Some("minimal")).unwrap(),
        FieldsMode::Minimal,
    );
}

#[test]
fn fields_mode_rejects_typos_with_helpful_message() {
    let err = parse_fields_mode(Some("minimall")).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("minimall") && msg.contains("'full'") && msg.contains("'minimal'"),
        "error should name the bad value and list the valid options, got {msg}",
    );
}

#[test]
fn detail_level_defaults_to_standard_when_absent() {
    assert_eq!(parse_detail_level(None).unwrap(), DetailLevel::Standard);
}

#[test]
fn detail_level_accepts_known_values() {
    assert_eq!(
        parse_detail_level(Some("minimal")).unwrap(),
        DetailLevel::Minimal,
    );
    assert_eq!(
        parse_detail_level(Some("standard")).unwrap(),
        DetailLevel::Standard,
    );
    assert_eq!(parse_detail_level(Some("tree")).unwrap(), DetailLevel::Tree);
}

#[test]
fn detail_level_rejects_typos_with_helpful_message() {
    let err = parse_detail_level(Some("treeee")).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("treeee") && msg.contains("'minimal'") && msg.contains("'standard'"),
        "error should name the bad value and list the valid options, got {msg}",
    );
}
