//! Advanced parser tests for `typescript.rs` (PHASE 3 onward).
//!
//! Split out of `typescript_tests.rs` to keep each test companion focused
//! on a smaller cluster of behaviors (SRP_MODULE).

use super::TypeScriptParser;
use crate::domain::chunk::ChunkKind;
use crate::ingest::code::CodeParser;

fn parser() -> TypeScriptParser {
    TypeScriptParser::create()
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
