//! Shared helper functions for common tree-sitter parsing patterns.
//!
//! Extracted from `base.rs` for SRP compliance. These helpers are used by
//! multiple language-specific parsers (Rust, Go, Java, C#, Python, PHP, etc.).

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
// Tree-sitter node traversal helpers
// =============================================================================

/// Extract the name from a node by finding the first child whose kind is one of `kinds`.
#[must_use]
pub fn first_child_text_by_kind(
    node: tree_sitter::Node,
    source: &[u8],
    kinds: &[&str],
) -> Option<String> {
    for i in 0..node.child_count() {
        let child = node.child(i as u32)?;
        if kinds.contains(&child.kind()) {
            return child.utf8_text(source).ok().map(std::string::ToString::to_string);
        }
    }
    None
}

/// Walk up the tree-sitter tree to find a parent node matching one of `parent_kinds`,
/// then extract the identifier from its child matching `ident_kind`.
///
/// Used by C#, Java, PHP, Python, and Rust to find enclosing class/struct/impl names.
#[must_use]
pub fn find_parent_by_kind(
    node: tree_sitter::Node,
    source: &[u8],
    parent_kinds: &[&str],
    ident_kind: &str,
) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent_kinds.contains(&parent.kind()) {
            let found = (0..parent.child_count())
                .filter_map(|i| parent.child(i as u32))
                .find(|child| child.kind() == ident_kind);
            if let Some(child) = found {
                return child
                    .utf8_text(source)
                    .ok()
                    .map(std::string::ToString::to_string);
            }
        }
        current = parent.parent();
    }
    None
}

// =============================================================================
// Sibling collection helpers (doc comments, attributes)
// =============================================================================

/// Configuration bundle for sibling collection, reducing parameter count.
///
/// Groups the filtering criteria that control which tree-sitter siblings
/// are collected, skipped, or stop the walk.
pub struct SiblingCollectConfig<'a> {
    /// Node kinds to collect (e.g. `&["line_comment"]`).
    pub kinds: &'a [&'a str],
    /// Node kinds to skip over (e.g. `&["attribute_item"]`).
    pub skip_kinds: &'a [&'a str],
    /// Optional prefix strings for prefix-based filtering on collected nodes.
    /// Empty means no prefix filtering on collect.
    pub prefixes: &'a [&'a str],
    /// When `true`, accumulate all consecutive matches; when `false`, at most one.
    pub multi: bool,
}

/// Controls where prefix-based filtering is applied in [`collect_prev_siblings_core`].
enum PrefixFilter<'a> {
    /// Filter on `collect_kinds`: only collect nodes whose text matches a prefix.
    /// A matching kind that fails the prefix check **stops** the walk.
    OnCollect(&'a [&'a str]),
    /// Filter on `skip_kinds`: only skip nodes whose text matches a prefix.
    /// A skip-kind node that fails the prefix check **stops** the walk.
    OnSkip(&'a [&'a str]),
    /// No prefix filtering at all.
    None,
}

/// Action returned by [`PrefixFilter`] methods to guide the sibling walk.
enum FilterAction {
    /// Accept and collect this sibling.
    Collect,
    /// Skip this sibling and continue walking.
    Skip,
    /// Stop the walk immediately.
    Stop,
}


/// Classify a sibling node against the collect/skip/prefix rules (operation: logic only).
///
/// Returns the appropriate `FilterAction` without calling any own-module functions.
fn classify_sibling(
    kind: &str,
    text: &str,
    collect_kinds: &[&str],
    skip_kinds: &[&str],
    prefix_filter: &PrefixFilter<'_>,
) -> FilterAction {
    if collect_kinds.contains(&kind) {
        if let PrefixFilter::OnCollect(prefixes) = prefix_filter {
            if !prefixes.is_empty() && !prefixes.iter().any(|p| text.starts_with(p)) {
                return FilterAction::Stop;
            }
        }
        FilterAction::Collect
    } else if skip_kinds.contains(&kind) {
        if let PrefixFilter::OnSkip(prefixes) = prefix_filter {
            if !prefixes.iter().any(|p| text.starts_with(p)) {
                return FilterAction::Stop;
            }
        }
        FilterAction::Skip
    } else {
        FilterAction::Stop
    }
}

