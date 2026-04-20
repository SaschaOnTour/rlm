//! Content-level helpers for tree-sitter-based parsers.
//!
//! The tree-cursor helpers (`find_parent_by_kind`,
//! `first_child_text_by_kind`, `SiblingCollectConfig`,
//! `collect_prev_siblings*`) moved to
//! `crate::infrastructure::parsing::tree_walker` in slice 4.2 and are
//! re-exported below so existing call sites keep working. Pure
//! string-extraction helpers (signatures, visibility) stay here.

// Re-exports from the shared tree-walker infrastructure.
pub use crate::infrastructure::parsing::tree_walker::{
    collect_prev_siblings, collect_prev_siblings_filtered_skip, find_parent_by_kind,
    first_child_text_by_kind, SiblingCollectConfig,
};

// =============================================================================
// Signature extraction helpers
// =============================================================================

/// Extract type signature (first line or up to brace).
#[must_use]
pub fn extract_type_signature(content: &str) -> Option<String> {
    if let Some(brace_pos) = content.find('{') {
        let sig = content[..brace_pos].trim();
        // Remove trailing where clauses if too long
        let sig = if let Some(where_pos) = sig.find("\nwhere") {
            sig[..where_pos].trim()
        } else {
            sig
        };
        Some(sig.to_string())
    } else if let Some(semi_pos) = content.find(';') {
        // Unit struct: `pub struct Foo;`
        Some(content[..=semi_pos].trim().to_string())
    } else {
        // Fallback: first line
        content.lines().next().map(|s| s.trim().to_string())
    }
}

/// Extract type signature up to the given `delimiter`, or fall back to the first line.
///
/// Used by C#, Go, Java, and PHP (delimiter `'{'`) and Python (delimiter `':'`).
#[must_use]
pub fn extract_type_signature_to(content: &str, delimiter: char) -> Option<String> {
    if let Some(pos) = content.find(delimiter) {
        let sig = content[..pos].trim();
        Some(sig.to_string())
    } else {
        content.lines().next().map(|s| s.trim().to_string())
    }
}

/// Convenience wrapper: extract type signature up to `{`.
#[must_use]
pub fn extract_type_signature_to_brace(content: &str) -> Option<String> {
    extract_type_signature_to(content, '{')
}

/// Convenience wrapper: extract type signature up to `:`.
#[must_use]
pub fn extract_type_signature_to_colon(content: &str) -> Option<String> {
    extract_type_signature_to(content, ':')
}

// =============================================================================
// Visibility extraction helpers
// =============================================================================

/// Extract keyword-based visibility from content.
///
/// Scans for common visibility keywords at the start of the content.
/// `default_visibility` is returned when no keyword matches (language-dependent).
/// `extra_keywords` allows adding language-specific keywords like `"internal"` for C#.
#[must_use]
pub fn extract_keyword_visibility(
    content: &str,
    default_visibility: &str,
    extra_keywords: &[(&str, &str)],
) -> Option<String> {
    let trimmed = content.trim_start();
    // Check extra keywords first (they may be more specific, e.g. "pub(crate)" before "pub")
    for &(keyword, value) in extra_keywords {
        if trimmed.starts_with(keyword) {
            return Some(value.into());
        }
    }
    if trimmed.starts_with("public") {
        Some("public".into())
    } else if trimmed.starts_with("protected") {
        Some("protected".into())
    } else if trimmed.starts_with("private") {
        Some("private".into())
    } else {
        Some(default_visibility.into())
    }
}

// =============================================================================
// Test-only helpers
// =============================================================================

#[cfg(test)]
/// Extract signature up to the opening brace.
#[must_use]
pub fn extract_signature_to_brace(content: &str) -> Option<String> {
    content
        .find('{')
        .map(|pos| content[..pos].trim().to_string())
}

#[cfg(test)]
/// Extract Python-style signature (up to colon).
#[must_use]
pub fn extract_signature_to_colon(content: &str) -> Option<String> {
    content
        .find(':')
        .map(|pos| content[..pos].trim().to_string())
}

#[cfg(test)]
#[path = "helpers_tests.rs"]
mod tests;
