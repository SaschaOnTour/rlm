//! Language and extension mapping for file classification.
//!
//! Maps file extensions to language identifiers, checks supported extensions,
//! and detects UI context from file paths.

/// Check if a file extension is supported for indexing.
#[must_use]
pub fn is_supported_extension(ext: &str) -> bool {
    matches!(
        ext.to_lowercase().as_str(),
        "rs" | "go"
            | "java"
            | "cs"
            | "py"
            | "php"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "md"
            | "markdown"
            | "pdf"
            | "json"
            | "html"
            | "css"
            | "sh"
            | "bash"
            | "yaml"
            | "yml"
            | "toml"
            | "sql"
            // C#/.NET ecosystem
            | "xml"
            | "csproj"
            | "fsproj"
            | "vbproj"
            | "sln"
            | "props"
            | "targets"
            // Java/Kotlin ecosystem
            | "gradle"
            | "kts"
            | "properties"
            // Python/config ecosystem
            | "pyi"
            | "cfg"
            | "ini"
            // Schema/IDL
            | "proto"
            | "graphql"
            | "gql"
            // Text documentation
            | "txt"
            | "rst"
            // Infrastructure as Code
            | "tf"
            | "hcl"
    )
}

/// Map file extension to language identifier.
#[must_use]
pub fn ext_to_lang(ext: &str) -> &str {
    match ext.to_lowercase().as_str() {
        "rs" => "rust",
        "go" => "go",
        "java" => "java",
        "cs" => "csharp",
        "py" => "python",
        "php" => "php",
        "js" | "jsx" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" | "cxx" => "cpp",
        "md" | "markdown" => "markdown",
        "pdf" => "pdf",
        "json" => "json",
        "html" | "htm" => "html",
        "css" => "css",
        "sh" | "bash" => "bash",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sql" => "sql",
        // C#/.NET ecosystem (XML-based project files)
        "xml" | "csproj" | "fsproj" | "vbproj" | "props" | "targets" => "xml",
        "sln" => "plaintext",
        // Java/Kotlin ecosystem
        "gradle" => "plaintext",
        "kts" => "plaintext",
        "properties" => "plaintext",
        // Python type stubs (valid Python syntax)
        "pyi" => "python",
        // Config files
        "cfg" | "ini" => "plaintext",
        // Schema/IDL
        "proto" => "plaintext",
        "graphql" | "gql" => "plaintext",
        // Text documentation
        "txt" | "rst" => "plaintext",
        // Infrastructure as Code
        "tf" | "hcl" => "plaintext",
        _ => "unknown",
    }
}

/// Table of (matcher, context) pairs for UI context detection.
/// Each entry is checked in order; the first match wins.
const UI_CONTEXT_TABLE: &[(&[&str], &[&str], &str)] = &[
    // (contains_patterns, ends_with_patterns, context_label)
    (&["/pages/", "/app/"], &[], "page"),
    (&["/components/"], &[], "component"),
    (&["/screens/"], &[], "screen"),
    (&["/layouts/"], &[], "layout"),
    (&[], &[".tsx", ".jsx", ".vue"], "ui"),
];

/// Detect UI context from file path using a table-driven lookup.
#[must_use]
pub fn detect_ui_context(path: &str) -> Option<String> {
    let lower = path.to_lowercase();
    UI_CONTEXT_TABLE.iter().find_map(|(contains, ends, ctx)| {
        let hit =
            contains.iter().any(|p| lower.contains(p)) || ends.iter().any(|p| lower.ends_with(p));
        hit.then(|| (*ctx).into())
    })
}

#[cfg(test)]
#[path = "lang_map_tests.rs"]
mod tests;
