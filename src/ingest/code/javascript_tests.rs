//! Parser tests for `javascript.rs`.
//!
//! Moved out of `javascript.rs` in slice 4.9 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "javascript_tests.rs"] mod tests;`.

use super::*;
use crate::ingest::code::CodeParser;
use crate::models::chunk::{ChunkKind, RefKind};

fn parser() -> JavaScriptParser {
    JavaScriptParser::create()
}

#[test]
fn parse_js_function() {
    let source = r#"
function hello(name) {
    return "Hello, " + name;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "hello" && c.kind == ChunkKind::Function));
}

#[test]
fn parse_js_arrow_function() {
    let source = r#"
const greet = (name) => {
    return "Hello, " + name;
};
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "greet" && c.kind == ChunkKind::Function));
}

#[test]
fn parse_js_class() {
    let source = r#"
class UserService {
    constructor(name) {
        this.name = name;
    }

    getName() {
        return this.name;
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "UserService" && c.kind == ChunkKind::Class));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "constructor" && c.kind == ChunkKind::Method));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "getName" && c.kind == ChunkKind::Method));
}

#[test]
fn parse_js_es_imports() {
    let source = r#"
import React from 'react';
import { useState, useEffect } from 'react';
import * as utils from './utils';

function App() {
    return <div>Hello</div>;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let imports_chunk = chunks.iter().find(|c| c.ident == "_imports");
    assert!(imports_chunk.is_some(), "Should have an _imports chunk");

    let refs = parser().extract_refs(source, &chunks).unwrap();
    let import_refs: Vec<_> = refs
        .iter()
        .filter(|r| r.ref_kind == RefKind::Import)
        .collect();
    assert!(!import_refs.is_empty(), "Should have import refs");
}

#[test]
fn parse_js_commonjs_require() {
    let source = r#"
const fs = require('fs');
const path = require('path');

function readFile(filename) {
    return fs.readFileSync(filename, 'utf8');
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let imports_chunk = chunks.iter().find(|c| c.ident == "_imports");
    assert!(
        imports_chunk.is_some(),
        "Should have an _imports chunk for require statements"
    );
}

#[test]
fn parse_js_async_function() {
    let source = r#"
async function fetchData(url) {
    const response = await fetch(url);
    return response.json();
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "fetchData" && c.kind == ChunkKind::Function));
}

#[test]
fn parse_js_generator_function() {
    let source = r#"
function* numberGenerator() {
    yield 1;
    yield 2;
    yield 3;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "numberGenerator" && c.kind == ChunkKind::Function));
}

#[test]
fn parse_js_export_function() {
    let source = r#"
export function helper() {
    return 42;
}

export default function main() {
    return helper();
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    // Note: Visibility extraction depends on tree-sitter node structure
    // The export keyword may or may not be included in the function content
    assert!(
        chunks.iter().any(|c| c.ident == "helper"),
        "Should find exported helper"
    );
    assert!(
        chunks.iter().any(|c| c.ident == "main"),
        "Should find default exported main"
    );
}

#[test]
fn validate_js_syntax() {
    assert!(parser().validate_syntax("function foo() { return 1; }"));
    assert!(!parser().validate_syntax("function foo( { return 1; }"));
}

// ============================================================
// PHASE 2: Critical Reliability Tests
// ============================================================

#[test]
fn byte_offset_round_trip() {
    let source = r#"
function hello(name) {
    return "Hello, " + name;
}

class Greeter {
    greet() {
        return "Hi!";
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty(), "Should have extracted chunks");

    for chunk in &chunks {
        if chunk.ident == "_imports" {
            continue;
        }
        let reconstructed = &source[chunk.start_byte as usize..chunk.end_byte as usize];
        assert_eq!(
            reconstructed, chunk.content,
            "Byte offset reconstruction failed for chunk '{}'",
            chunk.ident
        );
    }
}

#[test]
fn unicode_identifiers() {
    let source = r#"
function größe() {
    return 42;
}

const 名前 = "test";
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let groesse = chunks.iter().find(|c| c.ident == "größe");
    assert!(groesse.is_some(), "Should find function with German umlaut");
}

#[test]
fn crlf_line_endings() {
    let source = "function foo() {\r\n    return 1;\r\n}\r\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let foo = chunks.iter().find(|c| c.ident == "foo").unwrap();
    assert_eq!(foo.start_line, 1, "Start line should be 1");
}

#[test]
fn reference_positions_within_chunks() {
    let source = r#"
class Service {
    process() {
        this.helper();
        this.other();
    }

    helper() {}
    other() {}
}
"#;
    let (chunks, refs) = parser().parse_chunks_and_refs(source, 1).unwrap();

    for r in &refs {
        if r.chunk_id != 0 {
            if let Some(c) = chunks.iter().find(|c| c.id == r.chunk_id) {
                assert!(
                    r.line >= c.start_line && r.line <= c.end_line,
                    "Reference to '{}' at line {} should be within chunk '{}' lines {}-{}",
                    r.target_ident,
                    r.line,
                    c.ident,
                    c.start_line,
                    c.end_line
                );
            }
        }
    }
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
