use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::{Result, RlmError};
use crate::ingest::code::CodeParser;
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};

const CHUNK_QUERY_SRC: &str = r"
    (class_declaration name: (identifier) @class_name) @class_def
    (interface_declaration name: (identifier) @iface_name) @iface_def
    (enum_declaration name: (identifier) @enum_name) @enum_def
    (method_declaration name: (identifier) @method_name) @method_def
    (constructor_declaration name: (identifier) @ctor_name) @ctor_def
    (import_declaration) @import_decl
";

const REF_QUERY_SRC: &str = r"
    (method_invocation name: (identifier) @call_name)
    (import_declaration (scoped_identifier) @import_path)
    (import_declaration (identifier) @import_simple)
    (type_identifier) @type_ref
";

pub struct JavaParser {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl Default for JavaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaParser {
    #[must_use]
    pub fn new() -> Self {
        let language: Language = tree_sitter_java::LANGUAGE.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("Java chunk query must compile");
        let ref_query = Query::new(&language, REF_QUERY_SRC).expect("Java ref query must compile");
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
                detail: format!("failed to set Java language: {e}"),
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
        // Track seen chunks to avoid duplicates (name + start_line)
        let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();

        while let Some(m) = matches.next() {
            let mut name = String::new();
            let mut kind = ChunkKind::Other("unknown".into());
            let mut node = tree.root_node();
            let mut is_import_decl = false;

            for cap in m.captures {
                let cap_name = &self.chunk_query.capture_names()[cap.index as usize];
                let text = cap.node.utf8_text(source_bytes).unwrap_or("");

                match *cap_name {
                    "class_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Class;
                    }
                    "iface_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Interface;
                    }
                    "enum_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Enum;
                    }
                    "method_name" | "ctor_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Method;
                    }
                    "class_def" | "iface_def" | "enum_def" | "method_def" | "ctor_def" => {
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

            let visibility = extract_java_visibility(&content);
            let signature = match kind {
                ChunkKind::Method => content
                    .find('{')
                    .map(|pos| content[..pos].trim().to_string()),
                ChunkKind::Class | ChunkKind::Interface | ChunkKind::Enum => {
                    extract_java_type_signature(&content)
                }
                _ => None,
            };

            let parent = find_java_parent(node, source_bytes);

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
                doc_comment: collect_java_doc_comment(node, source_bytes),
                attributes: collect_java_annotations(node, source_bytes),
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
                    "call_name" => RefKind::Call,
                    "import_path" | "import_simple" => RefKind::Import,
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

impl CodeParser for JavaParser {
    fn language(&self) -> &'static str {
        "java"
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

fn extract_java_visibility(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("public") {
        Some("public".into())
    } else if trimmed.starts_with("protected") {
        Some("protected".into())
    } else if trimmed.starts_with("private") {
        Some("private".into())
    } else {
        Some("package".into())
    }
}

/// Extract signature for Java type declarations (class, interface, enum).
fn extract_java_type_signature(content: &str) -> Option<String> {
    if let Some(brace_pos) = content.find('{') {
        let sig = content[..brace_pos].trim();
        Some(sig.to_string())
    } else {
        content.lines().next().map(|s| s.trim().to_string())
    }
}

fn find_java_parent(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "class_declaration" || parent.kind() == "interface_declaration" {
            for i in 0..parent.child_count() {
                if let Some(child) = parent.child(i as u32) {
                    if child.kind() == "identifier" {
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

fn collect_java_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Check the previous sibling for javadoc or line comments
    if let Some(sib) = node.prev_sibling() {
        if sib.kind() == "block_comment" {
            let text = sib.utf8_text(source).unwrap_or("");
            if text.starts_with("/**") {
                return Some(text.to_string());
            }
        }
        if sib.kind() == "line_comment" {
            // Collect consecutive line comments
            let mut lines = vec![sib.utf8_text(source).unwrap_or("").to_string()];
            let mut prev = sib.prev_sibling();
            while let Some(p) = prev {
                if p.kind() == "line_comment" {
                    lines.push(p.utf8_text(source).unwrap_or("").to_string());
                    prev = p.prev_sibling();
                } else {
                    break;
                }
            }
            lines.reverse();
            return Some(lines.join("\n"));
        }
    }
    None
}

fn collect_java_annotations(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // In Java, annotations are within the modifiers child of the declaration
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
            if child.kind() == "modifiers" {
                let mut annots = Vec::new();
                for j in 0..child.child_count() {
                    if let Some(mod_child) = child.child(j as u32) {
                        if mod_child.kind() == "marker_annotation"
                            || mod_child.kind() == "annotation"
                        {
                            annots.push(mod_child.utf8_text(source).unwrap_or("").to_string());
                        }
                    }
                }
                if !annots.is_empty() {
                    return Some(annots.join("\n"));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> JavaParser {
        JavaParser::new()
    }

    #[test]
    fn parse_java_class_with_methods() {
        let source = r#"
public class UserService {
    public String getUser(int id) {
        return "user";
    }

    private void helper() {
        System.out.println("help");
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks
            .iter()
            .any(|c| c.ident == "UserService" && c.kind == ChunkKind::Class));
        assert!(chunks
            .iter()
            .any(|c| c.ident == "getUser" && c.kind == ChunkKind::Method));
        let helper = chunks.iter().find(|c| c.ident == "helper").unwrap();
        assert_eq!(helper.visibility.as_deref(), Some("private"));
        assert_eq!(helper.parent.as_deref(), Some("UserService"));
    }

    #[test]
    fn validate_java_syntax() {
        assert!(parser().validate_syntax("public class Foo { public void bar() {} }"));
        assert!(!parser().validate_syntax("public class Foo {"));
    }

    #[test]
    fn test_java_imports_extracted() {
        let source = r#"
import java.util.ArrayList;
import java.util.HashMap;
import static java.lang.Math.PI;

public class Test {
    public void test() {}
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
            import_refs.len() >= 2,
            "Should capture at least 2 import refs, got {}",
            import_refs.len()
        );
    }

    #[test]
    fn test_java_no_duplicate_methods() {
        let source = r#"
public class UserService {
    public String getUser(int id) {
        return "user";
    }

    public void setUser(String name) {
        System.out.println(name);
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        // Count method chunks
        let get_user_chunks: Vec<_> = chunks.iter().filter(|c| c.ident == "getUser").collect();
        assert_eq!(
            get_user_chunks.len(),
            1,
            "Should have exactly 1 'getUser' chunk, got {}",
            get_user_chunks.len()
        );

        let set_user_chunks: Vec<_> = chunks.iter().filter(|c| c.ident == "setUser").collect();
        assert_eq!(
            set_user_chunks.len(),
            1,
            "Should have exactly 1 'setUser' chunk, got {}",
            set_user_chunks.len()
        );
    }

    #[test]
    fn test_java_class_has_signature() {
        let source = r#"
public class UserService extends BaseService implements Handler {
    public void handle() {}
}

public interface Handler {
    void handle();
}

public enum Status {
    ACTIVE,
    INACTIVE
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let user_service = chunks.iter().find(|c| c.ident == "UserService").unwrap();
        assert!(
            user_service.signature.is_some(),
            "UserService should have a signature"
        );
        assert!(
            user_service
                .signature
                .as_ref()
                .unwrap()
                .contains("public class UserService"),
            "UserService signature should contain class declaration, got: {:?}",
            user_service.signature
        );

        let handler = chunks.iter().find(|c| c.ident == "Handler").unwrap();
        assert!(
            handler.signature.is_some(),
            "Handler should have a signature"
        );

        let status = chunks.iter().find(|c| c.ident == "Status").unwrap();
        assert!(status.signature.is_some(), "Status should have a signature");
    }

    // ============================================================
    // PHASE 2: Critical Reliability Tests
    // ============================================================

    /// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
    #[test]
    fn byte_offset_round_trip() {
        let source = r#"
public class Main {
    public static void main(String[] args) {
        System.out.println("Hello");
    }

    private int helper(int x) {
        return x * 2;
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

    /// Unicode identifiers.
    #[test]
    fn unicode_identifiers() {
        let source = r#"
public class Größe {
    public int berechne() {
        return 42;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let groesse = chunks.iter().find(|c| c.ident == "Größe");
        assert!(groesse.is_some(), "Should find class with German umlaut");
    }

    /// CRLF line endings.
    #[test]
    fn crlf_line_endings() {
        let source =
            "public class Foo {\r\n    public void bar() {\r\n        int x = 1;\r\n    }\r\n}\r\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let foo = chunks.iter().find(|c| c.ident == "Foo").unwrap();
        assert_eq!(foo.start_line, 1, "Start line should be 1");
        assert_eq!(foo.end_line, 5, "End line should account for CRLF");
    }

    /// Reference positions must be within their containing chunk.
    #[test]
    fn reference_positions_within_chunks() {
        let source = r#"
public class Service {
    public void process() {
        helper();
        otherMethod();
    }

    private void helper() {}
    private void otherMethod() {}
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

    /// Java records (16+) - likely NOT supported in tree-sitter-java 0.23.
    #[test]
    #[ignore = "Java records not supported in tree-sitter-java 0.23.5 (latest on crates.io)"]
    fn java_records() {
        let source = r#"
public record Point(int x, int y) {
    public double distance() {
        return Math.sqrt(x * x + y * y);
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let point = chunks.iter().find(|c| c.ident == "Point");
        assert!(point.is_some(), "Should find record Point");
    }

    /// Java sealed classes (15+) - likely NOT supported in tree-sitter-java 0.23.
    #[test]
    fn java_sealed_classes() {
        let source = r#"
public sealed class Shape permits Circle, Rectangle {
    abstract double area();
}

public final class Circle extends Shape {
    double radius;
    double area() { return Math.PI * radius * radius; }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let shape = chunks.iter().find(|c| c.ident == "Shape");
        assert!(shape.is_some(), "Should find sealed class Shape");
    }

    /// Java lambdas.
    #[test]
    fn java_lambdas() {
        let source = r#"
public class Service {
    public void process() {
        Runnable r = () -> System.out.println("hello");
        list.forEach(x -> doSomething(x));
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let process = chunks.iter().find(|c| c.ident == "process").unwrap();
        assert_eq!(process.kind, ChunkKind::Method);
        assert!(
            process.content.contains("->"),
            "Should contain lambda arrow"
        );
    }

    /// Java generics.
    #[test]
    fn java_generics() {
        let source = r#"
public class Container<T extends Comparable<T>> {
    private T value;

    public Container(T value) {
        this.value = value;
    }

    public <U> U transform(Function<T, U> fn) {
        return fn.apply(value);
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let container = chunks.iter().find(|c| c.ident == "Container").unwrap();
        assert_eq!(container.kind, ChunkKind::Class);
    }

    /// Java inner classes.
    #[test]
    fn java_inner_classes() {
        let source = r#"
public class Outer {
    private int x;

    public class Inner {
        public int getX() {
            return x;
        }
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let outer = chunks.iter().find(|c| c.ident == "Outer").unwrap();
        assert_eq!(outer.kind, ChunkKind::Class);

        let inner = chunks.iter().find(|c| c.ident == "Inner").unwrap();
        assert_eq!(inner.kind, ChunkKind::Class);
        assert_eq!(inner.parent.as_deref(), Some("Outer"));
    }

    /// Java constructor.
    #[test]
    fn java_constructor() {
        let source = r#"
public class Config {
    private String name;

    public Config(String name) {
        this.name = name;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let ctor = chunks
            .iter()
            .find(|c| c.ident == "Config" && c.kind == ChunkKind::Method);
        assert!(ctor.is_some(), "Should find constructor as a Method chunk");
    }

    /// Java annotations.
    #[test]
    fn java_annotations() {
        let source = r#"
public class Service {
    @Override
    public String toString() {
        return "Service";
    }

    @Deprecated
    public void oldMethod() {}
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let to_string = chunks.iter().find(|c| c.ident == "toString").unwrap();
        assert_eq!(to_string.kind, ChunkKind::Method);
    }

    // ============================================================
    // PHASE 3b: Latest Language Features (Java 21+)
    // ============================================================

    /// Text blocks (Java 15+).
    #[test]
    fn java_text_blocks() {
        let source = r#"
public class TextBlockExample {
    public String getJson() {
        return """
            {
                "name": "test",
                "value": 42
            }
            """;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "getJson");
        assert!(f.is_some(), "Should find method with text block");
    }

    /// Pattern matching in switch (Java 21+).
    #[test]
    #[ignore = "Java pattern matching switch not supported in tree-sitter-java 0.23.5"]
    fn java_pattern_matching_switch() {
        let source = r#"
public class PatternSwitch {
    public String describe(Object obj) {
        return switch (obj) {
            case Integer i -> "Integer: " + i;
            case String s -> "String: " + s;
            case null -> "null";
            default -> "Unknown";
        };
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "describe");
        assert!(
            f.is_some(),
            "Should find method with pattern matching switch"
        );
    }

    /// Local variable type inference with var.
    #[test]
    fn java_var_keyword() {
        let source = r#"
public class VarExample {
    public void process() {
        var list = new ArrayList<String>();
        var map = new HashMap<String, Integer>();
        for (var entry : map.entrySet()) {
            list.add(entry.getKey());
        }
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "process");
        assert!(f.is_some(), "Should find method using var keyword");
    }

    /// Static methods in interfaces.
    #[test]
    fn java_interface_static_methods() {
        let source = r#"
public interface Validator {
    boolean validate(String input);

    static Validator alwaysTrue() {
        return s -> true;
    }

    default boolean validateOrThrow(String input) {
        if (!validate(input)) {
            throw new IllegalArgumentException("Invalid: " + input);
        }
        return true;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let iface = chunks.iter().find(|c| c.ident == "Validator");
        assert!(
            iface.is_some(),
            "Should find interface with static and default methods"
        );
    }

    // ============================================================
    // PHASE 4: Fallback Mechanism Tests
    // ============================================================

    /// Parse with quality: clean code should be Complete.
    #[test]
    fn parse_with_quality_clean() {
        use crate::ingest::code::CodeParser;

        let source = r#"
public class Valid {
    public void method() {}
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
public class Broken {
    public void method( {
    }
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
// Single line comment
/* Block comment */
/** Javadoc comment */
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(
            chunks.is_empty(),
            "Comment-only file should produce no code chunks"
        );
    }

    /// Partial valid code.
    #[test]
    fn partial_valid_code() {
        let source = r#"
public class Valid {
    public void method() {}
}

public class Broken {
    public void method( {
"#;
        let result = parser().parse_chunks(source, 1);
        assert!(result.is_ok(), "Should not crash on partial valid code");
    }
}
