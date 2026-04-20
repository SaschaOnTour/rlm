//! Advanced parser tests for `java.rs` (PHASE 3 onward).
//!
//! Split out of `java_tests.rs` to keep each test companion focused on a
//! smaller cluster of behaviors (SRP_MODULE).

use super::JavaParser;
use crate::domain::chunk::ChunkKind;
use crate::ingest::code::CodeParser;

fn parser() -> JavaParser {
    JavaParser::create()
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
