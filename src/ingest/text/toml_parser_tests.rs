//! Tests for `toml_parser.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "toml_parser_tests.rs"] mod tests;`.

use super::{toml_value_kind_label, TextParser, TomlParser};
fn parser() -> TomlParser {
    TomlParser::new()
}

#[test]
fn parse_simple_toml() {
    let source = r#"
name = "my-project"
version = "1.0.0"
description = "A test project"
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty());
    assert!(chunks.iter().any(|c| c.ident == "name"));
    assert!(chunks.iter().any(|c| c.ident == "version"));
}

#[test]
fn parse_cargo_toml() {
    let source = r#"
[package]
name = "my-crate"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1.0"
tokio = { version = "1", features = ["full"] }

[dev-dependencies]
tempfile = "3"
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "package"));
    assert!(chunks.iter().any(|c| c.ident == "dependencies"));
    assert!(chunks.iter().any(|c| c.ident == "dev-dependencies"));
}

#[test]
fn parse_nested_tables() {
    let source = r#"
[tool.poetry]
name = "my-package"
version = "0.1.0"

[tool.poetry.dependencies]
python = "^3.9"
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "tool"));
}

#[test]
fn parse_array_of_tables() {
    let source = r#"
[[bin]]
name = "app"
path = "src/main.rs"

[[bin]]
name = "cli"
path = "src/cli.rs"
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "bin"));
}

#[test]
fn parse_invalid_toml_fallback() {
    let source = "[invalid toml content";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].ident, "_root");
}

#[test]
fn empty_toml() {
    let chunks = parser().parse_chunks("", 1).unwrap();
    assert_eq!(chunks.len(), 1);
}

#[test]
fn test_toml_json_type_name() {
    assert_eq!(toml_value_kind_label(&serde_json::Value::Null), "null");
    assert_eq!(
        toml_value_kind_label(&serde_json::Value::Bool(true)),
        "bool"
    );
    assert_eq!(
        toml_value_kind_label(&serde_json::Value::Bool(false)),
        "bool"
    );
    assert_eq!(
        toml_value_kind_label(&serde_json::Value::Number(serde_json::Number::from(42))),
        "integer"
    );
    assert_eq!(
        toml_value_kind_label(&serde_json::Value::Number(
            serde_json::Number::from_f64(3.15).unwrap()
        )),
        "float"
    );
    assert_eq!(
        toml_value_kind_label(&serde_json::Value::String("hello".into())),
        "string"
    );
    assert_eq!(
        toml_value_kind_label(&serde_json::Value::Array(vec![])),
        "array"
    );
    assert_eq!(
        toml_value_kind_label(&serde_json::Value::Object(serde_json::Map::new())),
        "table"
    );
}
