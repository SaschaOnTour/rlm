//! Cross-cutting request/response DTOs used by every interface adapter.

pub mod request;
pub mod response;

pub use request::{AlternativeCost, OperationMeta};
pub use response::OperationResponse;
