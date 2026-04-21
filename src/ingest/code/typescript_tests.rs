//! Parser tests for `typescript.rs`.
//!
//! Moved out of `typescript.rs` in slice 4.4 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "typescript_tests.rs"] mod tests;`.

use super::TypeScriptParser;
use crate::domain::chunk::ChunkKind;
use crate::ingest::code::CodeParser;

fn parser() -> TypeScriptParser {
    TypeScriptParser::create()
}

#[test]
fn parse_ts_function() {
    let source = r#"
function hello(name: string): string {
return "Hello, " + name;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "hello" && c.kind == ChunkKind::Function));
}

#[test]
fn parse_ts_interface() {
    let source = r#"
interface User {
name: string;
age: number;
email?: string;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let iface = chunks.iter().find(|c| c.ident == "User").unwrap();
    assert_eq!(iface.kind, ChunkKind::Interface);
}

#[test]
fn parse_ts_type_alias() {
    let source = r#"
type Status = 'active' | 'inactive' | 'pending';

type UserMap = Map<string, User>;
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "Status"));
    assert!(chunks.iter().any(|c| c.ident == "UserMap"));
}

#[test]
fn parse_ts_enum() {
    let source = r#"
enum Direction {
Up = 1,
Down = 2,
Left = 3,
Right = 4,
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let dir = chunks.iter().find(|c| c.ident == "Direction").unwrap();
    assert_eq!(dir.kind, ChunkKind::Enum);
}

#[test]
fn parse_ts_class_with_types() {
    let source = r#"
class UserService {
private users: User[] = [];

constructor(private readonly db: Database) {}

async getUser(id: string): Promise<User | null> {
    return this.users.find(u => u.id === id) ?? null;
}
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "UserService" && c.kind == ChunkKind::Class));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "constructor" && c.kind == ChunkKind::Method));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "getUser" && c.kind == ChunkKind::Method));
}

#[test]
fn parse_ts_generics() {
    let source = r#"
function identity<T>(value: T): T {
return value;
}

interface Repository<T> {
find(id: string): T | null;
save(item: T): void;
}

class GenericClass<T, U> {
constructor(public first: T, public second: U) {}
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "identity" && c.kind == ChunkKind::Function));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Repository" && c.kind == ChunkKind::Interface));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "GenericClass" && c.kind == ChunkKind::Class));
}

#[test]
fn parse_ts_imports() {
    let source = r#"
import React from 'react';
import { useState, useEffect } from 'react';
import type { User } from './types';

function Component() {
const [state, setState] = useState(0);
return <div>{state}</div>;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let imports_chunk = chunks.iter().find(|c| c.ident == "_imports");
    assert!(imports_chunk.is_some(), "Should have an _imports chunk");
}

#[test]
fn validate_ts_syntax() {
    assert!(parser().validate_syntax("function foo(): number { return 1; }"));
    assert!(!parser().validate_syntax("function foo(): { return 1; }"));
}

// ============================================================
// PHASE 2: Critical Reliability Tests
// ============================================================

#[test]
fn byte_offset_round_trip() {
    let source = r#"
function hello(name: string): string {
return "Hello, " + name;
}

interface User {
name: string;
}

class Greeter {
greet(): string {
    return "Hi!";
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

#[test]
fn unicode_identifiers() {
    let source = r#"
function größe(): number {
return 42;
}

interface 名前 {
value: string;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let groesse = chunks.iter().find(|c| c.ident == "größe");
    assert!(groesse.is_some(), "Should find function with German umlaut");
}

#[test]
fn crlf_line_endings() {
    let source = "function foo(): number {\r\n    return 1;\r\n}\r\n";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let foo = chunks.iter().find(|c| c.ident == "foo").unwrap();
    assert_eq!(foo.start_line, 1, "Start line should be 1");
}

#[test]
fn reference_positions_within_chunks() {
    let source = r#"
class Service {
process(): void {
    this.helper();
    this.other();
}

helper(): void {}
other(): void {}
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
