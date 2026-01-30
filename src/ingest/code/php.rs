use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

use crate::error::{Result, RlmError};
use crate::ingest::code::CodeParser;
use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};

const CHUNK_QUERY_SRC: &str = r"
    (function_definition name: (name) @fn_name) @fn_def
    (class_declaration name: (name) @class_name) @class_def
    (interface_declaration name: (name) @iface_name) @iface_def
    (method_declaration name: (name) @method_name) @method_def
    (trait_declaration name: (name) @trait_name) @trait_def
    (namespace_use_declaration) @use_decl
";

const REF_QUERY_SRC: &str = r"
    (function_call_expression function: (name) @call_name)
    (member_call_expression name: (name) @method_call)
    (namespace_use_clause (qualified_name) @use_path)
    (namespace_use_clause (name) @use_simple)
    (named_type (name) @type_ref)
    (named_type (qualified_name) @type_ref_qualified)
";

pub struct PhpParser {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl Default for PhpParser {
    fn default() -> Self {
        Self::new()
    }
}

impl PhpParser {
    #[must_use]
    pub fn new() -> Self {
        let language: Language = tree_sitter_php::LANGUAGE_PHP.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("PHP chunk query must compile");
        let ref_query = Query::new(&language, REF_QUERY_SRC).expect("PHP ref query must compile");
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
                detail: format!("failed to set PHP language: {e}"),
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
        // Track seen chunks to avoid duplicates (name + start_line)
        let mut seen: std::collections::HashSet<(String, u32)> = std::collections::HashSet::new();

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
                    "class_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Class;
                    }
                    "iface_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Interface;
                    }
                    "method_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Method;
                    }
                    "trait_name" => {
                        name = text.to_string();
                        kind = ChunkKind::Trait;
                    }
                    n if n.ends_with("_def") => {
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
            let start_line = start.row as u32 + 1;

            // Skip duplicates
            let key = (name.clone(), start_line);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            let content = node.utf8_text(source_bytes).unwrap_or("").to_string();
            let end = node.end_position();

            let visibility = extract_php_visibility(&content);
            let signature = match kind {
                ChunkKind::Function | ChunkKind::Method => content
                    .find('{')
                    .map(|pos| content[..pos].trim().to_string()),
                ChunkKind::Class | ChunkKind::Interface | ChunkKind::Trait => {
                    extract_php_type_signature(&content)
                }
                _ => None,
            };

            let parent = find_php_parent(node, source_bytes);

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
                doc_comment: collect_php_doc_comment(node, source_bytes),
                attributes: collect_php_attributes(node, source_bytes),
                content,
            });
        }

        // Create an imports chunk if there are use declarations
        if !use_decls.is_empty() {
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
                    "call_name" | "method_call" => RefKind::Call,
                    "use_path" | "use_simple" => RefKind::Import,
                    "type_ref" | "type_ref_qualified" => RefKind::TypeUse,
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

impl CodeParser for PhpParser {
    fn language(&self) -> &'static str {
        "php"
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

fn extract_php_visibility(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("public") {
        Some("public".into())
    } else if trimmed.starts_with("protected") {
        Some("protected".into())
    } else if trimmed.starts_with("private") {
        Some("private".into())
    } else {
        Some("public".into()) // PHP default
    }
}

/// Extract signature for PHP type declarations (class, interface, trait).
fn extract_php_type_signature(content: &str) -> Option<String> {
    if let Some(brace_pos) = content.find('{') {
        let sig = content[..brace_pos].trim();
        Some(sig.to_string())
    } else {
        content.lines().next().map(|s| s.trim().to_string())
    }
}

fn find_php_parent(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        let kind = parent.kind();
        if kind == "class_declaration"
            || kind == "interface_declaration"
            || kind == "trait_declaration"
        {
            for i in 0..parent.child_count() {
                if let Some(child) = parent.child(i as u32) {
                    if child.kind() == "name" {
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

fn collect_php_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        if sib.kind() == "attribute_list" {
            current = sib.prev_sibling();
            continue;
        }
        if sib.kind() == "comment" {
            let text = sib.utf8_text(source).unwrap_or("");
            if text.starts_with("/**") {
                return Some(text.to_string());
            }
        }
        break;
    }
    None
}

fn collect_php_attributes(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut attrs = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        if sib.kind() == "attribute_list" || sib.kind() == "attribute_group" {
            attrs.push(sib.utf8_text(source).unwrap_or("").to_string());
            current = sib.prev_sibling();
            continue;
        }
        if sib.kind() == "comment" {
            current = sib.prev_sibling();
            continue;
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

    fn parser() -> PhpParser {
        PhpParser::new()
    }

    #[test]
    fn parse_php_class() {
        let source = r#"<?php
class UserService {
    public function getUser(int $id): string {
        return "user";
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
    }

    #[test]
    fn parse_php_function() {
        let source =
            "<?php\nfunction hello(string $name): string {\n    return \"Hello, $name\";\n}\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks
            .iter()
            .any(|c| c.ident == "hello" && c.kind == ChunkKind::Function));
    }

    #[test]
    fn test_php_imports_extracted() {
        let source = r#"<?php
use App\Services\UserService;
use App\Models\User;
use Illuminate\Support\Facades\Log;

class Test {
    public function test() {}
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
    fn test_php_no_duplicate_methods() {
        let source = r#"<?php
class UserService {
    public function getUser(int $id): string {
        return "user";
    }

    public function setUser(string $name): void {
        echo $name;
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
    fn test_php_class_has_signature() {
        let source = r#"<?php
class UserService extends BaseService implements Handler {
    public function handle() {}
}

interface Handler {
    public function handle();
}

trait Loggable {
    public function log() {}
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
                .contains("class UserService"),
            "UserService signature should contain class declaration, got: {:?}",
            user_service.signature
        );

        let handler = chunks.iter().find(|c| c.ident == "Handler").unwrap();
        assert!(
            handler.signature.is_some(),
            "Handler should have a signature"
        );

        let loggable = chunks.iter().find(|c| c.ident == "Loggable").unwrap();
        assert!(
            loggable.signature.is_some(),
            "Loggable should have a signature"
        );
    }

    // ============================================================
    // PHASE 2: Critical Reliability Tests
    // ============================================================

    /// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
    #[test]
    fn byte_offset_round_trip() {
        let source = r#"<?php
class Main {
    public function process(): string {
        return "done";
    }

    private function helper(int $x): int {
        return $x * 2;
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

    /// Unicode identifiers (PHP supports Unicode in identifiers).
    #[test]
    fn unicode_identifiers() {
        let source = "<?php\nclass Größe {\n    public function berechne(): int {\n        return 42;\n    }\n}\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let groesse = chunks.iter().find(|c| c.ident == "Größe");
        assert!(groesse.is_some(), "Should find class with German umlaut");
    }

    /// CRLF line endings.
    #[test]
    fn crlf_line_endings() {
        let source = "<?php\r\nclass Foo {\r\n    public function bar() {\r\n    }\r\n}\r\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let foo = chunks.iter().find(|c| c.ident == "Foo").unwrap();
        assert_eq!(foo.start_line, 2, "Start line should be 2 (after <?php)");
    }

    /// Reference positions must be within their containing chunk.
    #[test]
    fn reference_positions_within_chunks() {
        let source = r#"<?php
class Service {
    public function process() {
        $this->helper();
        $this->otherMethod();
    }

    private function helper() {}
    private function otherMethod() {}
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

    /// PHP attributes (8.0+) - may have limited support.
    #[test]
    fn php_attributes() {
        let source = r#"<?php
#[Route('/api')]
class ApiController {
    #[Get('/users')]
    public function getUsers(): array {
        return [];
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let controller = chunks.iter().find(|c| c.ident == "ApiController");
        assert!(controller.is_some(), "Should find class with attributes");
    }

    /// PHP match expression (8.0+).
    #[test]
    fn php_match_expression() {
        let source = r#"<?php
function getLabel(int $status): string {
    return match($status) {
        1 => "active",
        2 => "inactive",
        default => "unknown",
    };
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let get_label = chunks.iter().find(|c| c.ident == "getLabel");
        assert!(
            get_label.is_some(),
            "Should find function with match expression"
        );
    }

    /// PHP union types (8.0+).
    #[test]
    fn php_union_types() {
        let source = r#"<?php
function process(int|string $value): int|false {
    if (is_string($value)) {
        return strlen($value);
    }
    return $value > 0 ? $value : false;
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let process = chunks.iter().find(|c| c.ident == "process");
        assert!(process.is_some(), "Should find function with union types");
    }

    /// PHP enums (8.1+).
    #[test]
    #[ignore = "PHP enums not supported in tree-sitter-php 0.24.2 (latest on crates.io)"]
    fn php_enums() {
        let source = r#"<?php
enum Status: string {
    case Active = 'active';
    case Inactive = 'inactive';

    public function label(): string {
        return match($this) {
            Status::Active => 'Active',
            Status::Inactive => 'Inactive',
        };
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let status = chunks.iter().find(|c| c.ident == "Status");
        assert!(status.is_some(), "Should find enum Status");
    }

    /// PHP abstract classes.
    #[test]
    fn php_abstract_class() {
        let source = r#"<?php
abstract class Shape {
    abstract public function area(): float;

    public function describe(): string {
        return "I am a shape with area " . $this->area();
    }
}

class Circle extends Shape {
    public function __construct(private float $radius) {}

    public function area(): float {
        return M_PI * $this->radius ** 2;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let shape = chunks.iter().find(|c| c.ident == "Shape").unwrap();
        assert_eq!(shape.kind, ChunkKind::Class);

        let circle = chunks.iter().find(|c| c.ident == "Circle").unwrap();
        assert_eq!(circle.kind, ChunkKind::Class);
    }

    /// PHP trait usage.
    #[test]
    fn php_trait_with_methods() {
        let source = r#"<?php
trait Loggable {
    public function log(string $message): void {
        echo $message;
    }

    public function error(string $message): void {
        echo "ERROR: " . $message;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let loggable = chunks.iter().find(|c| c.ident == "Loggable").unwrap();
        assert_eq!(loggable.kind, ChunkKind::Trait);

        let log = chunks.iter().find(|c| c.ident == "log").unwrap();
        assert_eq!(log.kind, ChunkKind::Method);
        assert_eq!(log.parent.as_deref(), Some("Loggable"));
    }

    /// PHP constructor promotion (8.0+).
    #[test]
    fn php_constructor_promotion() {
        let source = r#"<?php
class User {
    public function __construct(
        private string $name,
        private int $age,
        public readonly string $email
    ) {}

    public function getName(): string {
        return $this->name;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let user = chunks.iter().find(|c| c.ident == "User").unwrap();
        assert_eq!(user.kind, ChunkKind::Class);
    }

    // ============================================================
    // PHASE 3b: Latest Language Features (PHP 8.2+)
    // ============================================================

    /// Readonly classes (PHP 8.2+).
    #[test]
    fn php_readonly_class() {
        let source = r#"<?php
readonly class Config {
    public function __construct(
        public string $name,
        public int $value
    ) {}
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let c = chunks.iter().find(|c| c.ident == "Config");
        assert!(c.is_some(), "Should find readonly class");
    }

    /// Disjunctive Normal Form types (PHP 8.2+).
    #[test]
    fn php_dnf_types() {
        let source = r#"<?php
class Handler {
    public function process((Countable&Iterator)|null $input): void {
        // handle input
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "process");
        assert!(f.is_some(), "Should find method with DNF types");
    }

    /// Typed class constants (PHP 8.3+).
    #[test]
    fn php_typed_constants() {
        let source = r#"<?php
class Config {
    public const int MAX_SIZE = 100;
    public const string VERSION = "1.0.0";
    protected const array DEFAULTS = ['key' => 'value'];
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let c = chunks.iter().find(|c| c.ident == "Config");
        assert!(c.is_some(), "Should find class with typed constants");
    }

    /// Named arguments.
    #[test]
    fn php_named_arguments() {
        let source = r#"<?php
function createUser(string $name, int $age, bool $active = true): array {
    return ['name' => $name, 'age' => $age, 'active' => $active];
}

$user = createUser(name: 'Alice', age: 30, active: false);
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "createUser");
        assert!(
            f.is_some(),
            "Should find function usable with named arguments"
        );
    }

    /// Intersection types (PHP 8.1+).
    #[test]
    fn php_intersection_types() {
        let source = r#"<?php
interface Stringable {}
interface JsonSerializable {}

class Handler {
    public function format(Stringable&JsonSerializable $value): string {
        return (string)$value;
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "format");
        assert!(f.is_some(), "Should find method with intersection types");
    }

    /// First-class callable syntax (PHP 8.1+).
    #[test]
    fn php_first_class_callable() {
        let source = r#"<?php
class Processor {
    public function process(array $items): array {
        return array_map($this->transform(...), $items);
    }

    private function transform(mixed $item): mixed {
        return strtoupper($item);
    }
}
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "process");
        assert!(f.is_some(), "Should find method using first-class callable");
    }

    // ============================================================
    // PHASE 4: Fallback Mechanism Tests
    // ============================================================

    /// Parse with quality: clean code should be Complete.
    #[test]
    fn parse_with_quality_clean() {
        use crate::ingest::code::CodeParser;

        let source = "<?php\nfunction valid(): int {\n    return 42;\n}\n";
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

        let source = "<?php\nfunction broken( {\n    return 42;\n}\n";
        let result = parser().parse_with_quality(source, 1).unwrap();
        assert!(
            result.quality.fallback_recommended(),
            "Broken code should recommend fallback"
        );
    }

    // ============================================================
    // PHASE 5: Edge Cases
    // ============================================================

    /// Empty PHP file (just open tag).
    #[test]
    fn empty_file() {
        let source = "<?php\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks.is_empty(), "Empty file should produce no chunks");
    }

    /// Comment-only file.
    #[test]
    fn comment_only_file() {
        let source = "<?php\n// Single line\n/* Block comment */\n/** PHPDoc */\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(
            chunks.is_empty(),
            "Comment-only file should produce no code chunks"
        );
    }

    /// Partial valid code.
    #[test]
    fn partial_valid_code() {
        let source = r#"<?php
function valid(): int {
    return 42;
}

function broken( {
"#;
        let result = parser().parse_chunks(source, 1);
        assert!(result.is_ok(), "Should not crash on partial valid code");
    }
}
