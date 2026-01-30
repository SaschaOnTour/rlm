use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::{Result, RlmError};
use crate::ingest::code::CodeParser;
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};

const CHUNK_QUERY_SRC: &str = r"
    (function_item name: (identifier) @fn_name) @fn_def
    (struct_item name: (type_identifier) @struct_name) @struct_def
    (enum_item name: (type_identifier) @enum_name) @enum_def
    (trait_item name: (type_identifier) @trait_name) @trait_def
    (impl_item type: (type_identifier) @impl_name) @impl_def
    (const_item name: (identifier) @const_name) @const_def
    (static_item name: (identifier) @static_name) @static_def
    (mod_item name: (identifier) @mod_name) @mod_def
    (use_declaration) @use_decl
    (macro_definition name: (identifier) @macro_name) @macro_def
    (type_item name: (type_identifier) @type_alias_name) @type_alias_def
";

const REF_QUERY_SRC: &str = r"
    (call_expression function: (identifier) @call_name)
    (call_expression function: (scoped_identifier name: (identifier) @scoped_call))
    (call_expression function: (field_expression field: (field_identifier) @method_call))
    (use_declaration argument: (scoped_identifier name: (identifier) @use_name))
    (use_declaration argument: (scoped_identifier) @use_path)
    (use_declaration argument: (use_as_clause path: (scoped_identifier) @use_as_path))
    (use_declaration argument: (use_list (identifier) @use_list_item))
    (use_declaration argument: (use_list (scoped_identifier name: (identifier) @use_list_scoped)))
    (use_declaration argument: (scoped_use_list path: (scoped_identifier) @use_group_path))
    (use_declaration argument: (identifier) @use_simple)
    (type_identifier) @type_ref
";

/// Tree-sitter Rust parser.
pub struct RustParser {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl Default for RustParser {
    fn default() -> Self {
        Self::new()
    }
}

impl RustParser {
    #[must_use]
    pub fn new() -> Self {
        let language: Language = tree_sitter_rust::LANGUAGE.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("Rust chunk query must compile");
        let ref_query = Query::new(&language, REF_QUERY_SRC).expect("Rust ref query must compile");
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
                detail: format!("failed to set Rust language: {e}"),
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

        // Collect use declarations for an imports chunk
        let mut use_decls: Vec<tree_sitter::Node> = Vec::new();

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = ChunkKind::Other("unknown".into());
            let mut node = tree.root_node();
            let mut is_use_decl = false;

            for cap in m.captures {
                let cap_name = &self.chunk_query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("");

                match *cap_name {
                    "fn_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Function;
                    }
                    "struct_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Struct;
                    }
                    "enum_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Enum;
                    }
                    "trait_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Trait;
                    }
                    "impl_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Impl;
                    }
                    "const_name" | "static_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Constant;
                    }
                    "mod_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Module;
                    }
                    "macro_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Other("macro".into());
                    }
                    "type_alias_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Other("type_alias".into());
                    }
                    "fn_def" | "struct_def" | "enum_def" | "trait_def" | "impl_def"
                    | "const_def" | "static_def" | "mod_def" | "macro_def" | "type_alias_def" => {
                        node = cap.node;
                    }
                    "use_decl" => {
                        is_use_decl = true;
                        use_decls.push(cap.node);
                    }
                    _ => {}
                }
            }

            // Skip use declarations - we'll create a single imports chunk
            if is_use_decl {
                continue;
            }

            if name.is_empty() {
                continue;
            }

            let start = node.start_position();
            let end = node.end_position();
            let content = node.utf8_text(source_bytes).unwrap_or("").to_string();

            let visibility = extract_visibility(&content);
            let signature = match kind {
                ChunkKind::Function => extract_fn_signature(&content),
                ChunkKind::Struct | ChunkKind::Enum | ChunkKind::Trait => {
                    extract_type_signature(&content)
                }
                _ => None,
            };
            let parent = find_parent_impl(node, source_bytes);

            if kind == ChunkKind::Impl {
                let methods = extract_impl_methods(node, source_bytes, file_id, &name);
                chunks.extend(methods);
            }

            // Skip functions inside impl blocks - they're captured as methods by extract_impl_methods
            if kind == ChunkKind::Function && parent.is_some() {
                continue;
            }

            let doc_comment = collect_rust_doc_comment(node, source_bytes);
            let attributes = collect_rust_attributes(node, source_bytes);

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line: start.row as u32 + 1,
                end_line: end.row as u32 + 1,
                start_byte: node.start_byte() as u32,
                end_byte: node.end_byte() as u32,
                kind,
                ident: name,
                parent,
                signature,
                visibility,
                ui_ctx: None,
                doc_comment,
                attributes,
                content,
            });
        }

        // Create an imports chunk if there are use declarations
        if !use_decls.is_empty() {
            // Find the range that covers all use declarations
            let start_line = use_decls
                .iter()
                .map(|n| n.start_position().row)
                .min()
                .unwrap_or(0);
            let end_line = use_decls
                .iter()
                .map(|n| n.end_position().row)
                .max()
                .unwrap_or(0);
            let start_byte = use_decls
                .iter()
                .map(tree_sitter::Node::start_byte)
                .min()
                .unwrap_or(0);
            let end_byte = use_decls
                .iter()
                .map(tree_sitter::Node::end_byte)
                .max()
                .unwrap_or(0);

            // Collect all use declaration content
            let content: String = use_decls
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
                    "call_name" | "scoped_call" => RefKind::Call,
                    "method_call" => RefKind::Call,
                    "use_name" | "use_path" | "use_as_path" | "use_list_item"
                    | "use_list_scoped" | "use_group_path" | "use_simple" => RefKind::Import,
                    "type_ref" => RefKind::TypeUse,
                    _ => continue,
                };

                let line = pos.row as u32 + 1;
                let chunk_id = chunks
                    .iter()
                    .find(|c| line >= c.start_line && line <= c.end_line)
                    .map_or(0, |c| c.id);

                refs.push(Reference {
                    id: 0,
                    chunk_id,
                    target_ident: text,
                    ref_kind,
                    line,
                    col: pos.column as u32,
                });
            }
        }

        refs
    }
}

