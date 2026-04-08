use tree_sitter::{Language, Query};

use crate::ingest::code::base::{
    build_language_config, extract_type_signature_to_colon, find_parent_by_kind, BaseParser,
    ChunkCaptureResult, LanguageConfig,
};
use crate::models::chunk::{ChunkKind, RefKind};

const CHUNK_QUERY_SRC: &str = r"
    (function_definition name: (identifier) @fn_name) @fn_def
    (class_definition name: (identifier) @class_name) @class_def
    (import_statement) @import_decl
    (import_from_statement) @import_decl
";

const REF_QUERY_SRC: &str = r"
    (call function: (identifier) @call_name)
    (call function: (attribute attribute: (identifier) @method_call))
    (import_statement name: (dotted_name) @import_name)
    (import_from_statement module_name: (dotted_name) @import_from_module)
    (import_from_statement name: (dotted_name) @import_from_name)
    (aliased_import name: (dotted_name) @import_alias)
    (type) @type_ref
";

pub struct PythonConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl PythonConfig {
    fn new() -> Self {
        let (language, chunk_query, ref_query) = build_language_config(
            tree_sitter_python::LANGUAGE.into(),
            CHUNK_QUERY_SRC,
            REF_QUERY_SRC,
            "Python",
        );
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for PythonConfig {
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
        "python"
    }

    fn import_capture_name(&self) -> &'static str {
        "import_decl"
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "fn_name" => Some(ChunkCaptureResult::name(
                text.to_string(),
                ChunkKind::Function,
            )),
            "class_name" => Some(ChunkCaptureResult::name(text.to_string(), ChunkKind::Class)),
            "fn_def" | "class_def" => Some(ChunkCaptureResult::definition()),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "import_name" | "import_from_module" | "import_from_name" | "import_alias" => {
                Some(RefKind::Import)
            }
            "type_ref" => Some(RefKind::TypeUse),
            _ => None,
        }
    }

    fn extract_visibility(&self, _content: &str) -> Option<String> {
        // Python visibility is based on name, not content.
        // We handle it in adjust_chunk_metadata instead.
        // Return a placeholder that will be overwritten.
        None
    }

    fn adjust_chunk_metadata(
        &self,
        kind: &mut ChunkKind,
        name: &str,
        parent: &Option<String>,
        visibility: &mut Option<String>,
    ) {
        // Promote Function to Method when inside a class
        if parent.is_some() && *kind == ChunkKind::Function {
            *kind = ChunkKind::Method;
        }

        // Python visibility: _private, __dunder__, public
        *visibility = if name.starts_with("__") && name.ends_with("__") {
            Some("dunder".into())
        } else if name.starts_with('_') {
            Some("private".into())
        } else {
            Some("public".into())
        };
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        match kind {
            ChunkKind::Function | ChunkKind::Method => content
                .find(':')
                .map(|pos| content[..pos].trim().to_string()),
            ChunkKind::Class => extract_type_signature_to_colon(content),
            _ => None,
        }
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_parent_by_kind(node, source, &["class_definition"], "identifier")
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_python_docstring(node, source).or_else(|| collect_python_comment(node, source))
    }

    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_python_decorators(node, source)
    }
}

/// Public type alias for the Python parser.
pub type PythonParser = BaseParser<PythonConfig>;

impl Default for PythonParser {
    fn default() -> Self {
        Self::new(PythonConfig::new())
    }
}

impl PythonParser {
    /// Create a new Python parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(PythonConfig::new())
    }
}

fn collect_python_docstring(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Python docstrings are INSIDE the function/class body, not before it
    let body = node.child_by_field_name("body")?;
    // body is a "block" node; first child after ":" could be a string expression
    for i in 0..body.child_count() {
        let child = match body.child(i as u32) {
            Some(c) => c,
            None => continue,
        };
        if child.kind() == "expression_statement" {
            let str_node = match child.child(0) {
                Some(n) if n.kind() == "string" => n,
                _ => continue,
            };
            return str_node
                .utf8_text(source)
                .ok()
                .map(std::string::ToString::to_string);
        }
        // Skip newline/indent nodes but stop at non-string statements
        if child.kind() != "comment" {
            break;
        }
    }
    None
}

