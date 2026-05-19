//! Source-positional matcher for Rust qualified-path calls.
//!
//! Given a source line, a 0-indexed byte column where some `symbol`
//! ident is supposed to start, and a target `parent` type name, decide
//! whether the line carries `<parent>::<symbol>` in any of the
//! syntactic forms Rust accepts. Used by
//! [`crate::application::symbol::impact::filter_impacted_by_parent`]
//! to disambiguate multiple same-line calls (`Foo::new(); Bar::new();`)
//! and to accept fully-qualified call syntax (`<Foo as Trait>::new()`).
//!
//! Strictly string-based — no AST walk. The cost is bounded by the
//! line length. The low-level byte-scanning primitives
//! (bracket-balancing, ident reading) live in the sibling
//! [`super::text_scan`] module so this file stays focused on the
//! path-matching dispatch.

use super::text_scan::{
    consume_balanced_brackets_at_start, ends_with_ident, find_matching_open, is_ident_continuation,
    read_ident, strip_trailing_balanced_brackets,
};

/// True iff `line` carries a Rust path-call of `<parent>::<symbol>`
/// at byte column `col`. Recognises every common syntactic form the
/// language accepts for "call `symbol` qualified by type `parent`":
///
/// - bare:           `Foo::symbol`
/// - scoped bare:    `module::Foo::symbol`
/// - turbofish:      `Foo::<T>::symbol`
/// - qualified:      `<Foo>::symbol`
/// - trait-disamb.:  `<Foo as Trait>::symbol`, `<Foo<T> as Trait>::symbol`
/// - nested gen.:    `<Foo<HashMap<K, V>> as Trait>::symbol`
///
/// `col` is the 0-indexed byte offset of `symbol`'s first byte on the
/// line (tree-sitter convention from `Reference.col`).
pub(super) fn line_carries_path_call(line: &str, col: usize, parent: &str, symbol: &str) -> bool {
    let Some(after) = line.get(col..) else {
        return false;
    };
    if !after.starts_with(symbol) {
        return false;
    }
    let after_symbol = &after[symbol.len()..];
    if after_symbol
        .chars()
        .next()
        .is_some_and(is_ident_continuation)
    {
        return false;
    }

    // Rust accepts whitespace around `::` (`Foo :: new()`). Trim
    // around the separator on both sides so the form-matchers below
    // see the same canonical "<type-expr>::<ident>" shape regardless
    // of how loosely the source spaced it.
    let Some(prefix) = line
        .get(..col)
        .and_then(|p| p.trim_end().strip_suffix("::").map(str::trim_end))
    else {
        return false;
    };

    bare_path_matches_parent(prefix, parent) || qualified_path_matches_parent(prefix, parent)
}

/// Bare form: `... Foo`, `... module::Foo`, `... Foo::<T>` — i.e.,
/// the path written without `<…>::` enclosure.
fn bare_path_matches_parent(prefix: &str, parent: &str) -> bool {
    let no_turbofish = strip_trailing_balanced_brackets(prefix, b'<', b'>');
    let trimmed = no_turbofish.trim_end_matches(':');
    ends_with_ident(trimmed, parent)
}

/// Qualified form: `... <Foo>`, `... <Foo as Trait>`,
/// `... <Foo<T> as Trait>`, `... <Foo<HashMap<K, V>> as Trait>`.
fn qualified_path_matches_parent(prefix: &str, parent: &str) -> bool {
    let Some(without_close) = prefix.strip_suffix('>') else {
        return false;
    };
    let Some(open_idx) = find_matching_open(without_close, b'<', b'>') else {
        return false;
    };
    let inside = &without_close[open_idx + 1..];
    parent_is_first_ident_in_brackets(inside, parent)
}

/// Check whether `inside` (the chars inside the matching `<...>` of a
/// qualified path) names `parent` as the type being qualified.
///
/// The first type expression inside the brackets has shape
/// `ident ('::' ident)* ('<' ... '>')?`; we accept when the **last**
/// `::`-separated segment of that path equals `parent`. After the
/// type expression we tolerate end-of-brackets or ` as Trait...`.
///
/// Examples accepted for `parent = "Foo"`:
/// - `Foo`, `Foo<T>`, `outer::Foo`, `outer::Foo<T>`, `outer::nested::Foo`
/// - any of the above followed by ` as Trait` (plus optional turbofish)
fn parent_is_first_ident_in_brackets(inside: &str, parent: &str) -> bool {
    // Tolerate whitespace just inside the brackets: `< Foo as Make >`.
    let inside = inside.trim_start();
    let Some((last_ident, after_type)) = parse_first_type_expr(inside) else {
        return false;
    };
    if last_ident != parent {
        return false;
    }
    if after_type.is_empty() {
        return true;
    }
    let trimmed = after_type.trim_start();
    trimmed.starts_with("as ") || trimmed == "as"
}

/// Parse the leading `ident ('::' ident)* ('<' ... '>')?` from `s` and
/// return `(last_ident, remainder)`. The last `::`-segment of the
/// path is the type's bare name; the optional trailing `<...>` is the
/// type's own generics. Returns `None` if `s` doesn't start with an
/// identifier.
fn parse_first_type_expr(s: &str) -> Option<(&str, &str)> {
    let (mut last_id_start, mut last_id_end) = read_ident(s, 0)?;
    let mut pos = last_id_end;
    // `::` and `<` are ASCII; checking raw bytes here is safe because
    // those bytes never appear inside a multi-byte UTF-8 sequence.
    let bytes = s.as_bytes();
    while bytes.get(pos).copied() == Some(b':') && bytes.get(pos + 1).copied() == Some(b':') {
        let after_colons = pos + 2;
        let Some((id_start, id_end)) = read_ident(s, after_colons) else {
            break;
        };
        last_id_start = id_start;
        last_id_end = id_end;
        pos = id_end;
    }
    // Optional turbofish-style generics on the type: `Foo<T>`.
    if bytes.get(pos).copied() == Some(b'<') {
        match consume_balanced_brackets_at_start(&s[pos..], b'<', b'>') {
            Some(rest) => pos = s.len() - rest.len(),
            None => return None,
        }
    }
    Some((&s[last_id_start..last_id_end], &s[pos..]))
}

#[cfg(test)]
#[path = "path_match_tests.rs"]
mod tests;
