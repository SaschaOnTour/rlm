//! Result types for symbol signature views.
//!
//! Used by `application::query::read::render_enriched_body` when the
//! caller asks for `--metadata`. The shape is shared via these types
//! so the JSON envelope stays stable across surfaces, even though the
//! values are derived directly from the chunks the read returns
//! rather than from a separate `(symbol, parent)` lookup.

use serde::Serialize;

use crate::domain::token_budget::TokenEstimate;

/// One concrete signature for the queried symbol, tagged with its
/// parent type. Polysemic idents (e.g. `new` defined on every type
/// in the codebase) need this to disambiguate which signature
/// belongs to which `impl Type` block.
#[derive(Debug, Clone, Serialize)]
pub struct SignatureEntry {
    /// `Some(Type)` for `impl Type { fn ident }`, `None` for free
    /// functions / module-level items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The signature text, exactly as the parser captured it.
    pub signature: String,
}

/// Wire-format of the `signature` view emitted under
/// `read --metadata`.
///
/// `signatures` are chunk-scoped — they list the signatures of the
/// chunks the read returned, nothing more.
///
/// `ref_count` is **parent-wide, not selected-definition-scoped**.
/// When `--parent` is set the counter uses column-aware path-call
/// resolution to drop refs that targeted a sibling `parent::symbol`
/// (e.g. excludes `Bar::new()` when the caller asked about `Foo`).
/// But rlm cannot tell at the ref level which of multiple
/// `Foo::new` definitions a `Foo::new()` call resolves to — refs
/// carry only a target ident, and the parser does no flow analysis.
/// So when two files both define `Foo::new`, both reads return the
/// same parent-wide count. Treat `ref_count` as "calls to the
/// `parent::symbol` pair in the project", not "callers of the
/// specific definition this read returned".
#[derive(Debug, Clone, Serialize)]
pub struct SignatureResult {
    /// The symbol name.
    pub symbol: String,
    /// The signatures of the chunks the read returned. Multiple
    /// entries when the read isn't fully disambiguating (e.g. no
    /// `--parent` and the ident is polysemic in the read file).
    pub signatures: Vec<SignatureEntry>,
    /// Project-wide count of calls to the `parent::symbol` pair
    /// (parent-aware when `--parent` is set, raw `target_ident`
    /// count otherwise). Not scoped to the specific definition the
    /// read returned — see the struct-level doc for why.
    pub ref_count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}
