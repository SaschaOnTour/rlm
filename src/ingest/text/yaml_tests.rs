//! Basic tests for `yaml.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "yaml_tests.rs"] mod tests;`.
//!
//! Nested / real-world schema tests live in the sibling
//! `yaml_nested_tests.rs`.

use super::{TextParser, YamlParser};
use crate::ingest::text::yaml_helpers::yaml_type_name;

fn parser() -> YamlParser {
    YamlParser::new()
}

#[test]
fn parse_simple_yaml() {
    let source = r#"
name: my-project
version: 1.0.0
description: A test project
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty());
    assert!(chunks.iter().any(|c| c.ident == "name"));
    assert!(chunks.iter().any(|c| c.ident == "version"));
}

#[test]
fn parse_invalid_yaml_fallback() {
    let source = "this: is: not: valid: yaml: [";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].ident, "_root");
}

#[test]
fn empty_yaml() {
    let chunks = parser().parse_chunks("", 1).unwrap();
    assert_eq!(chunks.len(), 1);
}

#[test]
fn test_yaml_type_name() {
    use serde_yaml_ng::Value;
    assert_eq!(yaml_type_name(&Value::Null), "null");
    assert_eq!(yaml_type_name(&Value::Bool(true)), "bool");
    assert_eq!(yaml_type_name(&Value::Bool(false)), "bool");
    assert_eq!(
        yaml_type_name(&Value::Number(serde_yaml_ng::Number::from(42))),
        "number"
    );
    assert_eq!(yaml_type_name(&Value::String("hello".into())), "string");
    assert_eq!(yaml_type_name(&Value::Sequence(vec![])), "array");
    assert_eq!(
        yaml_type_name(&Value::Mapping(serde_yaml_ng::Mapping::new())),
        "object"
    );
    // Tagged values
    assert_eq!(
        yaml_type_name(&Value::Tagged(Box::new(
            serde_yaml_ng::value::TaggedValue {
                tag: serde_yaml_ng::value::Tag::new("!custom"),
                value: Value::Null,
            }
        ))),
        "tagged"
    );
}