/// Walk previous siblings of `node`, collecting text from siblings whose
/// kind is in `config.kinds` and skipping over siblings whose kind is in
/// `config.skip_kinds`.  Any other sibling kind stops the walk.
///
/// `prefix_filter` controls optional prefix-based filtering (see [`PrefixFilter`]).
///
/// When `config.multi` is `true`, all consecutive matching siblings are
/// accumulated (e.g. consecutive `///` doc-comment lines).  When `false`,
/// at most one match is returned (e.g. a single `/** ... */` block).
///
/// Results are returned in source order (reversed from walk order).
///
/// This is an operation: it uses only `classify_sibling` (inlined logic) and
/// tree-sitter node traversal (std-like).
#[must_use]
fn collect_prev_siblings_core(
    node: tree_sitter::Node,
    source: &[u8],
    config: &SiblingCollectConfig<'_>,
    prefix_filter: &PrefixFilter<'_>,
) -> Option<String> {
    let mut items = Vec::new();
    let mut current = node.prev_sibling();
    while let Some(sib) = current {
        let kind = sib.kind();
        let text = sib.utf8_text(source).unwrap_or("");
        let action = classify_sibling(kind, text, config.kinds, config.skip_kinds, prefix_filter);
        match action {
            FilterAction::Collect => {
                items.push(text.to_string());
                if !config.multi {
                    break;
                }
            }
            FilterAction::Skip => {}
            FilterAction::Stop => break,
        }
        current = sib.prev_sibling();
    }
    items.reverse();
    if items.is_empty() {
        None
    } else {
        Some(items.join("\n"))
    }
}

/// Walk previous siblings, collecting nodes in `config.kinds` and skipping
/// nodes in `config.skip_kinds`.  When `config.prefixes` is non-empty, only
/// collected nodes whose text starts with one of the prefixes are kept; a
/// match that fails the prefix check stops the walk.
#[must_use]
pub fn collect_prev_siblings(
    node: tree_sitter::Node,
    source: &[u8],
    config: &SiblingCollectConfig<'_>,
) -> Option<String> {
    let filter = if config.prefixes.is_empty() {
        PrefixFilter::None
    } else {
        PrefixFilter::OnCollect(config.prefixes)
    };
    collect_prev_siblings_core(node, source, config, &filter)
}

/// Like [`collect_prev_siblings`] but skips nodes in `config.skip_kinds`
/// **only** when their text starts with one of `config.prefixes`.  If a node
/// matches `skip_kinds` but fails the prefix check the walk stops.
#[must_use]
pub fn collect_prev_siblings_filtered_skip(
    node: tree_sitter::Node,
    source: &[u8],
    config: &SiblingCollectConfig<'_>,
) -> Option<String> {
    collect_prev_siblings_core(
        node,
        source,
        config,
        &PrefixFilter::OnSkip(config.prefixes),
    )
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
mod tests {
    use super::*;

    #[test]
    fn test_extract_signature_to_brace() {
        assert_eq!(
            extract_signature_to_brace("fn main() { }"),
            Some("fn main()".to_string())
        );
        assert_eq!(extract_signature_to_brace("fn main()"), None);
    }

    #[test]
    fn test_extract_type_signature() {
        assert_eq!(
            extract_type_signature("pub struct Foo { }"),
            Some("pub struct Foo".to_string())
        );
        assert_eq!(
            extract_type_signature("pub struct Foo;"),
            Some("pub struct Foo;".to_string())
        );
    }

    #[test]
    fn test_extract_signature_to_colon() {
        assert_eq!(
            extract_signature_to_colon("def foo(x):\n    pass"),
            Some("def foo(x)".to_string())
        );
    }
}
