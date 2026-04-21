//! Advanced parser tests for `rust.rs` (PHASE 3 onward).
//!
//! Split out of `rust_tests.rs` to keep each test companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Basic parsing tests (PHASE 1–2)
//! live in `rust_tests.rs`; this file covers modern language features
//! (PHASE 3–3b), fallback mechanism (PHASE 4), and edge cases (PHASE 5).

use super::RustParser;
use crate::domain::chunk::ChunkKind;
use crate::ingest::code::CodeParser;

/// Number of parameters to generate for the long-signature stress test.
const LONG_SIGNATURE_PARAM_COUNT: usize = 50;
/// Length of the repeated string for the very-long-line stress test.
const VERY_LONG_LINE_LENGTH: usize = 10_000;

fn parser() -> RustParser {
    RustParser::create()
}

// ============================================================
// PHASE 3: Modern Language Features
// ============================================================

/// Async functions.
#[test]
fn async_function() {
    let source = r#"
async fn fetch_data(url: &str) -> Result<String, Error> {
    let response = client.get(url).await?;
    Ok(response.text().await?)
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "fetch_data").unwrap();
    assert_eq!(f.kind, ChunkKind::Function);
    assert!(
        f.signature.as_ref().unwrap().contains("async fn"),
        "Signature should include 'async'"
    );
}

/// Generics with trait bounds.
#[test]
fn generics_with_bounds() {
    let source = r#"
fn process<T: Clone + Debug, U: Default>(items: Vec<T>, default: U) -> T
where
    T: Send + Sync,
{
    items.first().cloned().unwrap_or_else(|| panic!())
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "process").unwrap();
    assert_eq!(f.kind, ChunkKind::Function);
    assert!(
        f.signature.as_ref().unwrap().contains("<T:"),
        "Signature should include generic bounds"
    );
}

