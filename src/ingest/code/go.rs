use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::{Result, RlmError};
use crate::ingest::code::CodeParser;
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};

const CHUNK_QUERY_SRC: &str = r"
    (function_declaration name: (identifier) @fn_name) @fn_def
    (method_declaration name: (field_identifier) @method_name) @method_def
    (type_declaration (type_spec name: (type_identifier) @type_name)) @type_def
    (import_declaration) @import_decl
";

const REF_QUERY_SRC: &str = r"
    (call_expression function: (identifier) @call_name)
    (call_expression function: (selector_expression field: (field_identifier) @method_call))
    (import_spec path: (interpreted_string_literal) @import_path)
    (import_spec name: (package_identifier) @import_alias)
    (type_identifier) @type_ref
";

pub struct GoParser {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl Default for GoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl GoParser {
    #[must_use]
    pub fn new() -> Self {
        let language: Language = tree_sitter_go::LANGUAGE.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("Go chunk query must compile");
        let ref_query = Query::new(&language, REF_QUERY_SRC).expect("Go ref query must compile");
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
                detail: format!("failed to set Go language: {e}"),
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

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = ChunkKind::Other("unknown".into());
            let mut node = tree.root_node();
            let mut is_import_decl = false;

            for cap in m.captures {
                let cap_name = &self.chunk_query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("");

                match *cap_name {
                    "fn_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Function;
                    }
                    "method_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Method;
                    }
                    "type_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Struct; // Go types (struct, interface)
                    }
                    "fn_def" | "method_def" | "type_def" => {
                        node = cap.node;
                    }
                    "import_decl" => {
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

            let content = node.utf8_text(source_bytes).unwrap_or("").to_string();
            let start = node.start_position();
            let end = node.end_position();

            // Determine visibility from capitalization (Go convention)
            let visibility = if name.chars().next().is_some_and(char::is_uppercase) {
                Some("pub".into())
            } else {
                Some("private".into())
            };

            let signature = match kind {
                ChunkKind::Function | ChunkKind::Method => content
                    .find('{')
                    .map(|pos| content[..pos].trim().to_string()),
                ChunkKind::Struct => extract_go_type_signature(&content),
                _ => None,
            };

            chunks.push(Chunk {
                id: 0,
                file_id,
                start_line: start.row as u32 + 1,
                end_line: end.row as u32 + 1,
                start_byte: node.start_byte() as u32,
                end_byte: node.end_byte() as u32,
                kind,
                ident: name,
                parent: None,
                signature,
                visibility,
                ui_ctx: None,
                doc_comment: collect_go_doc_comment(node, source_bytes),
                attributes: None,
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
                    "import_path" | "import_alias" => RefKind::Import,
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

impl CodeParser for GoParser {
    fn language(&self) -> &'static str {
        "go"
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

/// Extract signature for Go type declarations (struct, interface).
fn extract_go_type_signature(content: &str) -> Option<String> {
    // For Go types, get the line up to the opening brace
    if let Some(brace_pos) = content.find('{') {
        let sig = content[..brace_pos].trim();
        Some(sig.to_string())
    } else {
        // For type aliases without braces
        content.lines().next().map(|s| s.trim().to_string())
    }
}

fn collect_go_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut lines = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        if sib.kind() == "comment" {
            lines.push(sib.utf8_text(source).unwrap_or("").to_string());
            current = sib.prev_sibling();
            continue;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> GoParser {
        GoParser::new()
    }

    #[test]
    fn parse_go_function() {
        let source = r#"
package main

func Hello(name string) string {
    return "Hello, " + name
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "Hello").unwrap();
        assert_eq!(f.kind, ChunkKind::Function);
        assert_eq!(f.visibility.as_deref(), Some("pub"));
    }

    #[test]
    fn parse_go_private_function() {
        let source = r#"
package main

func helper() int {
    return 42
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "helper").unwrap();
        assert_eq!(f.visibility.as_deref(), Some("private"));
    }

    #[test]
    fn validate_syntax_valid_go() {
        assert!(parser().validate_syntax("package main\nfunc main() {}"));
    }

    #[test]
    fn test_go_imports_extracted() {
        let source = r#"
package main

import (
    "fmt"
    "os"
    alias "path/filepath"
)

func main() {
    fmt.Println("hello")
}
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

        assert!(
            import_refs.len() >= 3,
            "Should capture at least 3 import refs (fmt, os, filepath), got {}",
            import_refs.len()
        );
    }

    #[test]
    fn test_go_type_has_signature() {
        let source = r#"
package main

type Config struct {
    Name string
    Port int
}

type Handler interface {
    Handle() error
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
                .contains("type Config struct"),
            "Config signature should contain 'type Config struct', got: {:?}",
            config.signature
        );

        let handler = chunks.iter().find(|c| c.ident == "Handler").unwrap();
        assert!(
            handler.signature.is_some(),
            "Handler should have a signature"
        );
    }

    // ============================================================
    // PHASE 2: Critical Reliability Tests
    // ============================================================

    /// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
    #[test]
    fn byte_offset_round_trip() {
        let source = r#"
package main

func Hello(name string) string {
    return "Hello, " + name
}

type Config struct {
    Name string
    Port int
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
                "Byte offset reconstruction failed for chunk '{}'\n\
                 Expected bytes {}..{} to equal content",
                chunk.ident, chunk.start_byte, chunk.end_byte
            );
        }
    }

    /// Unicode identifiers (Go supports Unicode letters).
    #[test]
    fn unicode_identifiers() {
        let source = r#"
package main

func größe() int {
    return 42
}

type Größe struct {
    Wert int
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let groesse = chunks.iter().find(|c| c.ident == "größe");
        assert!(groesse.is_some(), "Should find function with German umlaut");

        let struct_groesse = chunks.iter().find(|c| c.ident == "Größe");
        assert!(
            struct_groesse.is_some(),
            "Should find struct with German umlaut"
        );
    }

    /// CRLF line endings.
    #[test]
    fn crlf_line_endings() {
        let source = "package main\r\n\r\nfunc foo() {\r\n    bar()\r\n}\r\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let f = chunks.iter().find(|c| c.ident == "foo").unwrap();
        assert_eq!(f.start_line, 3, "Start line should be 3");
        assert_eq!(f.end_line, 5, "End line should account for CRLF");
    }

    /// Reference positions must be within their containing chunk.
    #[test]
    fn reference_positions_within_chunks() {
        let source = r#"
package main

func caller() {
    helper()
    otherFn()
}

func helper() int { return 42 }
func otherFn() int { return 0 }
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

    /// Go generics (1.18+) - may have limited support in tree-sitter-go 0.23.
    #[test]
    fn generics_type_parameters() {
        let source = r#"
package main

func Process[T any](items []T) T {
    return items[0]
}

type Stack[T any] struct {
    items []T
}

func (s *Stack[T]) Push(item T) {
    s.items = append(s.items, item)
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let process = chunks.iter().find(|c| c.ident == "Process");
        assert!(process.is_some(), "Should find generic function Process");

        let stack = chunks.iter().find(|c| c.ident == "Stack");
        assert!(stack.is_some(), "Should find generic struct Stack");
    }

    /// Go type constraints.
    #[test]
    fn type_constraints() {
        let source = r#"
package main

type Number interface {
    int | int64 | float64
}

func Sum[T Number](vals []T) T {
    var sum T
    for _, v := range vals {
        sum += v
    }
    return sum
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let number = chunks.iter().find(|c| c.ident == "Number");
        assert!(number.is_some(), "Should find constraint interface Number");
    }

    /// Goroutines and channels.
    #[test]
    fn goroutines_and_channels() {
        let source = r#"
package main

func worker(jobs <-chan int, results chan<- int) {
    for j := range jobs {
        results <- j * 2
    }
}

func main() {
    jobs := make(chan int, 100)
    results := make(chan int, 100)
    go worker(jobs, results)
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let worker = chunks.iter().find(|c| c.ident == "worker").unwrap();
        assert_eq!(worker.kind, ChunkKind::Function);
        assert!(
            worker.content.contains("<-chan"),
            "Should have channel in content"
        );
    }

    /// Method receivers.
    #[test]
    fn method_receivers() {
        let source = r#"
package main

type Server struct {
    port int
}

func (s *Server) Start() error {
    return nil
}

func (s Server) Port() int {
    return s.port
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let start = chunks.iter().find(|c| c.ident == "Start");
        assert!(start.is_some(), "Should find method Start");
        assert_eq!(start.unwrap().kind, ChunkKind::Method);

        let port = chunks.iter().find(|c| c.ident == "Port");
        assert!(port.is_some(), "Should find method Port");
    }

    // ============================================================
    // PHASE 3b: Latest Language Features (Go 1.23+)
    // ============================================================

    /// Range over integers (Go 1.22+).
    #[test]
    fn go_range_over_integers() {
        let source = r#"
package main

func countUp(n int) {
    for i := range n {
        println(i)
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "countUp");
        assert!(f.is_some(), "Should find function with range over integers");
    }

    /// Generic type aliases (Go 1.24+).
    #[test]
    fn go_generic_type_alias() {
        let source = r#"
package main

type List[T any] = []T

func ProcessList[T any](list List[T]) {
    for _, item := range list {
        println(item)
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "ProcessList");
        assert!(f.is_some(), "Should find function using generic type alias");
    }

    /// Multiple type parameters with constraints.
    #[test]
    fn go_multiple_type_params() {
        let source = r#"
package main

type Ordered interface {
    int | int64 | float64 | string
}

func Max[T Ordered](a, b T) T {
    if a > b {
        return a
    }
    return b
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "Max");
        assert!(f.is_some(), "Should find function with type constraints");
    }

    // ============================================================
    // PHASE 4: Fallback Mechanism Tests
    // ============================================================

    /// Parse with quality: clean code should be Complete.
    #[test]
    fn parse_with_quality_clean() {
        use crate::ingest::code::CodeParser;

        let source = r#"
package main

func valid() int {
    return 42
}
"#;
        let result = parser().parse_with_quality(source, 1).unwrap();
        assert!(
            result.quality.is_complete(),
            "Clean code should have Complete quality"
        );
    }

    /// Parse with quality: syntax errors should recommend fallback.
    #[test]
    fn parse_with_quality_syntax_error() {
        use crate::ingest::code::CodeParser;

        let source = r#"
package main

func broken( {
    return 42
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
/* Block comment */
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(
            chunks.is_empty(),
            "Comment-only file should produce no code chunks"
        );
    }

    /// Package declaration only.
    #[test]
    fn package_only() {
        let source = "package main\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        // May or may not produce chunks, but should not crash
        let _ = chunks;
    }

    /// Partial valid code.
    #[test]
    fn partial_valid_code() {
        let source = r#"
package main

func valid() int {
    return 42
}

func broken( {
"#;
        let result = parser().parse_chunks(source, 1);
        assert!(result.is_ok(), "Should not crash on partial valid code");
    }
}
