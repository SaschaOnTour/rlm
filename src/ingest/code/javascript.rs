//! JavaScript/JSX parser for rlm-cli.
//!
//! Supports ES6+ features including:
//! - Functions (regular, arrow, async, generator)
//! - Classes (methods, getters/setters, static members)
//! - ES Modules (import/export)
//! - `CommonJS` (require/module.exports)
//! - JSX Components

use std::collections::HashSet;

use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::{Result, RlmError};
use crate::ingest::code::CodeParser;
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};

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

pub struct JavaScriptParser {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl Default for JavaScriptParser {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaScriptParser {
    #[must_use]
    pub fn new() -> Self {
        let language: Language = tree_sitter_javascript::LANGUAGE.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("JavaScript chunk query must compile");
        let ref_query =
            Query::new(&language, REF_QUERY_SRC).expect("JavaScript ref query must compile");
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }

    fn make_parser(&self) -> Result<Parser> {
        let mut parser = Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| RlmError::Parse {
                path: String::new(),
                detail: format!("failed to set JavaScript language: {e}"),
            })?;
        Ok(parser)
    }

    fn extract_chunks_from_tree(
        &self,
        tree: &Tree,
        source_bytes: &[u8],
        file_id: i64,
    ) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.chunk_query, tree.root_node(), source_bytes);

        // Collect import declarations for an imports chunk
        let mut import_decls: Vec<tree_sitter::Node> = Vec::new();
        // Track seen chunks to avoid duplicates
        let mut seen: HashSet<(String, u32)> = HashSet::new();

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = ChunkKind::Other("unknown".into());
            let mut node = tree.root_node();
            let mut is_import_decl = false;

            for cap in m.captures {
                let cap_name = &self.chunk_query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("");

                match *cap_name {
                    "fn_name" | "gen_fn_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Function;
                    }
                    "arrow_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Function;
                    }
                    "class_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Class;
                    }
                    "method_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Method;
                    }
                    n if n.ends_with("_def") => {
                        node = cap.node;
                    }
                    "import_decl" | "require_decl" => {
                        is_import_decl = true;
                        import_decls.push(cap.node);
                    }
                    _ => {}
                }
            }

            // Skip import declarations - we'll create a single imports chunk
            if is_import_decl {
                continue;
            }

            if name.is_empty() {
                continue;
            }

            let start = node.start_position();
            let start_line = start.row as u32 + 1;

            // Skip duplicates
            let key = (name.clone(), start_line);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            let content = node.utf8_text(source_bytes).unwrap_or("").to_string();
            let end = node.end_position();

            let visibility = extract_js_visibility(&content);
            let signature = extract_js_signature(&content, &kind);
            let parent = find_js_parent(node, source_bytes);

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line,
                end_line: end.row as u32 + 1,
                start_byte: node.start_byte() as u32,
                end_byte: node.end_byte() as u32,
                kind,
                ident: name,
                parent,
                signature,
                visibility,
                ui_ctx: None,
                doc_comment: collect_js_doc_comment(node, source_bytes),
                attributes: None, // JS doesn't have attributes like decorators in this basic form
                content,
            });
        }

        // Create an imports chunk if there are import declarations
        if !import_decls.is_empty() {
            let start_line = import_decls
                .iter()
                .map(|n| n.start_position().row)
                .min()
                .unwrap_or(0);
            let end_line = import_decls
                .iter()
                .map(|n| n.end_position().row)
                .max()
                .unwrap_or(0);
            let start_byte = import_decls
                .iter()
                .map(tree_sitter::Node::start_byte)
                .min()
                .unwrap_or(0);
            let end_byte = import_decls
                .iter()
                .map(tree_sitter::Node::end_byte)
                .max()
                .unwrap_or(0);

            let content: String = import_decls
                .iter()
                .filter_map(|n| n.utf8_text(source_bytes).ok())
                .collect::<Vec<_>>()
                .join("\n");

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line: start_line as u32 + 1,
                end_line: end_line as u32 + 1,
                start_byte: start_byte as u32,
                end_byte: end_byte as u32,
                kind: ChunkKind::Other("imports".into()),
                ident: "_imports".to_string(),
                parent: None,
                signature: None,
                visibility: None,
                ui_ctx: None,
                doc_comment: None,
                attributes: None,
                content,
            });
        }

        chunks
    }

    fn extract_refs_from_tree(
        &self,
        tree: &Tree,
        source_bytes: &[u8],
        chunks: &[Chunk],
    ) -> Vec<Reference> {
        let mut refs = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&self.ref_query, tree.root_node(), source_bytes);

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let cap_name = &self.ref_query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("").to_string();
                let pos = cap.node.start_position();

                let ref_kind = match *cap_name {
                    "call_name" | "method_call" => RefKind::Call,
                    "import_path" | "require_path" => RefKind::Import,
                    "jsx_component" => {
                        // Only PascalCase names are components
                        if text.chars().next().is_some_and(char::is_uppercase) {
                            RefKind::TypeUse
                        } else {
                            continue;
                        }
                    }
                    _ => continue,
                };

                // Clean up string quotes from import paths
                let target = text.trim_matches('"').trim_matches('\'').to_string();

                let line = pos.row as u32 + 1;
                let chunk_id = chunks
                    .iter()
                    .find(|c| line >= c.start_line && line <= c.end_line)
                    .map_or(0, |c| c.id);

                refs.push(Reference {
                    id: 0,
                    chunk_id,
                    target_ident: target,
                    ref_kind,
                    line,
                    col: pos.column as u32,
                });
            }
        }

        refs
    }
}

