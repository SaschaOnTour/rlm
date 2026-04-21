//! Parser tests for `java.rs` (PHASE 1–2).
//!
//! Moved out of `java.rs` in slice 4.6 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "java_tests.rs"] mod tests;`.
//!
//! Advanced feature / edge-case tests (PHASE 3 onward) live in the sibling
//! `java_advanced_tests.rs`.

use super::JavaParser;
use crate::domain::chunk::{ChunkKind, RefKind};
use crate::ingest::code::CodeParser;

fn parser() -> JavaParser {
    JavaParser::create()
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