impl CodeParser for RustParser {
    fn language(&self) -> &'static str {
        "rust"
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

fn extract_visibility(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("pub(crate)") {
        Some("pub(crate)".into())
    } else if trimmed.starts_with("pub(super)") {
        Some("pub(super)".into())
    } else if trimmed.starts_with("pub") {
        Some("pub".into())
    } else {
        Some("private".into())
    }
}

fn extract_fn_signature(content: &str) -> Option<String> {
    if let Some(brace_pos) = content.find('{') {
        let sig = content[..brace_pos].trim();
        Some(sig.to_string())
    } else {
        content
            .find(';')
            .map(|semi_pos| content[..semi_pos].trim().to_string())
    }
}

/// Extract signature for struct/enum/trait (the first line before the opening brace).
fn extract_type_signature(content: &str) -> Option<String> {
    // For structs/enums/traits, get the line up to the opening brace or first newline
    if let Some(brace_pos) = content.find('{') {
        let sig = content[..brace_pos].trim();
        // Remove trailing where clauses if too long
        let sig = if let Some(where_pos) = sig.find("\nwhere") {
            sig[..where_pos].trim()
        } else {
            sig
        };
        Some(sig.to_string())
    } else if let Some(semi_pos) = content.find(';') {
        // Unit struct: `pub struct Foo;`
        Some(content[..=semi_pos].trim().to_string())
    } else {
        // Fallback: first line
        content.lines().next().map(|s| s.trim().to_string())
    }
}

fn find_parent_impl(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "impl_item" {
            for i in 0..parent.child_count() {
                if let Some(child) = parent.child(i as u32) {
                    if child.kind() == "type_identifier" {
                        return child
                            .utf8_text(source)
                            .ok()
                            .map(std::string::ToString::to_string);
                    }
                }
            }
        }
        current = parent.parent();
    }
    None
}