impl CodeParser for JavaScriptParser {
    fn language(&self) -> &'static str {
        "javascript"
    }

    fn parse_chunks(&self, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        let mut parser = self.make_parser()?;
        let tree = parser.parse(source, None).ok_or_else(|| RlmError::Parse {
            path: String::new(),
            detail: "tree-sitter parse returned None".into(),
        })?;
        Ok(self.extract_chunks_from_tree(&tree, source.as_bytes(), file_id))
    }

    fn extract_refs(&self, source: &str, chunks: &[Chunk]) -> Result<Vec<Reference>> {
        let mut parser = self.make_parser()?;
        let tree = parser.parse(source, None).ok_or_else(|| RlmError::Parse {
            path: String::new(),
            detail: "tree-sitter parse returned None".into(),
        })?;
        Ok(self.extract_refs_from_tree(&tree, source.as_bytes(), chunks))
    }

    fn parse_chunks_and_refs(
        &self,
        source: &str,
        file_id: i64,
    ) -> Result<(Vec<Chunk>, Vec<Reference>)> {
        let mut parser = self.make_parser()?;
        let tree = parser.parse(source, None).ok_or_else(|| RlmError::Parse {
            path: String::new(),
            detail: "tree-sitter parse returned None".into(),
        })?;
        let source_bytes = source.as_bytes();
        let chunks = self.extract_chunks_from_tree(&tree, source_bytes, file_id);
        let refs = self.extract_refs_from_tree(&tree, source_bytes, &chunks);
        Ok((chunks, refs))
    }

    fn validate_syntax(&self, source: &str) -> bool {
        let mut parser = match self.make_parser() {
            Ok(p) => p,
            Err(_) => return false,
        };
        match parser.parse(source, None) {
            Some(tree) => !tree.root_node().has_error(),
            None => false,
        }
    }

    fn parse_with_quality(
        &self,
        source: &str,
        file_id: i64,
    ) -> Result<crate::ingest::code::ParseResult> {
        use crate::ingest::code::{find_error_lines, ParseQuality, ParseResult};

        let mut parser = self.make_parser()?;
        let tree = parser.parse(source, None).ok_or_else(|| RlmError::Parse {
            path: String::new(),
            detail: "tree-sitter parse returned None".into(),
        })?;
        let source_bytes = source.as_bytes();
        let chunks = self.extract_chunks_from_tree(&tree, source_bytes, file_id);
        let refs = self.extract_refs_from_tree(&tree, source_bytes, &chunks);

        let quality = if tree.root_node().has_error() {
            let error_lines = find_error_lines(tree.root_node());
            ParseQuality::Partial {
                error_count: error_lines.len(),
                error_lines,
            }
        } else {
            ParseQuality::Complete
        };

        Ok(ParseResult {
            chunks,
            refs,
            quality,
        })
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
        let kind = parent.kind();
        if kind == "class_body" {
            // Go up one more to get class_declaration
            if let Some(class_decl) = parent.parent() {
                if class_decl.kind() == "class_declaration" || class_decl.kind() == "class" {
                    for i in 0..class_decl.child_count() {
                        if let Some(child) = class_decl.child(i as u32) {
                            if child.kind() == "identifier" {
                                return child
                                    .utf8_text(source)
                                    .ok()
                                    .map(std::string::ToString::to_string);
                            }
                        }
                    }
                }
            }
        }
        current = parent.parent();
    }
    None
}

fn collect_js_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    if let Some(sib) = node.prev_sibling() {
        if sib.kind() == "comment" {
            let text = sib.utf8_text(source).unwrap_or("");
            // JSDoc starts with /**
            if text.starts_with("/**") {
                return Some(text.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> JavaScriptParser {
        JavaScriptParser::new()
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
