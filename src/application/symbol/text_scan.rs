//! Low-level byte-string scanning primitives shared by the symbol
//! analyses — bracket balancing, ident reading, ident predicates.
//!
//! Lifted out of [`super::path_match`] so each sibling module stays a
//! single responsibility: `path_match` is the Rust-path-call matcher
//! and only knows about matching; this module owns the text-scanning
//! mechanics it leans on.
//!
//! All functions are intentionally byte-indexed (not char-indexed):
//! tree-sitter's `Reference.col` is a byte offset, so callers can use
//! the same offset both in source `&str` and through these helpers
//! without re-mapping char positions.

/// Strip a balanced `open ... close` block from the end of `s`.
/// Returns `s` unchanged if it doesn't end with `close` or if the
/// brackets aren't balanced.
///
/// `open` / `close` are `u8` because this whole family scans bytes
/// (matching tree-sitter's byte-offset convention). A `char` API
/// would silently lose non-ASCII delimiters via an `as u8` truncation
/// — the byte-typed signature keeps the contract honest.
pub(super) fn strip_trailing_balanced_brackets(s: &str, open: u8, close: u8) -> &str {
    let Some(without_close) = s.strip_suffix(close as char) else {
        return s;
    };
    find_matching_open(without_close, open, close).map_or(s, |open_idx| &without_close[..open_idx])
}

/// Given `s` that conceptually ends just after a stripped `close`
/// bracket, find the byte index of the matching `open`. Returns
/// `None` when brackets aren't balanced.
pub(super) fn find_matching_open(s: &str, open: u8, close: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    // Depth starts at 1 because the caller has already stripped one
    // `close` bracket whose matching `open` we're locating.
    let mut depth: i32 = 1;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        let b = bytes[i];
        if b == close {
            depth += 1;
        } else if b == open {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// Consume a balanced `open ... close` block from the start of `s`.
/// Returns the remainder after the matching `close`, or `None` if
/// `s` doesn't start with `open` or brackets aren't balanced.
pub(super) fn consume_balanced_brackets_at_start(s: &str, open: u8, close: u8) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.first().copied() != Some(open) {
        return None;
    }
    let mut depth: i32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(&s[i + 1..]);
            }
        }
    }
    None
}

/// Read a Rust identifier starting at byte position `start` in `s`.
/// Returns `(start, end)` byte offsets that are guaranteed to lie on
/// UTF-8 char boundaries, or `None` when `start` doesn't sit on a
/// boundary or the char there isn't a valid ident-start.
///
/// Operates on `&str` (not `&[u8]`) so multi-byte idents like
/// `Föö` are scanned char-by-char — the previous byte-at-a-time
/// loop classified `0xC3` (the lead byte of `ö`) as alphabetic
/// individually, then sliced inside the codepoint and panicked.
pub(super) fn read_ident(s: &str, start: usize) -> Option<(usize, usize)> {
    let tail = s.get(start..)?;
    let mut chars = tail.char_indices();
    let (_, first) = chars.next()?;
    if !is_ident_start(first) {
        return None;
    }
    let mut end = start + first.len_utf8();
    for (_, c) in chars {
        if !is_ident_continuation(c) {
            break;
        }
        end += c.len_utf8();
    }
    Some((start, end))
}

/// True iff `s` ends with `ident` at an identifier boundary — i.e.,
/// the character preceding the suffix isn't an ident continuation
/// (so `notFoo` doesn't accidentally end with `Foo`).
pub(super) fn ends_with_ident(s: &str, ident: &str) -> bool {
    let Some(before) = s.strip_suffix(ident) else {
        return false;
    };
    !before
        .chars()
        .next_back()
        .is_some_and(is_ident_continuation)
}

/// Rust identifier-start predicate: alphabetic or underscore (digits
/// excluded — Rust idents can't start with a digit).
pub(super) fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_'
}

/// Rust identifier-continuation predicate: alphanumeric or underscore.
pub(super) fn is_ident_continuation(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
