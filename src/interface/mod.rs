//! Interface adapters — translate external inputs/outputs to/from application calls.
//!
//! `shared` holds DTOs and cross-cutting concerns that both the CLI and MCP
//! adapters consume. The adapters themselves (current `cli/`, `mcp/`,
//! `setup.rs`) move under this module in later slices.

pub mod shared;
