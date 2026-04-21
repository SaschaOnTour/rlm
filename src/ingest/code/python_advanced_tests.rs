//! Advanced parser tests for `python.rs` (PHASE 3 onward).
//!
//! Split out of `python_tests.rs` to keep each test companion focused
//! on a smaller cluster of behaviors (SRP_MODULE).

use super::PythonParser;
use crate::domain::chunk::ChunkKind;
use crate::ingest::code::CodeParser;

fn parser() -> PythonParser {
    PythonParser::create()
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
