//! Typed identifiers for domain entities.
//!
//! Wrapping raw `i64` row IDs as distinct newtypes prevents accidentally
//! mixing kinds (e.g. passing a `FileId` where a `ChunkId` is expected) and
//! documents at call sites what an identifier refers to.

use serde::{Deserialize, Serialize};

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy,
            PartialEq, Eq, Hash, PartialOrd, Ord,
            Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl $name {
            #[must_use]
            pub const fn new(value: i64) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn value(self) -> i64 {
                self.0
            }

            /// Sentinel for "not yet persisted". Matches the convention used
            /// by the legacy `models::*` structs where `id: 0` flags a
            /// transient row that has not been inserted yet.
            pub const UNPERSISTED: Self = Self(0);

            #[must_use]
            pub const fn is_persisted(self) -> bool {
                self.0 != 0
            }
        }

        impl From<i64> for $name {
            fn from(value: i64) -> Self {
                Self(value)
            }
        }

        impl From<$name> for i64 {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(
    /// Row identifier for a persisted `Chunk`.
    ChunkId
);

define_id!(
    /// Row identifier for a persisted `File`.
    FileId
);

define_id!(
    /// Row identifier for a persisted `Reference`.
    ReferenceId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_id_round_trip() {
        let id = ChunkId::new(42);
        assert_eq!(id.value(), 42);
        assert_eq!(i64::from(id), 42);
        assert_eq!(ChunkId::from(42_i64), id);
    }

    #[test]
    fn unpersisted_sentinel() {
        assert_eq!(ChunkId::UNPERSISTED.value(), 0);
        assert!(!ChunkId::UNPERSISTED.is_persisted());
        assert!(ChunkId::new(1).is_persisted());
    }

    #[test]
    fn ids_do_not_mix_in_type_system() {
        // Compile-time property: distinct newtypes cannot be assigned to each
        // other. This test just asserts they coexist with identical bit
        // patterns but different types; the real guarantee is at the type
        // level.
        let c = ChunkId::new(7);
        let f = FileId::new(7);
        let r = ReferenceId::new(7);
        assert_eq!(c.value(), f.value());
        assert_eq!(f.value(), r.value());
    }

    #[test]
    fn ids_serialize_transparently() {
        let id = ChunkId::new(99);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "99");

        let parsed: ChunkId = serde_json::from_str("99").unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn ids_display_as_plain_integer() {
        assert_eq!(format!("{}", ChunkId::new(13)), "13");
        assert_eq!(format!("{}", FileId::new(5)), "5");
        assert_eq!(format!("{}", ReferenceId::new(0)), "0");
    }
}
