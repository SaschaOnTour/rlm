//! Reference entity — a call site, import, type use, or field access.

use serde::{Deserialize, Serialize};

use super::{ChunkId, ReferenceId};

/// What kind of reference was extracted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RefKind {
    Call,
    Import,
    TypeUse,
    FieldAccess,
}

impl RefKind {
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Call => "call",
            Self::Import => "import",
            Self::TypeUse => "type_use",
            Self::FieldAccess => "field_access",
        }
    }

    /// Parse a string produced by `as_str`. Unknown inputs fall back to
    /// `Call`, matching the legacy `models::chunk::RefKind::parse`
    /// behaviour; keeping parity here means the migration in later slices
    /// does not silently change lookup results.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "call" => Self::Call,
            "import" => Self::Import,
            "type_use" => Self::TypeUse,
            "field_access" => Self::FieldAccess,
            _ => Self::Call,
        }
    }
}

/// A reference found within a chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reference {
    pub id: ReferenceId,
    pub chunk_id: ChunkId,
    pub target_ident: String,
    pub ref_kind: RefKind,
    pub line: u32,
    pub col: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_kind_round_trip() {
        for k in [
            RefKind::Call,
            RefKind::Import,
            RefKind::TypeUse,
            RefKind::FieldAccess,
        ] {
            assert_eq!(RefKind::parse(k.as_str()), k);
        }
    }

    #[test]
    fn ref_kind_unknown_falls_back_to_call() {
        assert_eq!(RefKind::parse("totally-unknown"), RefKind::Call);
        assert_eq!(RefKind::parse(""), RefKind::Call);
    }

    #[test]
    fn reference_roundtrips_through_json() {
        let r = Reference {
            id: ReferenceId::new(3),
            chunk_id: ChunkId::new(11),
            target_ident: "do_thing".into(),
            ref_kind: RefKind::Call,
            line: 42,
            col: 8,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: Reference = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, r.id);
        assert_eq!(back.chunk_id, r.chunk_id);
        assert_eq!(back.target_ident, r.target_ident);
        assert_eq!(back.ref_kind, r.ref_kind);
    }
}
