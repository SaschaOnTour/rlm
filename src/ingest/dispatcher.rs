use std::collections::HashMap;

use crate::error::{Result, RlmError};
use crate::ingest::code::{
    csharp::CSharpParser, css::CssParser, go::GoParser, html::HtmlParser, java::JavaParser,
    javascript::JavaScriptParser, php::PhpParser, python::PythonParser, rust::RustParser,
    typescript::TypeScriptParser, CodeParser,
};
use crate::ingest::text::{
    json_semantic::JsonSemanticParser, markdown::MarkdownParser, pdf::PdfParser,
    plaintext::PlaintextParser, toml_parser::TomlParser, yaml::YamlParser, TextParser,
};
use crate::models::chunk::{Chunk, Reference};

/// Routes files to the appropriate parser based on language.
pub struct Dispatcher {
    code_parsers: HashMap<String, Box<dyn CodeParser>>,
    text_parsers: HashMap<String, Box<dyn TextParser>>,
}

impl Dispatcher {
    #[must_use]
    pub fn new() -> Self {
        let mut code_parsers: HashMap<String, Box<dyn CodeParser>> = HashMap::new();
        code_parsers.insert("rust".into(), Box::new(RustParser::new()));
        code_parsers.insert("go".into(), Box::new(GoParser::new()));
        code_parsers.insert("java".into(), Box::new(JavaParser::new()));
        code_parsers.insert("csharp".into(), Box::new(CSharpParser::new()));
        code_parsers.insert("python".into(), Box::new(PythonParser::new()));
        code_parsers.insert("php".into(), Box::new(PhpParser::new()));
        code_parsers.insert("javascript".into(), Box::new(JavaScriptParser::new()));
        code_parsers.insert("typescript".into(), Box::new(TypeScriptParser::new()));
        // TSX uses the TSX-specific parser
        code_parsers.insert("tsx".into(), Box::new(TypeScriptParser::new_tsx()));
        code_parsers.insert("html".into(), Box::new(HtmlParser::new()));
        code_parsers.insert("css".into(), Box::new(CssParser::new()));

        let mut text_parsers: HashMap<String, Box<dyn TextParser>> = HashMap::new();
        text_parsers.insert("markdown".into(), Box::new(MarkdownParser::new()));
        text_parsers.insert("pdf".into(), Box::new(PdfParser::new()));

        // Semantic parsers for config files
        text_parsers.insert("yaml".into(), Box::new(YamlParser::new()));
        text_parsers.insert("toml".into(), Box::new(TomlParser::new()));
        text_parsers.insert("json".into(), Box::new(JsonSemanticParser::new()));

        // PlaintextParser for file types without dedicated parsers (FTS5 searchability)
        for lang in &["bash", "sql", "xml", "plaintext", "c", "cpp"] {
            text_parsers.insert((*lang).into(), Box::new(PlaintextParser::new()));
        }

        Self {
            code_parsers,
            text_parsers,
        }
    }

    /// Check if a language has a parser available.
    #[must_use]
    pub fn supports(&self, lang: &str) -> bool {
        self.code_parsers.contains_key(lang) || self.text_parsers.contains_key(lang)
    }

    /// Check if the language is a code language (AST-aware).
    #[must_use]
    pub fn is_code_language(&self, lang: &str) -> bool {
        self.code_parsers.contains_key(lang)
    }

    /// Parse source code/text and extract chunks.
    pub fn parse(&self, lang: &str, source: &str, file_id: i64) -> Result<Vec<Chunk>> {
        if let Some(parser) = self.code_parsers.get(lang) {
            parser.parse_chunks(source, file_id)
        } else if let Some(parser) = self.text_parsers.get(lang) {
            parser.parse_chunks(source, file_id)
        } else {
            Err(RlmError::UnsupportedLanguage { ext: lang.into() })
        }
    }

    /// Parse chunks and extract references in a single pass (code languages only).
    pub fn parse_and_extract(
        &self,
        lang: &str,
        source: &str,
        file_id: i64,
    ) -> Result<(Vec<Chunk>, Vec<Reference>)> {
        if let Some(parser) = self.code_parsers.get(lang) {
            parser.parse_chunks_and_refs(source, file_id)
        } else {
            Err(RlmError::UnsupportedLanguage { ext: lang.into() })
        }
    }

    /// Extract references from source code (code languages only).
    pub fn extract_refs(
        &self,
        lang: &str,
        source: &str,
        chunks: &[Chunk],
    ) -> Result<Vec<Reference>> {
        if let Some(parser) = self.code_parsers.get(lang) {
            parser.extract_refs(source, chunks)
        } else {
            // Text parsers don't have references
            Ok(Vec::new())
        }
    }

    /// Validate syntax (code languages only). Returns true for non-code languages.
    #[must_use]
    pub fn validate_syntax(&self, lang: &str, source: &str) -> bool {
        if let Some(parser) = self.code_parsers.get(lang) {
            parser.validate_syntax(source)
        } else {
            true // Non-code languages always "valid"
        }
    }

    /// Parse with quality information (code languages only).
    pub fn parse_with_quality(
        &self,
        lang: &str,
        source: &str,
        file_id: i64,
    ) -> Result<crate::ingest::code::ParseResult> {
        if let Some(parser) = self.code_parsers.get(lang) {
            parser.parse_with_quality(source, file_id)
        } else {
            Err(RlmError::UnsupportedLanguage { ext: lang.into() })
        }
    }
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatcher_supports_languages() {
        let d = Dispatcher::new();
        assert!(d.supports("rust"));
        assert!(d.supports("go"));
        assert!(d.supports("java"));
        assert!(d.supports("csharp"));
        assert!(d.supports("python"));
        assert!(d.supports("php"));
        assert!(d.supports("markdown"));
        assert!(d.supports("pdf"));
        assert!(!d.supports("haskell"));
    }

    #[test]
    fn dispatcher_parses_rust() {
        let d = Dispatcher::new();
        let chunks = d.parse("rust", "fn main() {}", 1).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn dispatcher_parses_markdown() {
        let d = Dispatcher::new();
        let chunks = d.parse("markdown", "# Hello\n\nContent\n", 1).unwrap();
        assert!(!chunks.is_empty());
    }

    #[test]
    fn dispatcher_rejects_unknown() {
        let d = Dispatcher::new();
        assert!(d.parse("brainfuck", "+++", 1).is_err());
    }

    #[test]
    fn dispatcher_validates_code() {
        let d = Dispatcher::new();
        assert!(d.validate_syntax("rust", "fn main() {}"));
        assert!(!d.validate_syntax("rust", "fn main() {"));
        assert!(d.validate_syntax("markdown", "anything"));
    }
}
