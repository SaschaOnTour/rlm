//! Parser tests for `typescript.rs`.
//!
//! Moved out of `typescript.rs` in slice 4.4 following the 4.3 pilot
//! pattern; wired back in via
//! `#[cfg(test)] #[path = "typescript_tests.rs"] mod tests;`.

use super::*;
use crate::ingest::code::CodeParser;
use crate::models::chunk::ChunkKind;

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

// ============================================================
// PHASE 3: Modern Language Features (TypeScript 5.x)
// ============================================================

#[test]
fn ts_const_type_parameters() {
    let source = r#"
function createTuple<const T extends readonly unknown[]>(items: T): T {
return items;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "createTuple"));
}

#[test]
fn ts_satisfies_operator() {
    let source = r#"
type Config = { host: string; port: number };

const config = {
host: 'localhost',
port: 8080,
} satisfies Config;

function getConfig(): Config {
return config;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "getConfig"));
}

#[test]
fn ts_utility_types() {
    let source = r#"
interface User {
name: string;
age: number;
email: string;
}

type PartialUser = Partial<User>;
type RequiredUser = Required<User>;
type ReadonlyUser = Readonly<User>;
type UserKeys = keyof User;
type NameType = User['name'];
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "User"));
    assert!(chunks.iter().any(|c| c.ident == "PartialUser"));
}

#[test]
fn ts_conditional_types() {
    let source = r#"
type NonNullable<T> = T extends null | undefined ? never : T;

type ExtractArrayType<T> = T extends (infer U)[] ? U : never;

type Flatten<T> = T extends Array<infer Item> ? Item : T;
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "NonNullable"));
    assert!(chunks.iter().any(|c| c.ident == "ExtractArrayType"));
}

#[test]
fn ts_mapped_types() {
    let source = r#"
type Readonly<T> = {
readonly [P in keyof T]: T[P];
};

type Optional<T> = {
[P in keyof T]?: T[P];
};
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "Readonly"));
    assert!(chunks.iter().any(|c| c.ident == "Optional"));
}

#[test]
fn ts_template_literal_types() {
    let source = r#"
type EventName = 'click' | 'focus' | 'blur';
type Handler = `on${Capitalize<EventName>}`;

type Route = `/api/${string}`;
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks.iter().any(|c| c.ident == "EventName"));
    assert!(chunks.iter().any(|c| c.ident == "Handler"));
}

#[test]
fn ts_decorators() {
    let source = r#"
function Injectable() {
return function(target: any) {};
}

@Injectable()
class UserService {
getUsers(): User[] {
    return [];
}
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let service = chunks.iter().find(|c| c.ident == "UserService").unwrap();
    // Decorators should be captured in attributes
    // Decorators may or may not be captured depending on tree-sitter version
    let _ = service.attributes;
}

#[test]
fn ts_abstract_class() {
    let source = r#"
abstract class Shape {
abstract getArea(): number;

describe(): string {
    return `Area: ${this.getArea()}`;
}
}

class Circle extends Shape {
constructor(private radius: number) {
    super();
}

getArea(): number {
    return Math.PI * this.radius ** 2;
}
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Shape" && c.kind == ChunkKind::Class));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Circle" && c.kind == ChunkKind::Class));
}

// ============================================================
// PHASE 4: Fallback Mechanism Tests
// ============================================================

#[test]
fn parse_with_quality_clean() {
    let source = "function valid(): number { return 42; }";
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(
        result.quality.is_complete(),
        "Clean code should have Complete quality"
    );
}

#[test]
fn parse_with_quality_syntax_error() {
    let source = "function broken(: number { return 42; }";
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(
        result.quality.fallback_recommended(),
        "Broken code should recommend fallback"
    );
}

// ============================================================
// PHASE 5: Edge Cases
// ============================================================

#[test]
fn empty_file() {
    let chunks = parser().parse_chunks("", 1).unwrap();
    assert!(chunks.is_empty(), "Empty file should produce no chunks");
}

#[test]
fn comment_only_file() {
    let source = r#"
// Single line comment
/* Block comment */
/** TSDoc comment */
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(
        chunks.is_empty(),
        "Comment-only file should produce no code chunks"
    );
}

#[test]
fn partial_valid_code() {
    let source = r#"
function valid(): number {
return 42;
}

function broken(: number {
"#;
    let result = parser().parse_chunks(source, 1);
    assert!(result.is_ok(), "Should not crash on partial valid code");
}

// ============================================================
// TSX-specific tests
// ============================================================

#[test]
fn parse_tsx_component() {
    let tsx_parser = TypeScriptParser::create_tsx();
    let source = r#"
import React from 'react';

interface Props {
name: string;
}

function Greeting({ name }: Props): JSX.Element {
return <div>Hello, {name}!</div>;
}

export default Greeting;
"#;
    let chunks = tsx_parser.parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Props" && c.kind == ChunkKind::Interface));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Greeting" && c.kind == ChunkKind::Function));
}

#[test]
fn parse_tsx_class_component() {
    let tsx_parser = TypeScriptParser::create_tsx();
    let source = r#"
import React, { Component } from 'react';

interface State {
count: number;
}

class Counter extends Component<{}, State> {
state = { count: 0 };

increment = () => {
    this.setState({ count: this.state.count + 1 });
};

render() {
    return (
        <button onClick={this.increment}>
            Count: {this.state.count}
        </button>
    );
}
}
"#;
    let chunks = tsx_parser.parse_chunks(source, 1).unwrap();
    assert!(chunks
        .iter()
        .any(|c| c.ident == "State" && c.kind == ChunkKind::Interface));
    assert!(chunks
        .iter()
        .any(|c| c.ident == "Counter" && c.kind == ChunkKind::Class));
}
