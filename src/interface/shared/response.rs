//! Response-side DTO shared by every interface adapter.

/// Final response from an operation pipeline.
///
/// CLI and MCP adapters consume this the same way: hand `body` to their
/// output path (where a `Formatter` may reformat it for pretty / TOON)
/// and attach `tokens_out` to their own metadata.
#[derive(Debug, Clone)]
pub struct OperationResponse {
    /// Serialized operation result as produced by the pipeline. Always
    /// raw minified JSON; adapters reformat it via `Formatter` before
    /// writing to their output sink.
    pub body: String,
    /// Estimated token count of `body` on the caller side.
    pub tokens_out: u64,
}

impl OperationResponse {
    #[must_use]
    pub fn new(body: String, tokens_out: u64) -> Self {
        Self { body, tokens_out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_preserves_body_and_tokens() {
        let r = OperationResponse::new("{\"result\":42}".into(), 5);
        assert_eq!(r.body, "{\"result\":42}");
        assert_eq!(r.tokens_out, 5);
    }

    #[test]
    fn response_is_cloneable() {
        let r = OperationResponse::new("payload".into(), 1);
        let cloned = r.clone();
        assert_eq!(cloned.body, r.body);
        assert_eq!(cloned.tokens_out, r.tokens_out);
    }
}
