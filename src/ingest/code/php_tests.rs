//! Parser tests for `php.rs`.
//!
//! Moved out of `php.rs` in slice 4.8 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "php_tests.rs"] mod tests;`.

use super::PhpParser;
use crate::domain::chunk::{ChunkKind, RefKind};
use crate::ingest::code::CodeParser;

fn parser() -> PhpParser {
    PhpParser::create()
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
    let source = "<?php\nfunction hello(string $name): string {\n    return \"Hello, $name\";\n}\n";
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
