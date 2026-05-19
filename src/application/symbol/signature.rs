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
/// `read --metadata`. Reflects the chunks the read actually returned
/// (chunk-scoped), and a `ref_count` that is parent-aware when the
/// caller scoped by `--parent` (uses column-aware path-call
/// resolution from the impact analyser to drop refs that actually
/// targeted a sibling `parent::symbol`).
#[derive(Debug, Clone, Serialize)]
pub struct SignatureResult {
    /// The symbol name.
    pub symbol: String,
    /// The signatures of the chunks the read returned. Multiple
    /// entries when the read isn't fully disambiguating (e.g. no
    /// `--parent` and the ident is polysemic in the read file).
    pub signatures: Vec<SignatureEntry>,
    /// Count of call sites, parent-scoped when the caller passed
    /// `--parent`. See
    /// [`crate::application::symbol::impact::filter_impacted_by_parent`].
    pub ref_count: usize,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}
