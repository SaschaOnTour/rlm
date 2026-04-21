//! Tests for `json_semantic.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "json_semantic_tests.rs"] mod tests;`.

use super::{json_type_name, JsonSemanticParser, TextParser};
fn parser() -> JsonSemanticParser {
    JsonSemanticParser::new()
}

#[test]
fn parse_simple_json() {
    let source = r#"{
  "name": "my-project",
  "version": "1.0.0",
  "description": "A test project"
}"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty());
    assert!(chunks.iter().any(|c| c.ident == "name"));
    assert!(chunks.iter().any(|c| c.ident == "version"));
}

#[test]
fn parse_package_json() {
    let source = r#"{
  "name": "my-package",
  "version": "1.0.0",
  "scripts": {
    "build": "tsc",
    "test": "jest"
  },
  "dependencies": {
    "express": "^4.18.0"
  },
  "devDependencies": {
    "typescript": "^5.0.0"
  }
}"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "scripts"));
    assert!(chunks.iter().any(|c| c.ident == "dependencies"));
    assert!(chunks.iter().any(|c| c.ident == "devDependencies"));

    // Verify scripts is categorized correctly
    let scripts = chunks.iter().find(|c| c.ident == "scripts");
    assert!(scripts.is_some());
}

#[test]
fn parse_tsconfig_json() {
    let source = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "strict": true,
    "outDir": "./dist"
  },
  "include": ["src/**/*"],
  "exclude": ["node_modules"]
}"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "compilerOptions"));
    assert!(chunks.iter().any(|c| c.ident == "include"));
}

#[test]
fn parse_nested_json() {
    let source = r#"{
  "config": {
    "database": {
      "host": "localhost",
      "port": 5432
    }
  }
}"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "config"));
    assert!(chunks.iter().any(|c| c.ident == "config.database"));
}

#[test]
fn parse_invalid_json_fallback() {
    let source = "{invalid json}";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].ident, "_root");
}

#[test]
fn empty_json() {
    let chunks = parser().parse_chunks("{}", 1).unwrap();
    // Empty object should still produce a fallback chunk
    assert_eq!(chunks.len(), 1);
}

#[test]
fn parse_array_json() {
    let source = r#"{
  "items": [
    {"name": "item1"},
    {"name": "item2"}
  ]
}"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "items"));
}

#[test]
fn test_json_type_name() {
    assert_eq!(json_type_name(&serde_json::Value::Null), "null");
    assert_eq!(json_type_name(&serde_json::Value::Bool(true)), "bool");
    assert_eq!(json_type_name(&serde_json::Value::Bool(false)), "bool");
    assert_eq!(
        json_type_name(&serde_json::Value::Number(serde_json::Number::from(42))),
        "number"
    );
    assert_eq!(
        json_type_name(&serde_json::Value::String("hello".into())),
        "string"
    );
    assert_eq!(json_type_name(&serde_json::Value::Array(vec![])), "array");
    assert_eq!(
        json_type_name(&serde_json::Value::Object(serde_json::Map::new())),
        "object"
    );
}
