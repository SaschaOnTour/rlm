//! Supported extensions operations shared between CLI and MCP.
//!
//! Provides consistent behavior for listing all supported file extensions.

use serde::Serialize;

use crate::ingest::scanner::{ext_to_lang, is_supported_extension};

/// Result of listing supported extensions.
#[derive(Debug, Clone, Serialize)]
pub struct SupportedResult {
    /// The list of supported extensions.
    pub extensions: Vec<ExtensionInfo>,
}

/// Information about a supported extension.
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionInfo {
    /// The file extension (e.g., ".rs").
    pub ext: String,
    /// The language name (e.g., "rust").
    pub lang: String,
    /// The parser type (tree-sitter, structural, semantic, plaintext).
    pub parser: String,
}

/// All known extensions to check.
const KNOWN_EXTENSIONS: &[&str] = &[
    "rs",
    "go",
    "java",
    "cs",
    "py",
    "php",
    "js",
    "ts",
    "tsx",
    "jsx",
    "c",
    "cpp",
    "h",
    "hpp",
    "md",
    "markdown",
    "pdf",
    "json",
    "html",
    "css",
    "sh",
    "bash",
    "yaml",
    "yml",
    "toml",
    "sql",
    "xml",
    "csproj",
    "fsproj",
    "vbproj",
    "sln",
    "props",
    "targets",
    "gradle",
    "kts",
    "properties",
    "pyi",
    "cfg",
    "ini",
    "proto",
    "graphql",
    "gql",
    "txt",
    "rst",
    "tf",
    "hcl",
];

/// Get the parser type for a language.
fn parser_for_lang(lang: &str) -> &'static str {
    match lang {
        "rust" | "go" | "java" | "csharp" | "python" | "php" | "javascript" | "typescript"
        | "tsx" | "html" | "css" => "tree-sitter",
        "markdown" | "pdf" => "structural",
        "yaml" | "toml" | "json" => "semantic",
        _ => "plaintext",
    }
}

/// List all supported file extensions with their language and parser type.
#[must_use]
pub fn list_supported() -> SupportedResult {
    let mut infos: Vec<ExtensionInfo> = KNOWN_EXTENSIONS
        .iter()
        .filter(|e| is_supported_extension(e))
        .map(|e| {
            let lang = ext_to_lang(e);
            ExtensionInfo {
                ext: format!(".{e}"),
                lang: lang.to_string(),
                parser: parser_for_lang(lang).to_string(),
            }
        })
        .collect();

    // Sort by extension
    infos.sort_by(|a, b| a.ext.cmp(&b.ext));

    SupportedResult { extensions: infos }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_supported_not_empty() {
        let result = list_supported();
        assert!(!result.extensions.is_empty());
    }

    #[test]
    fn list_supported_includes_rust() {
        let result = list_supported();
        let rust = result.extensions.iter().find(|e| e.ext == ".rs");
        assert!(rust.is_some());
        let rust = rust.unwrap();
        assert_eq!(rust.lang, "rust");
        assert_eq!(rust.parser, "tree-sitter");
    }

    #[test]
    fn list_supported_sorted() {
        let result = list_supported();
        let exts: Vec<&str> = result.extensions.iter().map(|e| e.ext.as_str()).collect();
        let mut sorted = exts.clone();
        sorted.sort();
        assert_eq!(exts, sorted);
    }
}
