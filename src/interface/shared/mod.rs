//! Cross-cutting request/response DTOs used by every interface adapter.

pub mod request;
pub mod response;
pub mod savings_middleware;

pub use request::{AlternativeCost, OperationMeta};
pub use response::OperationResponse;
pub use savings_middleware::record_operation;
