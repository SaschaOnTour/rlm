//! Request-side DTOs shared by every interface adapter.

/// Metadata about an operation independent of its payload.
///
/// Both CLI and MCP adapters populate this alongside the operation-specific
/// arguments so the pipeline's savings middleware can record the work
/// without needing to know which operation ran.
#[derive(Debug, Clone)]
pub struct OperationMeta {
    /// Identifier recorded in the savings table (e.g. `"search"`, `"refs"`).
    /// `&'static str` avoids per-call allocation and lets adapters share
    /// command names through named constants across CLI and MCP without
    /// copying. Not a typo guard — any string literal compiles; for
    /// compile-time validation use an enum instead.
    pub command: &'static str,
    /// Number of distinct source files the operation consults when the
    /// savings middleware needs a caller-supplied file count.
    ///
    /// This value is consumed for variants whose alternative-cost estimate
    /// depends on the adapter's notion of how many files were involved
    /// (for example `SymbolFiles`, `Fixed`, and `AtLeastBody`).
    ///
    /// For some `AlternativeCost` variants the middleware derives the count
    /// itself instead of using this field: `SingleFile` always counts as `1`
    /// and `ScopedFiles` is computed from the scoped file stats.
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
    /// Same as `Fixed` but clamps the recorded alternative cost **up**
    /// to the actual body token count if the body turns out larger than
    /// `base`. Matches the `base.max(out_tokens)` safeguard used by
    /// operations whose native-tool estimate (e.g. `search.tokens.output`)
    /// approximates the result size — prevents negative recorded savings
    /// when the actual JSON payload exceeds the up-front estimate.
    AtLeastBody { base: u64 },
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
        let at_least = AlternativeCost::AtLeastBody { base: 42 };

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
        match at_least {
            AlternativeCost::AtLeastBody { base } => assert_eq!(base, 42),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