/// Const generics.
#[test]
fn const_generics() {
    let source = r#"
fn fixed_array<const N: usize>() -> [u8; N] {
    [0u8; N]
}

struct Buffer<const SIZE: usize> {
    data: [u8; SIZE],
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    let f = chunks.iter().find(|c| c.ident == "fixed_array").unwrap();
    assert_eq!(f.kind, ChunkKind::Function);

    let s = chunks.iter().find(|c| c.ident == "Buffer").unwrap();
    assert_eq!(s.kind, ChunkKind::Struct);
}

/// Closures and their captures.
#[test]
fn closures_in_function() {
    let source = r#"
fn with_closure() {
    let captured = 42;
    let closure = |x: i32| x + captured;
    let moved = move || captured;
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "with_closure").unwrap();
    assert_eq!(f.kind, ChunkKind::Function);
    // Closures are inside the function, not separate chunks
    assert!(f.content.contains("closure"));
}

/// Macro definitions.
#[test]
fn macro_rules_definition() {
    let source = r#"
macro_rules! my_vec {
    ($($x:expr),*) => {
        {
            let mut v = Vec::new();
            $(v.push($x);)*
            v
        }
    };
}
"#;
    // macro_rules! is not captured by current query, but should not cause errors
    let chunks = parser().parse_chunks(source, 1).unwrap();
    // Macros are currently not extracted as chunks
    let macro_chunk = chunks.iter().find(|c| c.ident == "my_vec");
    assert!(
        macro_chunk.is_some(),
        "macro_rules should be captured as a chunk"
    );
    assert_eq!(macro_chunk.unwrap().kind, ChunkKind::Other("macro".into()));
}

/// Derive macros on structs.
#[test]
fn derive_macros() {
    let source = r#"
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Config {
    pub name: String,
    pub value: i32,
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let s = chunks.iter().find(|c| c.ident == "Config").unwrap();
    assert_eq!(s.kind, ChunkKind::Struct);
    // Struct must be found; attribute may or may not be part of the node
    // depending on tree-sitter node boundaries
    assert!(s.content.contains("pub struct Config"));
    assert!(
        s.attributes.is_some(),
        "Should capture #[derive(...)] attribute"
    );
    assert!(s.attributes.as_ref().unwrap().contains("derive"));
}

/// Impl blocks for traits.
#[test]
fn impl_trait_for_type() {
    let source = r#"
impl Display for Config {
    fn fmt(&self, f: &mut Formatter) -> Result {
        write!(f, "{}", self.name)
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();

    // Should have impl block and method
    let impl_chunk = chunks.iter().find(|c| c.kind == ChunkKind::Impl);
    assert!(impl_chunk.is_some(), "Should have impl chunk");

    let fmt_method = chunks.iter().find(|c| c.ident == "fmt");
    assert!(fmt_method.is_some(), "Should have fmt method");
    assert_eq!(fmt_method.unwrap().kind, ChunkKind::Method);
}

// ============================================================
// PHASE 3b: Latest Language Features (Rust 1.85+)
// ============================================================

/// C-string literals (Rust 1.77+).
#[test]
fn rust_c_string_literals() {
    let source = r#"
fn with_c_strings() {
    let s = c"hello world";
    let raw = cr"raw\path";
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "with_c_strings");
    assert!(f.is_some(), "Should find function with c-string literals");
}

/// Let chains in if-let (Rust 1.88+).
#[test]
fn rust_let_chains() {
    let source = r#"
fn with_let_chains(opt: Option<i32>) -> bool {
    if let Some(x) = opt && x > 0 && x < 100 {
        true
    } else {
        false
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "with_let_chains");
    assert!(f.is_some(), "Should find function with let chains");
}

/// Type alias impl Trait bounds.
#[test]
fn rust_type_alias_impl_trait() {
    let source = r#"
type Callback = impl Fn(i32) -> i32;

fn create_callback() -> Callback {
    |x| x * 2
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    // Type alias may or may not be captured, but function should be
    let f = chunks.iter().find(|c| c.ident == "create_callback");
    assert!(
        f.is_some(),
        "Should find function returning impl Trait alias"
    );
}

/// Associated type bounds.
#[test]
fn rust_associated_type_bounds() {
    let source = r#"
trait Container {
    type Item;
}

fn process<C: Container<Item = i32>>(c: C) {
    // process container with i32 items
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "process");
    assert!(
        f.is_some(),
        "Should find function with associated type bounds"
    );
}

/// RPITIT (Return Position Impl Trait in Trait).
#[test]
fn rust_rpitit() {
    let source = r#"
trait Factory {
    fn create(&self) -> impl Clone;
}

struct MyFactory;

impl Factory for MyFactory {
    fn create(&self) -> impl Clone {
        42
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    let factory_trait = chunks.iter().find(|c| c.ident == "Factory");
    assert!(factory_trait.is_some(), "Should find trait with RPITIT");
}

// ============================================================
// PHASE 4: Fallback Mechanism Tests
// ============================================================

/// Parse with quality: clean code should be Complete.
#[test]
fn parse_with_quality_clean() {
    let source = r#"
fn valid() -> i32 {
    42
}
"#;
    let result = parser().parse_with_quality(source, 1).unwrap();
    assert!(
        result.quality.is_complete(),
        "Clean code should have Complete quality"
    );
    assert!(
        !result.quality.fallback_recommended(),
        "Clean code should not recommend fallback"
    );
}

/// Parse with quality: syntax errors should be Partial.
#[test]
fn parse_with_quality_syntax_error() {
    let source = r#"
fn broken( {
    42
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

/// Deeply nested structures.
#[test]
fn deeply_nested_impl() {
    let source = r#"
mod outer {
    mod inner {
        pub struct Deep {
            value: i32,
        }

        impl Deep {
            pub fn new() -> Self {
                Self { value: 0 }
            }

            fn helper(&self) -> i32 {
                if true {
                    if true {
                        if true {
                            self.value
                        } else { 0 }
                    } else { 0 }
                } else { 0 }
            }
        }
    }
}
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(!chunks.is_empty(), "Should parse deeply nested code");
}

/// Very long function signature.
#[test]
fn very_long_signature() {
    let long_params = (0..LONG_SIGNATURE_PARAM_COUNT)
        .map(|i| format!("param{}: Type{}", i, i))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "fn long_function({}) -> Result<(), Error> {{ Ok(()) }}",
        long_params
    );

    let chunks = parser().parse_chunks(&source, 1).unwrap();
    let f = chunks.iter().find(|c| c.ident == "long_function").unwrap();
    assert_eq!(f.kind, ChunkKind::Function);
}

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
// This is a comment
/* This is a block comment */
/// Doc comment
//! Inner doc comment
"#;
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(
        chunks.is_empty(),
        "Comment-only file should produce no code chunks"
    );
}

/// Whitespace-only file.
#[test]
fn whitespace_only_file() {
    let source = "   \n\t\n   \r\n   ";
    let chunks = parser().parse_chunks(source, 1).unwrap();
    assert!(
        chunks.is_empty(),
        "Whitespace-only file should produce no chunks"
    );
}

/// Very long line (should not crash).
#[test]
fn very_long_line() {
    let long_string = "x".repeat(VERY_LONG_LINE_LENGTH);
    let source = format!("const LONG: &str = \"{}\";", long_string);

    let chunks = parser().parse_chunks(&source, 1).unwrap();
    // Should parse without crashing; may or may not extract the const
    assert!(
        chunks.len() <= 1,
        "Should handle very long lines gracefully"
    );
}

/// Partial valid code: valid function followed by invalid.
#[test]
fn partial_valid_code() {
    let source = r#"
fn valid() -> i32 {
    42
}

fn broken( {
"#;
    // Parser should not crash
    let result = parser().parse_chunks(source, 1);
    assert!(result.is_ok(), "Should not crash on partial valid code");

    let chunks = result.unwrap();
    // May or may not extract the valid function depending on error recovery
    // But should not panic
    let _ = chunks;
}
