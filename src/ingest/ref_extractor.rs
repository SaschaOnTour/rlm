use crate::error::Result;
use crate::ingest::dispatcher::Dispatcher;
use crate::models::chunk::{Chunk, Reference};

/// High-level reference extraction that delegates to the dispatcher.
pub fn extract_references(
    dispatcher: &Dispatcher,
    lang: &str,
    source: &str,
    chunks: &[Chunk],
) -> Result<Vec<Reference>> {
    dispatcher.extract_refs(lang, source, chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_rust_refs() {
        let d = Dispatcher::new();
        let source = r#"
fn helper() -> i32 { 42 }
fn main() {
    let x = helper();
}
"#;
        let chunks = d.parse("rust", source, 1).unwrap();
        let refs = extract_references(&d, "rust", source, &chunks).unwrap();
        assert!(refs.iter().any(|r| r.target_ident == "helper"));
    }

    #[test]
    fn extract_markdown_refs_empty() {
        let d = Dispatcher::new();
        let chunks = d.parse("markdown", "# Title\nContent\n", 1).unwrap();
        let refs = extract_references(&d, "markdown", "# Title\nContent\n", &chunks).unwrap();
        assert!(refs.is_empty());
    }
}
