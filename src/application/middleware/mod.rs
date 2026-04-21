//! Application-layer middleware: operation envelopes + savings recording.
//!
//! Every read-side tool in `application::{query,symbol,content}` is
//! executed through one of the `record_*` helpers below. The helpers
//! build an [`OperationMeta`], run the operation, serialise the
//! result, and record the Claude-Code-alternative cost in the
//! savings store. Adapters never call these directly — they go
//! through [`crate::application::session::RlmSession`] which wires
//! the DB handle + config into the middleware.

pub mod request;
pub mod response;
pub mod savings_recorder;

pub use request::{AlternativeCost, OperationMeta};
pub use response::OperationResponse;
pub use savings_recorder::{record_file_query, record_operation, record_symbol_query};

#[cfg(test)]
#[path = "fixtures_tests.rs"]
mod fixtures;