fn extract_impl_methods(
    impl_node: tree_sitter::Node,
    source: &[u8],
    file_id: i64,
    impl_name: &str,
) -> Vec<Chunk> {
    let mut methods = Vec::new();

    for i in 0..impl_node.child_count() {
        let child = match impl_node.child(i as u32) {
            Some(c) => c,
            None => continue,
        };

        if child.kind() == "declaration_list" {
            for j in 0..child.child_count() {
                let item = match child.child(j as u32) {
                    Some(c) => c,
                    None => continue,
                };

                if item.kind() == "function_item" {
                    let mut fn_name = String::new();
                    for k in 0..item.child_count() {
                        if let Some(name_node) = item.child(k as u32) {
                            if name_node.kind() == "identifier" {
                                fn_name = name_node.utf8_text(source).unwrap_or("").to_string();
                                break;
                            }
                        }
                    }

                    if fn_name.is_empty() {
                        continue;
                    }

                    let content = item.utf8_text(source).unwrap_or("").to_string();
                    let start = item.start_position();
                    let end = item.end_position();

                    let doc_comment = collect_rust_doc_comment(item, source);
                    let attributes = collect_rust_attributes(item, source);

                    methods.push(Chunk {
                        id: 0,
                        file_id,
                        start_line: start.row as u32 + 1,
                        end_line: end.row as u32 + 1,
                        start_byte: item.start_byte() as u32,
                        end_byte: item.end_byte() as u32,
                        kind: ChunkKind::Method,
                        ident: fn_name,
                        parent: Some(impl_name.to_string()),
                        signature: extract_fn_signature(&content),
                        visibility: extract_visibility(&content),
                        ui_ctx: None,
                        doc_comment,
                        attributes,
                        content,
                    });
                }
            }
        }
    }

    methods
}

