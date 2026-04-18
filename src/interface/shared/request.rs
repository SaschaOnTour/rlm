//! Request-side DTOs shared by every interface adapter.

/// Metadata about an operation independent of its payload.
///
/// Both CLI and MCP adapters populate this alongside the operation-specific
/// arguments so the pipeline's savings middleware can record the work
/// without needing to know which operation ran.
#[derive(Debug, Clone)]
pub struct OperationMeta {
    /// Identifier recorded in the savings table (e.g. `"search"`, `"refs"`).
    /// `&'static str` so typos become compile errors when operations are
    /// referenced through named constants.
    pub command: &'static str,
    /// Number of distinct source files the operation consults. Used for
    /// the "how many Reads would CC need" side of the savings calculation.
    pub files_touched: u64,
    /// How to estimate what Claude Code's native tools would have cost.
    pub alternative: AlternativeCost,
}

/// What Claude Code would have needed to do to compute the same result.
///
/// The savings middleware translates this into an estimated token count
/// for the alternative path (e.g. single Read vs Grep+Read×N).
#[derive(Debug, Clone)]
pub enum AlternativeCost {
    /// A single `Read(path)`.
    SingleFile { path: String },
    /// A `Grep(symbol)` followed by `Read` for each involved file.
    SymbolFiles { symbol: String },
    /// A `Glob(prefix)` followed by `Read` for every file underneath.
    /// `None` prefix means the whole project.
    ScopedFiles { prefix: Option<String> },
    /// Operation doesn't map cleanly to any model above; supply a
    /// precomputed token estimate directly.
    Fixed(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_holds_all_fields() {
        let m = OperationMeta {
            command: "search",
            files_touched: 3,
            alternative: AlternativeCost::Fixed(1_000),
        };
        assert_eq!(m.command, "search");
        assert_eq!(m.files_touched, 3);
        assert!(matches!(m.alternative, AlternativeCost::Fixed(1_000)));
    }

    #[test]
    fn alternative_cost_variants_carry_expected_payload() {
        let single = AlternativeCost::SingleFile {
            path: "src/main.rs".into(),
        };
        let symbol = AlternativeCost::SymbolFiles {
            symbol: "foo".into(),
        };
        let scoped = AlternativeCost::ScopedFiles {
            prefix: Some("src/".into()),
        };
        let whole_project = AlternativeCost::ScopedFiles { prefix: None };

        match single {
            AlternativeCost::SingleFile { path } => assert_eq!(path, "src/main.rs"),
            other => panic!("unexpected variant: {other:?}"),
        }
        match symbol {
            AlternativeCost::SymbolFiles { symbol } => assert_eq!(symbol, "foo"),
            other => panic!("unexpected variant: {other:?}"),
        }
        match scoped {
            AlternativeCost::ScopedFiles { prefix } => assert_eq!(prefix.as_deref(), Some("src/")),
            other => panic!("unexpected variant: {other:?}"),
        }
        match whole_project {
            AlternativeCost::ScopedFiles { prefix } => assert!(prefix.is_none()),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
