use crate::error::{Result, RlmError};
use crate::infrastructure::filesystem::atomic_writer::write_atomic;
use crate::ingest::dispatcher::Dispatcher;

/// STRICT syntax validation. No bypass mechanism.
/// Validates modified code in RAM before allowing writes.
pub struct SyntaxGuard {
    dispatcher: Dispatcher,
}

impl SyntaxGuard {
    #[must_use]
    pub fn new() -> Self {
        Self {
            dispatcher: Dispatcher::new(),
        }
    }

    /// Validate that the given source code is syntactically valid for the language.
    /// Returns Ok(()) if valid, Err with details if invalid.
    /// Non-code languages (markdown, pdf) always pass.
    pub fn validate(&self, lang: &str, source: &str) -> Result<()> {
        if !self.dispatcher.is_code_language(lang) {
            return Ok(());
        }

        if self.dispatcher.validate_syntax(lang, source) {
            Ok(())
        } else {
            Err(RlmError::SyntaxGuard {
                detail: format!(
                    "syntax validation failed for {lang}: modified code has parse errors"
                ),
            })
        }
    }
}

/// Validate syntax then write file atomically (free function, decoupled from `SyntaxGuard`).
///
/// First validates via `guard.validate()`, then delegates the on-disk
/// replacement to the shared `infrastructure::filesystem::atomic_writer`
/// so source-edit writes use the same O_EXCL-protected path as
/// `rlm setup`.
pub fn validate_and_write(
    guard: &SyntaxGuard,
    lang: &str,
    source: &str,
    path: &std::path::Path,
) -> Result<()> {
    guard.validate(lang, source)?;
    write_atomic(path, source.as_bytes())?;
    Ok(())
}

impl Default for SyntaxGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn validate_valid_rust() {
        let guard = SyntaxGuard::new();
        assert!(guard.validate("rust", "fn main() {}").is_ok());
    }

    #[test]
    fn validate_invalid_rust_rejects() {
        let guard = SyntaxGuard::new();
        let result = guard.validate("rust", "fn main() {");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("syntax"));
    }

    #[test]
    fn validate_markdown_always_passes() {
        let guard = SyntaxGuard::new();
        assert!(guard.validate("markdown", "any content").is_ok());
    }

    #[test]
    fn validate_and_write_valid() {
        let guard = SyntaxGuard::new();
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.rs");
        validate_and_write(&guard, "rust", "fn main() {}", &path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fn main() {}");
    }

    #[test]
    fn validate_and_write_invalid_rejects() {
        let guard = SyntaxGuard::new();
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.rs");
        let result = validate_and_write(&guard, "rust", "fn main() {", &path);
        assert!(result.is_err());
        assert!(!path.exists()); // File should NOT be written
    }
}
