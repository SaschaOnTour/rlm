//! Serialisable DTOs for the wire format.
//!
//! The domain entities in `crate::domain` are pure — they don't know
//! about JSON, TOON, or any other serialisation. When an adapter needs
//! to emit a domain entity as part of its response (currently only the
//! `read --symbol` path in the CLI), it converts the entity into the
//! matching DTO here.
//!
//! The DTOs mirror the domain fields exactly so the wire format is
//! unchanged from the pre-refactor shape. Introducing the split lets
//! us strip `Serialize` from the domain types (slice 6.5) without
//! breaking the public output contract.

pub mod chunk_dto;

pub use chunk_dto::{ChunkDto, ChunkKindDto, RefKindDto, ReferenceDto};
