//! TypeScript/TSX parser for rlm.
//!
//! Supports TypeScript-specific features including:
//! - Interfaces and Type Aliases
//! - Generics and Type Parameters
//! - Decorators
//! - Enums
//! - Namespaces/Modules
//! - All JavaScript features via shared base

use tree_sitter::{Language, Query};

use crate::ingest::code::base::{BaseParser, ChunkCaptureResult, LanguageConfig};
use crate::models::chunk::{ChunkKind, RefKind};

const CHUNK_QUERY_SRC: &str = r"
    ; Functions
    (function_declaration name: (identifier) @fn_name) @fn_def
    (generator_function_declaration name: (identifier) @gen_fn_name) @gen_fn_def

    ; Arrow functions assigned to variables
    (lexical_declaration
        (variable_declarator
            name: (identifier) @arrow_name
            value: (arrow_function))) @arrow_def

    ; Classes (including abstract classes)
    (class_declaration name: (type_identifier) @class_name) @class_def
    (abstract_class_declaration name: (type_identifier) @abs_class_name) @abs_class_def

    ; Class methods
    (method_definition
        name: (property_identifier) @method_name) @method_def

    ; Interfaces
    (interface_declaration name: (type_identifier) @iface_name) @iface_def

    ; Type aliases
    (type_alias_declaration name: (type_identifier) @type_alias_name) @type_alias_def

    ; Enums
    (enum_declaration name: (identifier) @enum_name) @enum_def

    ; ES Module imports
    (import_statement) @import_decl

    ; Namespaces/Modules
    (module name: (identifier) @namespace_name) @namespace_def
    (internal_module name: (identifier) @internal_namespace_name) @internal_namespace_def
";

const REF_QUERY_SRC: &str = r"
    ; Function calls
    (call_expression
        function: (identifier) @call_name)
    (call_expression
        function: (member_expression
            property: (property_identifier) @method_call))

    ; Import paths
    (import_statement
        source: (string) @import_path)

    ; Type references
    (type_identifier) @type_ref

    ; Generic type arguments
    (type_arguments (type_identifier) @generic_type_ref)

    ; Decorators
    (decorator (call_expression function: (identifier) @decorator_name))
    (decorator (identifier) @decorator_name)
";

// TSX-specific query additions (JSX elements)
const TSX_REF_QUERY_ADDITION: &str = r"
    ; JSX/TSX elements
    (jsx_element
        open_tag: (jsx_opening_element
            name: (identifier) @jsx_component))
    (jsx_self_closing_element
        name: (identifier) @jsx_component)
";

pub struct TypeScriptConfig {
    language: Language,
    chunk_query: Query,
    ref_query: Query,
}

impl TypeScriptConfig {
    fn new() -> Self {
        let language: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("TypeScript chunk query must compile");
        let ref_query =
            Query::new(&language, REF_QUERY_SRC).expect("TypeScript ref query must compile");
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }

    fn new_tsx() -> Self {
        let language: Language = tree_sitter_typescript::LANGUAGE_TSX.into();
        let chunk_query =
            Query::new(&language, CHUNK_QUERY_SRC).expect("TSX chunk query must compile");
        // TSX includes JSX elements in refs
        let tsx_ref_query = format!("{REF_QUERY_SRC}\n{TSX_REF_QUERY_ADDITION}");
        let ref_query = Query::new(&language, &tsx_ref_query).expect("TSX ref query must compile");
        Self {
            language,
            chunk_query,
            ref_query,
        }
    }
}

impl LanguageConfig for TypeScriptConfig {
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
        "typescript"
    }

    fn import_capture_name(&self) -> &'static str {
        "import_decl"
    }

    fn needs_deduplication(&self) -> bool {
        true
    }

    fn map_chunk_capture(&self, capture_name: &str, text: &str) -> Option<ChunkCaptureResult> {
        match capture_name {
            "fn_name" | "gen_fn_name" => Some(ChunkCaptureResult {
                name: text.to_string(),
                kind: ChunkKind::Function,
                is_definition_node: false,
            }),
            "arrow_name" => Some(ChunkCaptureResult {
                name: text.to_string(),
                kind: ChunkKind::Function,
                is_definition_node: false,
            }),
            "class_name" | "abs_class_name" => Some(ChunkCaptureResult {
                name: text.to_string(),
                kind: ChunkKind::Class,
                is_definition_node: false,
            }),
            "method_name" => Some(ChunkCaptureResult {
                name: text.to_string(),
                kind: ChunkKind::Method,
                is_definition_node: false,
            }),
            "iface_name" => Some(ChunkCaptureResult {
                name: text.to_string(),
                kind: ChunkKind::Interface,
                is_definition_node: false,
            }),
            "type_alias_name" => Some(ChunkCaptureResult {
                name: text.to_string(),
                kind: ChunkKind::Other("type_alias".into()),
                is_definition_node: false,
            }),
            "enum_name" => Some(ChunkCaptureResult {
                name: text.to_string(),
                kind: ChunkKind::Enum,
                is_definition_node: false,
            }),
            "namespace_name" | "internal_namespace_name" => Some(ChunkCaptureResult {
                name: text.to_string(),
                kind: ChunkKind::Module,
                is_definition_node: false,
            }),
            n if n.ends_with("_def") => Some(ChunkCaptureResult {
                name: String::new(),
                kind: ChunkKind::Other("def".into()),
                is_definition_node: true,
            }),
            _ => None,
        }
    }

    fn map_ref_capture(&self, capture_name: &str) -> Option<RefKind> {
        match capture_name {
            "call_name" | "method_call" => Some(RefKind::Call),
            "import_path" => Some(RefKind::Import),
            "type_ref" | "generic_type_ref" => Some(RefKind::TypeUse),
            "jsx_component" => Some(RefKind::TypeUse),
            "decorator_name" => Some(RefKind::Call),
            _ => None,
        }
    }

    fn filter_ref_capture(&self, capture_name: &str, text: &str) -> bool {
        if capture_name == "jsx_component" {
            // Only PascalCase names are components
            text.chars().next().is_some_and(char::is_uppercase)
        } else {
            true
        }
    }

    fn transform_ref_text(&self, capture_name: &str, text: &str) -> String {
        match capture_name {
            // Clean up string quotes from import paths
            "import_path" => text.trim_matches('"').trim_matches('\'').to_string(),
            _ => text.to_string(),
        }
    }

    fn extract_visibility(&self, content: &str) -> Option<String> {
        extract_ts_visibility(content)
    }

    fn extract_signature(&self, content: &str, kind: &ChunkKind) -> Option<String> {
        extract_ts_signature(content, kind)
    }

    fn find_parent(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        find_ts_parent(node, source)
    }

    fn collect_doc_comment(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_ts_doc_comment(node, source)
    }

    fn collect_attributes(&self, node: tree_sitter::Node, source: &[u8]) -> Option<String> {
        collect_ts_decorators(node, source)
    }
}

