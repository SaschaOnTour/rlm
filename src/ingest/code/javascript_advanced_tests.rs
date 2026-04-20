//! Advanced parser tests for `javascript.rs` (PHASE 3 onward).
//!
//! Split out of `javascript_tests.rs` to keep each test companion focused
//! on a smaller cluster of behaviors (SRP_MODULE).

use super::JavaScriptParser;
use crate::domain::chunk::ChunkKind;
use crate::ingest::code::CodeParser;

fn parser() -> JavaScriptParser {
    JavaScriptParser::create()
}

// ============================================================
// PHASE 3: Modern Language Features
// ============================================================

#[test]
fn js_destructuring() {
    let source = r#"
function processUser({ name, age }) {
    return `${name} is ${age} years old`;
}

const [first, ...rest] = [1, 2, 3];
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "processUser"));
}

#[test]
fn js_template_literals() {
    let source = r#"
function greeting(name) {
    return `Hello, ${name}!`;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "greeting"));
}

#[test]
fn js_spread_operator() {
    let source = r#"
function merge(...arrays) {
    return [...arrays.flat()];
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "merge"));
}

#[test]
fn js_optional_chaining() {
    let source = r#"
function getCity(user) {
    return user?.address?.city ?? 'Unknown';
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "getCity"));
}

#[test]
fn js_nullish_coalescing() {
    let source = r#"
function getDefault(value) {
    return value ?? 'default';
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "getDefault"));
}

#[test]
fn js_class_static_methods() {
    let source = r#"
class MathUtils {
    static add(a, b) {
        return a + b;
    }

    static multiply(a, b) {
        return a * b;
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "MathUtils" && c.kind == ChunkKind::Class));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "add" && c.kind == ChunkKind::Method));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "multiply" && c.kind == ChunkKind::Method));
}

#[test]
fn js_class_getters_setters() {
    let source = r#"
class User {
    constructor(name) {
        this._name = name;
    }

    get name() {
        return this._name;
    }

    set name(value) {
        this._name = value;
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "User" && c.kind == ChunkKind::Class));
    // Getters and setters are methods
    assert!(chunks
        .iter()
        .any(|c| c.ident == "name" && c.kind == ChunkKind::Method));
}

#[test]
fn js_class_inheritance() {
    let source = r#"
class Animal {
    constructor(name) {
        this.name = name;
    }
}

class Dog extends Animal {
    bark() {
        return 'Woof!';
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Animal" && c.kind == ChunkKind::Class));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Dog" && c.kind == ChunkKind::Class));
}

// ============================================================
// PHASE 4: Fallback Mechanism Tests
// ============================================================

#[test]
fn parse_with_quality_clean() {
    let source = "function valid() { return 42; }";
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(
        result.quality.is_complete(),
        "Clean code should have Complete quality"
    );
}

#[test]
fn parse_with_quality_syntax_error() {
    let source = "function broken( { return 42; }";
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(
        result.quality.fallback_recommended(),
        "Broken code should recommend fallback"
    );
}

// ============================================================
// PHASE 5: Edge Cases
// ============================================================

#[test]
fn empty_file() {
    let chunks = parser().parse_chunks("", 1).unwrap();
    assert!(chunks.is_empty(), "Empty file should produce no chunks");
}

#[test]
fn comment_only_file() {
    let source = r#"
// Single line comment
/* Block comment */
/** JSDoc comment */
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(
        chunks.is_empty(),
        "Comment-only file should produce no code chunks"
    );
}

#[test]
fn partial_valid_code() {
    let source = r#"
function valid() {
    return 42;
}

function broken( {
"#;
    let result = parser().parse_chunks(source, 1);
    assert!(result.is_ok(), "Should not crash on partial valid code");
}