fn collect_python_decorators(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Check if this function/class is wrapped in a decorated_definition
    let parent = node.parent()?;
    if parent.kind() != "decorated_definition" {
        return None;
    }
    let decorators: Vec<String> = (0..parent.child_count())
        .filter_map(|i| parent.child(i as u32))
        .filter(|c| c.kind() == "decorator")
        .map(|c| c.utf8_text(source).unwrap_or("").to_string())
        .collect();

    if decorators.is_empty() {
        None
    } else {
        Some(decorators.join("\n"))
    }
}

fn collect_python_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    // Collect preceding # comments (not decorators)
    let mut lines = Vec::new();
    let check_node = if let Some(parent) = node.parent() {
        if parent.kind() == "decorated_definition" {
            parent
        } else {
            node
        }
    } else {
        node
    };
    let mut current = check_node.prev_sibling();
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
    use crate::ingest::code::CodeParser;
    use crate::models::chunk::{ChunkKind, RefKind};

    fn parser() -> PythonParser {
        PythonParser::create()
    }

    #[test]
    fn parse_python_function() {
        let source = "def hello(name: str) -> str:\n    return f'Hello, {name}'\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "hello").unwrap();
        assert_eq!(f.kind, ChunkKind::Function);
        assert_eq!(f.visibility.as_deref(), Some("public"));
    }

    #[test]
    fn parse_python_class_with_methods() {
        let source = r#"
class UserService:
    def __init__(self, db):
        self.db = db

    def get_user(self, user_id):
        return self.db.find(user_id)

    def _internal(self):
        pass
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(chunks
            .iter()
            .any(|c| c.ident == "UserService" && c.kind == ChunkKind::Class));
        let init = chunks.iter().find(|c| c.ident == "__init__").unwrap();
        assert_eq!(init.kind, ChunkKind::Method);
        assert_eq!(init.parent.as_deref(), Some("UserService"));
        assert_eq!(init.visibility.as_deref(), Some("dunder"));

        let internal = chunks.iter().find(|c| c.ident == "_internal").unwrap();
        assert_eq!(internal.visibility.as_deref(), Some("private"));
    }

    #[test]
    fn validate_python_syntax() {
        assert!(parser().validate_syntax("def foo():\n    pass\n"));
    }

    #[test]
    fn test_python_imports_extracted() {
        let source = r#"
import os
import sys
from datetime import datetime
from collections import defaultdict, OrderedDict
import json as j

def main():
    pass
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
            "Should capture at least 3 import refs, got {}",
            import_refs.len()
        );
    }

    #[test]
    fn test_python_class_has_signature() {
        let source = r#"
class UserService(BaseService, Mixin):
    def __init__(self, db):
        self.db = db

class SimpleClass:
    pass
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
        assert!(
            user_service
                .signature
                .as_ref()
                .unwrap()
                .contains("BaseService"),
            "UserService signature should contain base class, got: {:?}",
            user_service.signature
        );

        let simple_class = chunks.iter().find(|c| c.ident == "SimpleClass").unwrap();
        assert!(
            simple_class.signature.is_some(),
            "SimpleClass should have a signature"
        );
    }

    // ============================================================
    // PHASE 2: Critical Reliability Tests
    // ============================================================

    /// CRITICAL: Byte offsets must allow exact reconstruction of chunk content.
    #[test]
    fn byte_offset_round_trip() {
        let source = r#"
def hello(name):
    return f"Hello, {name}"

class Config:
    def __init__(self, name):
        self.name = name
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
        let source = "def größe():\n    return 42\n\ndef 计算():\n    return 0\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let groesse = chunks.iter().find(|c| c.ident == "größe");
        assert!(groesse.is_some(), "Should find function with German umlaut");

        let chinese = chunks.iter().find(|c| c.ident == "计算");
        assert!(chinese.is_some(), "Should find function with Chinese name");
    }

    /// CRLF line endings.
    #[test]
    fn crlf_line_endings() {
        let source = "def foo():\r\n    return 42\r\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let f = chunks.iter().find(|c| c.ident == "foo").unwrap();
        assert_eq!(f.start_line, 1, "Start line should be 1");
        assert_eq!(f.end_line, 2, "End line should be 2 with CRLF");
    }

    /// Reference positions must be within their containing chunk.
    #[test]
    fn reference_positions_within_chunks() {
        let source = r#"
def caller():
    helper()
    other_fn()

def helper():
    return 42

def other_fn():
    return 0
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

    /// Python match statement (3.10+) - likely NOT supported.
    #[test]
    fn python_match_statement() {
        let source = r#"
def process(command):
    match command:
        case "quit":
            return False
        case "hello":
            return True
        case _:
            return None
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let process = chunks.iter().find(|c| c.ident == "process");
        assert!(
            process.is_some(),
            "Should find function with match statement"
        );
    }

    /// Python type hints.
    #[test]
    fn python_type_hints() {
        let source = r#"
def process(items: list[int], name: str = "default") -> dict[str, int]:
    return {name: sum(items)}

class Config:
    name: str
    port: int = 8080

    def __init__(self, name: str, port: int = 8080) -> None:
        self.name = name
        self.port = port
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let process = chunks.iter().find(|c| c.ident == "process").unwrap();
        assert_eq!(process.kind, ChunkKind::Function);
        // Signature is extracted up to the first colon (Python convention).
        // The full type-annotated content is in chunk.content.
        assert!(
            process.content.contains("list[int]"),
            "Content should contain type hints"
        );
        assert!(
            process.signature.as_ref().unwrap().contains("def process"),
            "Signature should contain function name"
        );
    }

    /// Python async/await.
    #[test]
    fn python_async_await() {
        let source = r#"
async def fetch_data(url: str) -> str:
    async with aiohttp.ClientSession() as session:
        async for chunk in response.content:
            pass
    return "done"
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let fetch = chunks.iter().find(|c| c.ident == "fetch_data");
        assert!(fetch.is_some(), "Should find async function");
        assert!(
            fetch.unwrap().content.contains("async def"),
            "Content should contain async def"
        );
    }

    /// Python decorators.
    #[test]
    fn python_decorators() {
        let source = r#"
class Service:
    @staticmethod
    def create():
        return Service()

    @classmethod
    def from_config(cls, config):
        return cls()

    @property
    def name(self):
        return self._name
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let create = chunks.iter().find(|c| c.ident == "create").unwrap();
        assert_eq!(create.kind, ChunkKind::Method);

        let from_config = chunks.iter().find(|c| c.ident == "from_config").unwrap();
        assert_eq!(from_config.kind, ChunkKind::Method);
    }

    /// Python nested functions.
    #[test]
    fn python_nested_functions() {
        let source = r#"
def outer():
    def inner():
        return 42
    return inner()
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let outer = chunks.iter().find(|c| c.ident == "outer").unwrap();
        assert_eq!(outer.kind, ChunkKind::Function);
        // inner might or might not be extracted as a separate chunk
    }

    /// Python dataclass-like.
    #[test]
    fn python_dataclass() {
        let source = r#"
from dataclasses import dataclass

@dataclass
class Point:
    x: float
    y: float

    def distance(self) -> float:
        return (self.x ** 2 + self.y ** 2) ** 0.5
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let point = chunks.iter().find(|c| c.ident == "Point").unwrap();
        assert_eq!(point.kind, ChunkKind::Class);
    }

    // ============================================================
    // PHASE 3b: Latest Language Features (Python 3.12+)
    // ============================================================

    /// Exception groups (Python 3.11+).
    #[test]
    fn python_exception_groups() {
        let source = r#"
def handle_errors():
    try:
        raise ExceptionGroup("multiple", [ValueError("bad"), TypeError("wrong")])
    except* ValueError as eg:
        print(f"Value errors: {eg}")
    except* TypeError as eg:
        print(f"Type errors: {eg}")
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "handle_errors");
        assert!(f.is_some(), "Should find function with exception groups");
    }

    /// Type parameter syntax (Python 3.12+).
    #[test]
    fn python_type_parameter_syntax() {
        let source = r#"
def first[T](items: list[T]) -> T:
    return items[0]

class Stack[T]:
    def __init__(self) -> None:
        self._items: list[T] = []

    def push(self, item: T) -> None:
        self._items.append(item)
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "first");
        assert!(f.is_some(), "Should find generic function");
        let c = chunks.iter().find(|c| c.ident == "Stack");
        assert!(c.is_some(), "Should find generic class");
    }

    /// Type statement (Python 3.12+).
    #[test]
    fn python_type_statement() {
        let source = r#"
type Point = tuple[float, float]
type IntFunc = Callable[[int], int]

def distance(p1: Point, p2: Point) -> float:
    return ((p2[0] - p1[0])**2 + (p2[1] - p1[1])**2)**0.5
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "distance");
        assert!(f.is_some(), "Should find function using type alias");
    }

    /// Positional-only and keyword-only parameters.
    #[test]
    fn python_param_separators() {
        let source = r#"
def complex_params(pos_only, /, standard, *, kw_only):
    return pos_only + standard + kw_only

def with_defaults(x, /, y=10, *, z=20):
    return x + y + z
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f1 = chunks.iter().find(|c| c.ident == "complex_params");
        assert!(f1.is_some(), "Should find function with param separators");
        let f2 = chunks.iter().find(|c| c.ident == "with_defaults");
        assert!(
            f2.is_some(),
            "Should find function with defaults and separators"
        );
    }

    /// Walrus operator.
    #[test]
    fn python_walrus_operator() {
        let source = r#"
def process_data(data):
    if (n := len(data)) > 10:
        return f"Large: {n}"
    while (line := data.readline()):
        print(line)
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "process_data");
        assert!(f.is_some(), "Should find function with walrus operator");
    }

    /// F-string nesting (Python 3.12+).
    #[test]
    fn python_fstring_nesting() {
        let source = r#"
def nested_fstrings():
    items = ["a", "b", "c"]
    return f"Items: {', '.join(f'{x!r}' for x in items)}"
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();
        let f = chunks.iter().find(|c| c.ident == "nested_fstrings");
        assert!(f.is_some(), "Should find function with nested f-strings");
    }

    // ============================================================
    // PHASE 4: Fallback Mechanism Tests
    // ============================================================

    /// Parse with quality: clean code should be Complete.
    #[test]
    fn parse_with_quality_clean() {
        use crate::ingest::code::CodeParser;

        let source = "def valid():\n    return 42\n";
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

        let source = "def broken(:\n    return 42\n";
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
        let source = "# This is a comment\n\"\"\"This is a docstring\"\"\"\n";
        let chunks = parser().parse_chunks(source, 1).unwrap();
        assert!(
            chunks.is_empty(),
            "Comment-only file should produce no code chunks"
        );
    }

    /// Partial valid code.
    #[test]
    fn partial_valid_code() {
        let source = "def valid():\n    return 42\n\ndef broken(:\n";
        let result = parser().parse_chunks(source, 1);
        assert!(result.is_ok(), "Should not crash on partial valid code");
    }

    /// Indentation-sensitive: ensure methods stay inside classes.
    #[test]
    fn indentation_method_parenting() {
        let source = r#"
class First:
    def method_a(self):
        pass

class Second:
    def method_b(self):
        pass
"#;
        let chunks = parser().parse_chunks(source, 1).unwrap();

        let method_a = chunks.iter().find(|c| c.ident == "method_a").unwrap();
        assert_eq!(method_a.parent.as_deref(), Some("First"));

        let method_b = chunks.iter().find(|c| c.ident == "method_b").unwrap();
        assert_eq!(method_b.parent.as_deref(), Some("Second"));
    }
}
