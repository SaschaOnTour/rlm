//! Interface adapters — translate external inputs/outputs to/from application calls.
//!
//! `shared` holds DTOs and cross-cutting concerns that both the CLI and MCP
//! adapters consume. `cli` currently holds only the decomposed `setup/` module
//! (slice 5.1); the broader `src/cli/` adapter migrates here in a later slice.

pub mod cli;
pub mod shared;
