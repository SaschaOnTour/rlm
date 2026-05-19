//! Tests for `application::facades`.
//!
//! Facades are thin delegating wrappers: each opens an [`RlmSession`]
//! and forwards to one session method. End-to-end coverage comes
//! through the CLI integration tests (`tests/cli_*`) and the MCP
//! tests (`tests/mcp_tests.rs`) — every tool-level test exercises a
//! facade transitively. Unit-level tests would be re-tests of the
//! session machinery one level up; we keep them on
//! [`RlmSession`](crate::application::session) instead.
