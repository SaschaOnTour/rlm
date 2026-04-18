//! Tree-sitter query execution helpers.
//!
//! `iter_matches` wraps the `StreamingIterator`-flavoured `QueryCursor`
//! iteration so callers don't have to repeat the
//! `QueryCursor::new()` + `cursor.matches(...)` + `while let Some(m) =
//! matches.next()` boilerplate. Used by every per-language parser
//! during chunk and reference extraction.

use tree_sitter::{Node, Query, QueryCursor, QueryMatch, StreamingIterator};

/// Run `query` against `root` with `source`, invoking `handle` for
/// every [`QueryMatch`]. Returns after all matches have been consumed.
///
/// Tree-sitter 0.26 exposes `QueryMatches` as a `StreamingIterator`,
/// so callers have to drive it with `while let Some(m) = it.next()`.
/// This helper centralises that pattern and keeps the cursor's
/// lifetime local.
pub fn iter_matches<'tree, F>(
    query: &'tree Query,
    root: Node<'tree>,
    source: &'tree [u8],
    mut handle: F,
) where
    F: FnMut(&QueryMatch<'_, 'tree>),
{
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source);
    while let Some(m) = matches.next() {
        handle(m);
    }
}