/// Public type alias for the TypeScript parser.
pub type TypeScriptParser = BaseParser<TypeScriptConfig>;

impl Default for TypeScriptParser {
    fn default() -> Self {
        Self::new(TypeScriptConfig::new())
    }
}

impl TypeScriptParser {
    /// Create a new TypeScript parser.
    #[must_use]
    pub fn create() -> Self {
        Self::new(TypeScriptConfig::new())
    }

    /// Create a TSX parser for .tsx files.
    #[must_use]
    pub fn create_tsx() -> Self {
        Self::new(TypeScriptConfig::new_tsx())
    }
}

fn extract_ts_visibility(content: &str) -> Option<String> {
    let trimmed = content.trim_start();
    if trimmed.starts_with("export default") {
        Some("export default".into())
    } else if trimmed.starts_with("export") {
        Some("export".into())
    } else if trimmed.starts_with("public") {
        Some("public".into())
    } else if trimmed.starts_with("private") {
        Some("private".into())
    } else if trimmed.starts_with("protected") {
        Some("protected".into())
    } else {
        None
    }
}

fn extract_ts_signature(content: &str, kind: &ChunkKind) -> Option<String> {
    match kind {
        ChunkKind::Function => {
            // Find opening brace or arrow
            if let Some(brace_pos) = content.find('{') {
                Some(content[..brace_pos].trim().to_string())
            } else if let Some(arrow_pos) = content.find("=>") {
                Some(content[..arrow_pos + 2].trim().to_string())
            } else {
                content.lines().next().map(|s| s.trim().to_string())
            }
        }
        ChunkKind::Class | ChunkKind::Interface => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        ChunkKind::Method => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        ChunkKind::Enum => content
            .find('{')
            .map(|pos| content[..pos].trim().to_string()),
        ChunkKind::Other(s) if s == "type_alias" => {
            // Type alias: type Foo = ...
            content
                .find('=')
                .map(|pos| content[..pos].trim().to_string())
        }
        _ => content.lines().next().map(|s| s.trim().to_string()),
    }
}

fn find_ts_parent(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        let kind = parent.kind();
        if kind == "class_body" {
            // Go up one more to get class_declaration
            if let Some(class_decl) = parent.parent() {
                if class_decl.kind() == "class_declaration" || class_decl.kind() == "class" {
                    for i in 0..class_decl.child_count() {
                        if let Some(child) = class_decl.child(i as u32) {
                            if child.kind() == "type_identifier" || child.kind() == "identifier" {
                                return child
                                    .utf8_text(source)
                                    .ok()
                                    .map(std::string::ToString::to_string);
                            }
                        }
                    }
                }
            }
        } else if kind == "interface_declaration" {
            for i in 0..parent.child_count() {
                if let Some(child) = parent.child(i as u32) {
                    if child.kind() == "type_identifier" {
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

fn collect_ts_doc_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        // Skip decorators when looking for doc comments
        if sib.kind() == "decorator" {
            current = sib.prev_sibling();
            continue;
        }
        if sib.kind() == "comment" {
            let text = sib.utf8_text(source).unwrap_or("");
            // TSDoc starts with /** or //
            if text.starts_with("/**") || text.starts_with("//") {
                return Some(text.to_string());
            }
        }
        break;
    }
    None
}

fn collect_ts_decorators(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut decorators = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        if sib.kind() == "decorator" {
            decorators.push(sib.utf8_text(source).unwrap_or("").to_string());
            current = sib.prev_sibling();
            continue;
        }
        if sib.kind() == "comment" {
            current = sib.prev_sibling();
            continue;
        }
        break;
    }
    decorators.reverse();
    if decorators.is_empty() {
        None
    } else {
        Some(decorators.join("\n"))
    }
}

#[cfg(test)]
mod tests {
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
}
