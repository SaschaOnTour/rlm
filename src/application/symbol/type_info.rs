//! Result type for the symbol type-info view.
//!
//! Used by `application::query::read::render_enriched_body` when the
//! caller asks for `--metadata`. The shape is shared via this type
//! so the JSON envelope stays stable across surfaces, even though
//! the values are derived from the chunks the read actually returns
//! (priority lattice `src/` > default > `fixtures`/`test`, applied
//! within the already-selected set so a read from a test file can't
//! pick up a `src/` sibling).

use serde::Serialize;

use crate::domain::token_budget::TokenEstimate;

/// Wire-format of the `type_info` view emitted under
/// `read --metadata`.
#[derive(Debug, Clone, Serialize)]
pub struct TypeInfoResult {
    /// The symbol name.
    pub symbol: String,
    /// `Some(Type)` when the picked chunk is a method on a concrete
    /// type, `None` for free functions / module-level items. Surfaces
    /// which `Foo::ident` was selected when polysemy forced a
    /// priority decision (`src/` > default > fixtures), so the caller
    /// can tell whether the picked one is actually what they meant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The kind of the symbol (fn, struct, class, etc.).
    pub kind: String,
    /// The signature if available.
    pub signature: Option<String>,
    /// The full content of the symbol.
    pub content: String,
    /// The file path where the symbol is defined.
    pub file: String,
    /// Token estimate for this response.
    pub tokens: TokenEstimate,
}
