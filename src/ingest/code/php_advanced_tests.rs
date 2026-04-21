//! Advanced parser tests for `php.rs` (PHASE 3 onward).
//!
//! Split out of `php_tests.rs` to keep each test companion focused
//! on a smaller cluster of behaviors (SRP_MODULE).

use super::PhpParser;
use crate::domain::chunk::ChunkKind;
use crate::ingest::code::CodeParser;

fn parser() -> PhpParser {
    PhpParser::create()
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