fn collect_rust_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        if sib.kind() == "attribute_item" {
            current = sib.prev_sibling();
            continue;
        }
        if sib.kind() == "line_comment" {
            let text = sib.utf8_text(source).unwrap_or("");
            if text.starts_with("///") || text.starts_with("//!") {
                lines.push(text.to_string());
                current = sib.prev_sibling();
                continue;
            }
        }
        break;
    }
    lines.reverse();
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn collect_rust_attributes(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut attrs = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        if sib.kind() == "attribute_item" {
            attrs.push(sib.utf8_text(source).unwrap_or("").to_string());
            current = sib.prev_sibling();
            continue;
        }
        if sib.kind() == "line_comment" {
            let text = sib.utf8_text(source).unwrap_or("");
            if text.starts_with("///") || text.starts_with("//!") {
                current = sib.prev_sibling();
                continue;
            }
        }
        break;
    }
    attrs.reverse();
    if attrs.is_empty() {
        None
    } else {
        Some(attrs.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> RustParser {
        RustParser::new()
    }

    #[test]
    fn parse_function() {
        let source = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {}", name)
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(!chunks.is_empty());
        let f = chunks.iter().find(|c| c.ident == "hello").unwrap();
        assert_eq!(f.kind, ChunkKind::Function);
        assert_eq!(f.visibility.as_deref(), Some("pub"));
        assert!(f.signature.as_ref().unwrap().contains("pub fn hello"));
    }

    #[test]
    fn parse_struct() {
        let source = r#"
pub struct Config {
    pub root: PathBuf,
    pub db_path: PathBuf,
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let s = chunks.iter().find(|c| c.ident == "Config").unwrap();
        assert_eq!(s.kind, ChunkKind::Struct);
    }

    #[test]
    fn parse_impl_with_methods() {
        let source = r#"
impl Config {
    pub fn new() -> Self {
        Self { root: PathBuf::new(), db_path: PathBuf::new() }
    }

    fn private_method(&self) -> bool {
        true
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks
            .iter()
            .any(|c| c.ident == "Config" && c.kind == ChunkKind::Impl));
        let new_method = chunks
            .iter()
            .find(|c| c.ident == "new" && c.kind == ChunkKind::Method);
        assert!(new_method.is_some());
        let m = new_method.unwrap();
        assert_eq!(m.parent.as_deref(), Some("Config"));
        assert_eq!(m.visibility.as_deref(), Some("pub"));
    }

    #[test]
    fn parse_enum() {
        let source = r#"
pub enum Color {
    Red,
    Green,
    Blue,
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let e = chunks.iter().find(|c| c.ident == "Color").unwrap();
        assert_eq!(e.kind, ChunkKind::Enum);
    }

    #[test]
    fn parse_trait() {
        let source = r#"
pub trait Parser {
    fn parse(&self, input: &str) -> Result<()>;
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let t = chunks.iter().find(|c| c.ident == "Parser").unwrap();
        assert_eq!(t.kind, ChunkKind::Trait);
    }

    #[test]
    fn extract_refs_finds_calls() {
        let source = r#"
fn main() {
    let x = hello("world");
    println!("{}", x);
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let refs = parser().extract_refs(source, &chunks).unwrap();
        assert!(refs
            .iter()
            .any(|r| r.target_ident == "hello" && r.ref_kind == RefKind::Call));
    }

    #[test]
    fn validate_syntax_valid() {
        assert!(parser().validate_syntax("fn main() {}"));
    }

    #[test]
    fn validate_syntax_invalid() {
        assert!(!parser().validate_syntax("fn main() {"));
    }

    // Regression tests for parser fixes

    #[test]
    fn extract_imports_all_patterns() {
        let source = r#"
use std::collections::HashMap;
use crate::error::Result;
use super::helper;
use foo::{bar, baz};
use some::path as alias;
use simple;
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        // Verify _imports chunk exists
        let imports_chunk = chunks.iter().find(|c| c.ident == "_imports");
        assert!(imports_chunk.is_some(), "Should have an _imports chunk");

        // Verify refs extraction captures imports
        let refs = parser().extract_refs(source, &chunks).unwrap();
        let import_refs: Vec<_> = refs
            .iter()
            .filter(|r| r.ref_kind == RefKind::Import)
            .collect();

        // Should capture multiple import targets
        assert!(
            import_refs.len() >= 5,
            "Should capture at least 5 import refs, got {}",
            import_refs.len()
        );

        // Check specific imports are captured
        let import_names: Vec<&str> = import_refs
            .iter()
            .map(|r| r.target_ident.as_str())
            .collect();
        assert!(
            import_names
                .iter()
                .any(|n| n.contains("HashMap") || n.contains("collections")),
            "Should capture HashMap or collections"
        );
    }

    #[test]
    fn no_duplicate_methods_in_impl() {
        let source = r#"
impl Config {
    pub fn new() -> Self { Self {} }
    fn helper(&self) -> bool { true }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        // Count chunks named "new"
        let new_chunks: Vec<_> = chunks.iter().filter(|c| c.ident == "new").collect();
        assert_eq!(
            new_chunks.len(),
            1,
            "Should have exactly 1 'new' chunk, not duplicates, got {}",
            new_chunks.len()
        );
        assert_eq!(new_chunks[0].kind, ChunkKind::Method);

        // Count chunks named "helper"
        let helper_chunks: Vec<_> = chunks.iter().filter(|c| c.ident == "helper").collect();
        assert_eq!(
            helper_chunks.len(),
            1,
            "Should have exactly 1 'helper' chunk, got {}",
            helper_chunks.len()
        );
    }

    #[test]
    fn struct_enum_trait_have_signatures() {
        let source = r#"
pub struct Config {
    pub name: String,
}

pub enum Status {
    Active,
    Inactive,
}

pub trait Handler {
    fn handle(&self);
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let config = chunks.iter().find(|c| c.ident == "Config").unwrap();
        assert!(config.signature.is_some(), "Config should have a signature");
        assert!(
            config
                .signature
                .as_ref()
                .unwrap()
                .contains("pub struct Config"),
            "Config signature should contain 'pub struct Config'"
        );

        let status = chunks.iter().find(|c| c.ident == "Status").unwrap();
        assert!(status.signature.is_some(), "Status should have a signature");
        assert!(
            status
                .signature
                .as_ref()
                .unwrap()
                .contains("pub enum Status"),
            "Status signature should contain 'pub enum Status'"
        );

        let handler = chunks.iter().find(|c| c.ident == "Handler").unwrap();
        assert!(
            handler.signature.is_some(),
            "Handler should have a signature"
        );
        assert!(
            handler
                .signature
                .as_ref()
                .unwrap()
                .contains("pub trait Handler"),
            "Handler signature should contain 'pub trait Handler'"
        );
    }

    // ============================================================
    // PHASE 2: Critical Reliability Tests
    // ============================================================

    /// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
    /// This is essential for surgical editing operations.
    #[test]
    fn byte_offset_round_trip() {
        let source = r#"
pub fn hello(name: &str) -> String {
    format!("Hello, {}", name)
}

pub struct Config {
    pub root: PathBuf,
}

impl Config {
    pub fn new() -> Self {
        Self { root: PathBuf::new() }
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(!chunks.is_empty(), "Should have extracted chunks");

        for chunk in &chunks {
            // Skip imports chunk as it may join multiple nodes
            if chunk.ident == "_imports" {
                continue;
            }
            let reconstructed = &source[chunk.start_byte as usize..chunk.end_byte as usize];
            assert_eq!(
                reconstructed,
                chunk.content,
                "Byte offset reconstruction failed for chunk '{}' (kind: {:?})\n\
                 Expected bytes {}..{} to equal content:\n---\n{}\n---\n\
                 But got:\n---\n{}\n---",
                chunk.ident,
                chunk.kind,
                chunk.start_byte,
                chunk.end_byte,
                chunk.content,
                reconstructed
            );
        }
    }

    /// Unicode identifiers must be handled correctly.
    #[test]
    fn unicode_identifiers() {
        let source = r#"
fn größe() -> usize {
    42
}

fn 日本語() {
    println!("こんにちは");
}

struct Größe {
    wert: u32,
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let groesse = chunks.iter().find(|c| c.ident == "größe");
        assert!(groesse.is_some(), "Should find function with German umlaut");

        let japanese = chunks.iter().find(|c| c.ident == "日本語");
        assert!(
            japanese.is_some(),
            "Should find function with Japanese name"
        );

        let struct_groesse = chunks.iter().find(|c| c.ident == "Größe");
        assert!(
            struct_groesse.is_some(),
            "Should find struct with German umlaut"
        );
    }

    /// Unicode byte offsets must be correct (not just char indices).
    #[test]
    fn unicode_byte_offset_accuracy() {
        let source = "fn größe() { let x = 1; }\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let f = chunks.iter().find(|c| c.ident == "größe").unwrap();
        let reconstructed = &source[f.start_byte as usize..f.end_byte as usize];
        assert_eq!(
            reconstructed, f.content,
            "Unicode byte offsets must be exact"
        );
    }

    /// CRLF line endings must not break line number calculation.
    #[test]
    fn crlf_line_endings() {
        let source = "fn foo() {\r\n    bar()\r\n}\r\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let f = chunks.iter().find(|c| c.ident == "foo").unwrap();
        assert_eq!(f.start_line, 1, "Start line should be 1");
        assert_eq!(
            f.end_line, 3,
            "End line should be 3 (CRLF counted correctly)"
        );
    }

    /// Mixed line endings (Unix, Windows, old Mac).
    #[test]
    fn mixed_line_endings() {
        let source = "fn one() {}\nfn two() {}\r\nfn three() {}\rfn four() {}";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        assert!(chunks.iter().any(|c| c.ident == "one"), "Should find 'one'");
        assert!(chunks.iter().any(|c| c.ident == "two"), "Should find 'two'");
        assert!(
            chunks.iter().any(|c| c.ident == "three"),
            "Should find 'three'"
        );
        // Note: \r alone may or may not be handled depending on tree-sitter
    }

    /// Reference positions must be within their containing chunk's line range.
    #[test]
    fn reference_positions_within_chunks() {
        let source = r#"
fn caller() {
    let x = helper();
    let y = other_fn();
}

fn helper() -> i32 { 42 }
fn other_fn() -> i32 { 0 }
"#;
        let (chunks, refs) = parser().parse_chunks_and_refs(source, 1).unwrap();

        // All refs with a non-zero chunk_id should be within that chunk's range
        for r in &refs {
            if r.chunk_id != 0 {
                let chunk = chunks.iter().find(|c| c.id == r.chunk_id);
                if let Some(c) = chunk {
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

    /// Async functions.
    #[test]
    fn async_function() {
        let source = r#"
async fn fetch_data(url: &str) -> Result<String, Error> {
    let response = client.get(url).await?;
    Ok(response.text().await?)
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "fetch_data").unwrap();
        assert_eq!(f.kind, ChunkKind::Function);
        assert!(
            f.signature.as_ref().unwrap().contains("async fn"),
            "Signature should include 'async'"
        );
    }

    /// Generics with trait bounds.
    #[test]
    fn generics_with_bounds() {
        let source = r#"
fn process<T: Clone + Debug, U: Default>(items: Vec<T>, default: U) -> T
where
    T: Send + Sync,
{
    items.first().cloned().unwrap_or_else(|| panic!())
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "process").unwrap();
        assert_eq!(f.kind, ChunkKind::Function);
        assert!(
            f.signature.as_ref().unwrap().contains("<T:"),
            "Signature should include generic bounds"
        );
    }

    /// Const generics.
    #[test]
    fn const_generics() {
        let source = r#"
fn fixed_array<const N: usize>() -> [u8; N] {
    [0u8; N]
}

struct Buffer<const SIZE: usize> {
    data: [u8; SIZE],
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let f = chunks.iter().find(|c| c.ident == "fixed_array").unwrap();
        assert_eq!(f.kind, ChunkKind::Function);

        let s = chunks.iter().find(|c| c.ident == "Buffer").unwrap();
        assert_eq!(s.kind, ChunkKind::Struct);
    }

    /// Closures and their captures.
    #[test]
    fn closures_in_function() {
        let source = r#"
fn with_closure() {
    let captured = 42;
    let closure = |x: i32| x + captured;
    let moved = move || captured;
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "with_closure").unwrap();
        assert_eq!(f.kind, ChunkKind::Function);
        // Closures are inside the function, not separate chunks
        assert!(f.content.contains("closure"));
    }

    /// Macro definitions.
    #[test]
    fn macro_rules_definition() {
        let source = r#"
macro_rules! my_vec {
    ($($x:expr),*) => {
        {
            let mut v = Vec::new();
            $(v.push($x);)*
            v
        }
    };
}
"#;
        // macro_rules! is not captured by current query, but should not cause errors
        let chunks = parser().parse_chunks(source, 1).unwrap();
        // Macros are currently not extracted as chunks
        let macro_chunk = chunks.iter().find(|c| c.ident == "my_vec");
        assert!(
            macro_chunk.is_some(),
            "macro_rules should be captured as a chunk"
        );
        assert_eq!(macro_chunk.unwrap().kind, ChunkKind::Other("macro".into()));
    }

    /// Derive macros on structs.
    #[test]
    fn derive_macros() {
        let source = r#"
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Config {
    pub name: String,
    pub value: i32,
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let s = chunks.iter().find(|c| c.ident == "Config").unwrap();
        assert_eq!(s.kind, ChunkKind::Struct);
        // Struct must be found; attribute may or may not be part of the node
        // depending on tree-sitter node boundaries
        assert!(s.content.contains("pub struct Config"));
        assert!(
            s.attributes.is_some(),
            "Should capture #[derive(...)] attribute"
        );
        assert!(s.attributes.as_ref().unwrap().contains("derive"));
    }

    /// Impl blocks for traits.
    #[test]
    fn impl_trait_for_type() {
        let source = r#"
impl Display for Config {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{}", self.name)
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        // Should have impl block and method
        let impl_chunk = chunks.iter().find(|c| c.kind == ChunkKind::Impl);
        assert!(impl_chunk.is_some(), "Should have impl chunk");

        let fmt_method = chunks.iter().find(|c| c.ident == "fmt");
        assert!(fmt_method.is_some(), "Should have fmt method");
        assert_eq!(fmt_method.unwrap().kind, ChunkKind::Method);
    }

    // ============================================================
    // PHASE 3b: Latest Language Features (Rust 1.85+)
    // ============================================================

    /// C-string literals (Rust 1.77+).
    #[test]
    fn rust_c_string_literals() {
        let source = r#"
fn with_c_strings() {
    let s = c"hello world";
    let raw = cr"raw\path";
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "with_c_strings");
        assert!(f.is_some(), "Should find function with c-string literals");
    }

    /// Let chains in if-let (Rust 1.88+).
    #[test]
    fn rust_let_chains() {
        let source = r#"
fn with_let_chains(opt: Option<i32>) -> bool {
    if let Some(x) = opt && x > 0 && x < 100 {
        true
    } else {
        false
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "with_let_chains");
        assert!(f.is_some(), "Should find function with let chains");
    }

    /// Type alias impl Trait bounds.
    #[test]
    fn rust_type_alias_impl_trait() {
        let source = r#"
type Callback = impl Fn(i32) -> i32;

fn create_callback() -> Callback {
    |x| x * 2
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        // Type alias may or may not be captured, but function should be
        let f = chunks.iter().find(|c| c.ident == "create_callback");
        assert!(
            f.is_some(),
            "Should find function returning impl Trait alias"
        );
    }

    /// Associated type bounds.
    #[test]
    fn rust_associated_type_bounds() {
        let source = r#"
trait Container {
    type Item;
}

fn process<C: Container<Item = i32>>(c: C) {
    // process container with i32 items
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "process");
        assert!(
            f.is_some(),
            "Should find function with associated type bounds"
        );
    }

    /// RPITIT (Return Position Impl Trait in Trait).
    #[test]
    fn rust_rpitit() {
        let source = r#"
trait Factory {
    fn create(&self) -> impl Clone;
}

struct MyFactory;

impl Factory for MyFactory {
    fn create(&self) -> impl Clone {
        42
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let factory_trait = chunks.iter().find(|c| c.ident == "Factory");
        assert!(factory_trait.is_some(), "Should find trait with RPITIT");
    }

    // ============================================================
    // PHASE 4: Fallback Mechanism Tests
    // ============================================================

    /// Parse with quality: clean code should be Complete.
    #[test]
    fn parse_with_quality_clean() {
        use crate::ingest::code::CodeParser;

        let source = r#"
fn valid() -> i32 {
    42
}
"#;
        let result = parser().parse_with_quality(source, 1).unwrap();
        assert!(
            result.quality.is_complete(),
            "Clean code should have Complete quality"
        );
        assert!(
            !result.quality.fallback_recommended(),
            "Clean code should not recommend fallback"
        );
    }

    /// Parse with quality: syntax errors should be Partial.
    #[test]
    fn parse_with_quality_syntax_error() {
        use crate::ingest::code::CodeParser;

        let source = r#"
fn broken( {
    42
}
"#;
        let result = parser().parse_with_quality(source, 1).unwrap();
        assert!(
            result.quality.fallback_recommended(),
            "Broken code should recommend fallback"
        );
    }

    // ============================================================
    // PHASE 5: Edge Cases
    // ============================================================

    /// Deeply nested structures.
    #[test]
    fn deeply_nested_impl() {
        let source = r#"
mod outer {
    mod inner {
        pub struct Deep {
            value: i32,
        }

        impl Deep {
            pub fn new() -> Self {
                Self { value: 0 }
            }

            fn helper(&self) -> i32 {
                if true {
                    if true {
                        if true {
                            self.value
                        } else { 0 }
                    } else { 0 }
                } else { 0 }
            }
        }
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(!chunks.is_empty(), "Should parse deeply nested code");
    }

    /// Very long function signature.
    #[test]
    fn very_long_signature() {
        let long_params = (0..50)
            .map(|i| format!("param{}: Type{}", i, i))
            .collect::<Vec<_>>()
            .join(", ");
        let source = format!(
            "fn long_function({}) -> Result<(), Error> {{ Ok(()) }}",
            long_params
        );

        let chunks = parser().parse_chunks(&source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "long_function").unwrap();
        assert_eq!(f.kind, ChunkKind::Function);
    }

    /// Empty file.
    #[test]
    fn empty_file() {
        let source = "";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.is_empty(), "Empty file should produce no chunks");
    }

    /// Comment-only file.
    #[test]
    fn comment_only_file() {
        let source = r#"
// This is a comment
/* This is a block comment */
/// Doc comment
//! Inner doc comment
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(
            chunks.is_empty(),
            "Comment-only file should produce no code chunks"
        );
    }

    /// Whitespace-only file.
    #[test]
    fn whitespace_only_file() {
        let source = "   \n\t\n   \r\n   ";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(
            chunks.is_empty(),
            "Whitespace-only file should produce no chunks"
        );
    }

    /// Very long line (should not crash).
    #[test]
    fn very_long_line() {
        let long_string = "x".repeat(10_000);
        let source = format!("const LONG: &str = \"{}\";", long_string);

        let chunks = parser().parse_chunks(&source, 1).unwrap();
        // Should parse without crashing; may or may not extract the const
        assert!(
            chunks.len() <= 1,
            "Should handle very long lines gracefully"
        );
    }

    /// Partial valid code: valid function followed by invalid.
    #[test]
    fn partial_valid_code() {
        let source = r#"
fn valid() -> i32 {
    42
}

fn broken( {
"#;
        // Parser should not crash
        let result = parser().parse_chunks(source, 1);
        assert!(result.is_ok(), "Should not crash on partial valid code");

        let chunks = result.unwrap();
        // May or may not extract the valid function depending on error recovery
        // But should not panic
        let _ = chunks;
    }
}
