//! Tests for `commands.rs`.
//!
//! Moved out of the inline `mod tests` so the `panic!`-using helpers
//! sit in a `*_tests.rs` companion (matches the Phase-4 convention
//! and the `no_panic_macros_in_production` rustqual rule).

use super::{Cli, Command, QualityCmd};
use clap::Parser;

/// `Cli` doesn't derive `Debug` (clap surface), so tests can't
/// `.unwrap()` a parse result directly. Pattern-match instead.
fn parse(argv: &[&str]) -> Cli {
    match Cli::try_parse_from(argv) {
        Ok(cli) => cli,
        Err(e) => panic!("clap parse failed for {argv:?}: {e}"),
    }
}

#[test]
fn quality_without_subcommand_parses_with_top_level_flags() {
    let cli = parse(&["rlm", "quality", "--summary"]);
    match cli.command {
        Command::Quality {
            cmd: None,
            unknown_only,
            all,
            summary,
        } => {
            assert!(!unknown_only);
            assert!(!all);
            assert!(summary);
        }
        _ => panic!("expected Quality without subcommand"),
    }
}

#[test]
fn quality_clear_parses_as_subcommand() {
    let cli = parse(&["rlm", "quality", "clear"]);
    match cli.command {
        Command::Quality {
            cmd: Some(QualityCmd::Clear),
            ..
        } => {}
        _ => panic!("expected Quality with Clear subcommand"),
    }
}

#[test]
fn quality_with_old_clear_flag_is_rejected() {
    // The previous `rlm quality --clear` surface is removed in favour
    // of the subcommand. We want a parse error so scripts that still
    // pass the old flag fail loudly instead of being silently
    // downgraded to a read-only inspect.
    let result = Cli::try_parse_from(["rlm", "quality", "--clear"]);
    assert!(result.is_err(), "old --clear flag must no longer parse");
}
