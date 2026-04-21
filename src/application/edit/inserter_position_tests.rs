//! `InsertPosition` parse / target / preview tests for `inserter.rs`.
//!
//! Split out of `inserter_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Tests that actually apply
//! edits (happy path, path-traversal rejection) stay in `inserter_tests.rs`;
//! this file covers the `InsertPosition` value type (parsing, target line,
//! preview mapping).

use super::InsertPosition;

#[test]
fn parse_position_top() {
    assert_eq!(
        "top".parse::<InsertPosition>().unwrap(),
        InsertPosition::Top
    );
}

#[test]
fn parse_position_bottom() {
    assert_eq!(
        "bottom".parse::<InsertPosition>().unwrap(),
        InsertPosition::Bottom
    );
}

#[test]
fn parse_position_before_line() {
    assert_eq!(
        "before:5".parse::<InsertPosition>().unwrap(),
        InsertPosition::BeforeLine(5)
    );
}

#[test]
fn parse_position_after_line() {
    assert_eq!(
        "after:10".parse::<InsertPosition>().unwrap(),
        InsertPosition::AfterLine(10)
    );
}

#[test]
fn parse_position_invalid_format() {
    assert!("middle".parse::<InsertPosition>().is_err());
}

#[test]
fn parse_position_invalid_line_number() {
    assert!("before:abc".parse::<InsertPosition>().is_err());
}

#[test]
fn deserialize_position_from_json_string() {
    let pos: InsertPosition = serde_json::from_str("\"before:5\"").unwrap();
    assert_eq!(pos, InsertPosition::BeforeLine(5));
}

#[test]
fn deserialize_position_invalid() {
    assert!(serde_json::from_str::<InsertPosition>("\"invalid\"").is_err());
}

#[test]
fn target_line_top() {
    assert_eq!(InsertPosition::Top.target_line(), Some(1));
}

#[test]
fn target_line_bottom() {
    assert_eq!(InsertPosition::Bottom.target_line(), None);
}

#[test]
fn target_line_before() {
    assert_eq!(InsertPosition::BeforeLine(10).target_line(), Some(10));
}

#[test]
fn target_line_after() {
    assert_eq!(InsertPosition::AfterLine(10).target_line(), Some(11));
}

#[test]
fn parse_before_zero_rejected() {
    assert!("before:0".parse::<InsertPosition>().is_err());
}

#[test]
fn parse_after_zero_rejected() {
    assert!("after:0".parse::<InsertPosition>().is_err());
}

#[test]
fn preview_source_bottom_is_last() {
    assert!(matches!(
        InsertPosition::Bottom.preview_source(),
        crate::application::index::PreviewSource::Last
    ));
}

#[test]
fn preview_source_after_is_line() {
    assert!(matches!(
        InsertPosition::AfterLine(5).preview_source(),
        crate::application::index::PreviewSource::Line(6)
    ));
}
