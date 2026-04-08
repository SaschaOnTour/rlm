//! JavaScript/JSX parser for rlm.
//!
//! Supports ES6+ features including:
//! - Functions (regular, arrow, async, generator)
//! - Classes (methods, getters/setters, static members)
//! - ES Modules (import/export)
//! - `CommonJS` (require/module.exports)
//! - JSX Components

use tree_sitter::{Language, Query};

use crate::ingest::code::base::{
    build_language_config, collect_prev_siblings, first_child_text_by_kind, BaseParser,
    ChunkCaptureResult, LanguageConfig, SiblingCollectConfig,
};
use crate::models::chunk::{ChunkKind, RefKind};

const CHUNK_QUERY_SRC: &str = r#"
    ; Functions
    (function_declaration name: (identifier) @fn_name) @fn_def
    (generator_function_declaration name: (identifier) @gen_fn_name) @gen_fn_def

    ; Arrow functions assigned to variables
    (lexical_declaration
        (variable_declarator
            name: (identifier) @arrow_name
            value: (arrow_function))) @arrow_def
    (variable_declaration
        (variable_declarator
            name: (identifier) @arrow_name
            value: (arrow_function))) @arrow_def

    ; Classes
    (class_declaration name: (identifier) @class_name) @class_def

    ; Class methods
    (method_definition
        name: (property_identifier) @method_name) @method_def

    ; ES Module imports
    (import_statement) @import_decl

    ; CommonJS require (variable declarations with require)
    (lexical_declaration
        (variable_declarator
            value: (call_expression
                function: (identifier) @_require_fn
                (#eq? @_require_fn "require")))) @require_decl
    (variable_declaration
        (variable_declarator
            value: (call_expression
                function: (identifier) @_require_fn
                (#eq? @_require_fn "require")))) @require_decl
"#;

const REF_QUERY_SRC: &str = r#"
    ; Function calls
    (call_expression
        function: (identifier) @call_name)
    (call_expression
        function: (member_expression
            property: (property_identifier) @method_call))

    ; Import paths
    (import_statement
        source: (string) @import_path)

    ; Require paths
    (call_expression
        function: (identifier) @_require
        arguments: (arguments (string) @require_path)
        (#eq? @_require "require"))

    ; JSX elements
    (jsx_element
        open_tag: (jsx_opening_element
            name: (identifier) @jsx_component))
    (jsx_self_closing_element
        name: (identifier) @jsx_component)
"#;

pub struct JavaScriptConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl JavaScriptConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_javascript::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "JavaScript",
        );
        Self { language, chunk_query, ref_query }
    }
}

impl LanguageConfig for JavaScriptConfig {
    fn language(&self) -> &Language {
        &self.language
    }

    fn chunk_query(&self) -> &Query {
        &self.chunk_query
    }

    fn ref_query(&self) -> &Query {
        &self.ref_query
    }

    fn language_name(&self) -> &'static str {
        "javascript"
    }

    fn import_capture_name(&self) -> &'static str {
        "import_decl"
    }

    fn is_import_capture(&self, capture_name: &str) -> bool {
        capture_name == "import_decl" || capture_name == "require_decl"
    }

    fn needs_deduplication(&self) -> bool {
        true
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "fn_name" | "gen_fn_name" | "arrow_name" => {
                Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Function))
            }
            "class_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Class)),
            "method_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Method)),
            n if n.ends_with("_def") => Some(ChunkCaptureResult::definition()),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "import_path" | "require_path" => Some(RefKind::Import),
            "jsx_component" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn filter_ref_capture(&self, capture_name: &str, text: &str) -> bool {
        if capture_name == "jsx_component" {
            // Only PascalCase names are components
            text.chars().next().is_some_and(char::is_uppercase)
        } else {
            true
        }
    }

    fn transform_ref_text(&self, capture_name: &str, text: &str) -> String {
        match capture_name {
            // Clean up string quotes from import paths
            "import_path" | "require_path" => {
                text.trim_matches('"').trim_matches('\'').to_string()
            }
            _ => text.to_string(),
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        extract_js_visibility(content)
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        extract_js_signature(content, kind)
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_js_parent(node, source)
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_prev_siblings(
            node,
            source,
            &SiblingCollectConfig {
                kinds: &["comment"],
                skip_kinds: &[],
                prefixes: &["/**"],
                multi: false,
            },
        )
    }

    fn collect_attributes(&self, _node: tree_sitter::Node, _source: &[u8]) -> Option<String> {
        None // JS doesn't have attributes like decorators in this basic form
    }
}

/// Public type alias for the JavaScript parser.
pub type JavaScriptParser = BaseParser<JavaScriptConfig>;

impl Default for JavaScriptParser {
    fn default() -> Self {
        Self::new(JavaScriptConfig::new())
    }
}

impl JavaScriptParser {
    /// Create a new JavaScript parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(JavaScriptConfig::new())
    }
}

fn extract_js_visibility(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("export default") {
        Some("export default".into())
    } else if trimmed.starts_with("export") {
        Some("export".into())
    } else {
        None
    }
}

fn extract_js_signature(content: &str, kind: &ChunkKind) -> Option<String> {
    match kind {
        ChunkKind::Function => {
            // Find opening brace or arrow
            if let Some(brace_pos) = content.find('{') {
                Some(content[..brace_pos].trim().to_string())
            } else if let Some(arrow_pos) = content.find("=>") {
                Some(content[..arrow_pos + 2].trim().to_string())
            } else {
                content.lines().next().map(|s| s.trim().to_string())
            }
        }
        ChunkKind::Class => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        ChunkKind::Method => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        _ => None,
    }
}

fn find_js_parent(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "class_body" {
            let class_decl = parent.parent()?;
            if class_decl.kind() == "class_declaration" || class_decl.kind() == "class" {
                return first_child_text_by_kind(class_decl, source, &["identifier"]);
            }
        }
        current = parent.parent();
    }
    None
}

#[cfg(test)]
mod tests {
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
}
