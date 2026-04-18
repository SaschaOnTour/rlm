//! Pure math and data types for token-savings accounting.
//!
//! DB recording, cost estimation against an index, and reporting live in
//! `crate::operations::savings`. This module contains only the numbers,
//! the cost model, and the record types — everything else depends on it.

use serde::Serialize;

/// Per-call overhead in tokens (tool_use block structure).
pub const CALL_OVERHEAD: u64 = 30;

/// Typical grep/read result snippet size in tokens.
pub const SNIPPET_TOKENS: u64 = 200;

/// CC calls for Grep→Read→Edit (replace).
pub const CC_CALLS_REPLACE: u64 = 3;

/// CC calls for Read→Edit (insert).
pub const CC_CALLS_INSERT: u64 = 2;

/// API pricing ratio: input tokens cost per million (microdollars).
const INPUT_COST_PER_M: u64 = 3;

/// API pricing ratio: output/call-overhead cost per million (microdollars).
const OVERHEAD_COST_PER_M: u64 = 15;

/// Line-number overhead ratio (cat -n adds ~10% tokens).
const LINE_OVERHEAD_DIVISOR: u64 = 10;

/// Multiplier to convert a ratio to a percentage.
const PERCENT: f64 = 100.0;

/// Add Claude Code's line-number overhead (`N\t` prefix, ~10%) to a base token count.
#[must_use]
pub fn with_line_overhead(base: u64) -> u64 {
    base.saturating_add(base / LINE_OVERHEAD_DIVISOR)
}

/// Savings percentage (`saved / alternative * 100`), 0 when `alternative` is 0.
#[must_use]
pub fn savings_pct(saved: u64, alternative: u64) -> f64 {
    if alternative > 0 {
        (saved as f64 / alternative as f64) * PERCENT
    } else {
        0.0
    }
}

/// Full round-trip savings record covering input tokens, output tokens,
/// and call counts for both the rlm path and the Claude Code alternative.
pub struct SavingsEntry {
    pub command: String,
    /// Tokens Claude sends as tool parameters (rlm side).
    pub rlm_input: u64,
    /// Tokens in rlm's response.
    pub rlm_output: u64,
    /// Number of rlm tool calls (always 1 today).
    pub rlm_calls: u64,
    /// Tokens Claude would send as tool parameters (CC side).
    pub alt_input: u64,
    /// Tokens in CC's tool results.
    pub alt_output: u64,
    /// Number of CC tool calls.
    pub alt_calls: u64,
    pub files_touched: u64,
}

impl SavingsEntry {
    /// Total tokens consumed on the rlm path.
    #[must_use]
    pub fn rlm_total(&self) -> u64 {
        self.rlm_input + self.rlm_output + self.rlm_calls * CALL_OVERHEAD
    }

    /// Total tokens consumed on the Claude Code path.
    #[must_use]
    pub fn alt_total(&self) -> u64 {
        self.alt_input + self.alt_output + self.alt_calls * CALL_OVERHEAD
    }

    /// Net tokens saved.
    #[must_use]
    pub fn saved(&self) -> u64 {
        self.alt_total().saturating_sub(self.rlm_total())
    }

    /// Savings as weighted cost in microdollars.
    ///
    /// Tool results become input tokens in subsequent turns, so both
    /// `alt_input`/`alt_output` use the input rate ($3/1M). Only call
    /// overhead uses the output rate ($15/1M) since Claude generates
    /// tool_use blocks as output tokens.
    // qual:api
    #[must_use]
    pub fn cost_saved_microdollars(&self) -> u64 {
        let alt_cost = self
            .alt_input
            .saturating_mul(INPUT_COST_PER_M)
            .saturating_add(self.alt_output.saturating_mul(INPUT_COST_PER_M))
            .saturating_add(
                self.alt_calls
                    .saturating_mul(CALL_OVERHEAD)
                    .saturating_mul(OVERHEAD_COST_PER_M),
            );
        let rlm_cost = self
            .rlm_input
            .saturating_mul(INPUT_COST_PER_M)
            .saturating_add(self.rlm_output.saturating_mul(INPUT_COST_PER_M))
            .saturating_add(
                self.rlm_calls
                    .saturating_mul(CALL_OVERHEAD)
                    .saturating_mul(OVERHEAD_COST_PER_M),
            );
        alt_cost.saturating_sub(rlm_cost)
    }
}

/// Aggregate savings report.
#[derive(Debug, Clone, Serialize)]
pub struct SavingsReport {
    /// Total number of operations tracked.
    pub ops: u64,
    /// Total rlm output tokens (legacy, kept for compat).
    pub output: u64,
    /// Total CC output tokens (legacy, kept for compat).
    pub alternative: u64,
    /// Output-only savings (legacy, kept for compat).
    pub saved: u64,
    /// Output-only savings percentage (legacy).
    pub pct: f64,
    /// Full rlm cost (input + output + call overhead).
    pub rlm_total: u64,
    /// Full CC cost (input + output + call overhead).
    pub alt_total: u64,
    /// Full round-trip savings.
    pub total_saved: u64,
    /// Full round-trip savings percentage.
    pub total_pct: f64,
    /// Input token savings (alt_input - rlm_input).
    pub input_saved: u64,
    /// Result token savings (alt_output - rlm_output).
    pub result_saved: u64,
    /// Call count savings (alt_calls - rlm_calls).
    pub calls_saved: u64,
    /// Breakdown by command.
    pub by_cmd: Vec<CommandSavings>,
}

/// Per-command savings breakdown.
#[derive(Debug, Clone, Serialize)]
pub struct CommandSavings {
    /// Command/feature name.
    pub cmd: String,
    /// Number of invocations.
    pub ops: u64,
    /// Total rlm output tokens.
    pub output: u64,
    /// Total CC output tokens.
    pub alternative: u64,
    /// Output-only savings.
    pub saved: u64,
    /// Output-only savings percentage.
    pub pct: f64,
    /// CC call count total.
    pub alt_calls: u64,
    /// Full rlm cost.
    pub rlm_total: u64,
    /// Full CC cost.
    pub alt_total: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_line_overhead_adds_ten_percent() {
        assert_eq!(with_line_overhead(0), 0);
        assert_eq!(with_line_overhead(10), 11);
        assert_eq!(with_line_overhead(100), 110);
        assert_eq!(with_line_overhead(1000), 1100);
    }

    #[test]
    fn with_line_overhead_truncates_on_non_multiples_of_ten() {
        // base=9 → 9 + 9/10 = 9+0 = 9; ~10% is rounded down
        assert_eq!(with_line_overhead(9), 9);
        // base=19 → 19 + 19/10 = 19+1 = 20
        assert_eq!(with_line_overhead(19), 20);
    }

    #[test]
    fn savings_pct_typical_ratios() {
        assert!((savings_pct(90, 100) - 90.0).abs() < f64::EPSILON);
        assert!((savings_pct(50, 200) - 25.0).abs() < f64::EPSILON);
        assert!((savings_pct(0, 100) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn savings_pct_zero_alternative_is_zero() {
        assert!((savings_pct(500, 0) - 0.0).abs() < f64::EPSILON);
        assert!((savings_pct(0, 0) - 0.0).abs() < f64::EPSILON);
    }

    fn sample_entry() -> SavingsEntry {
        SavingsEntry {
            command: "peek".into(),
            rlm_input: 10,
            rlm_output: 50,
            rlm_calls: 1,
            alt_input: 100,
            alt_output: 2000,
            alt_calls: 3,
            files_touched: 5,
        }
    }

    #[test]
    fn savings_entry_rlm_total() {
        // 10 + 50 + 1 * 30
        assert_eq!(sample_entry().rlm_total(), 90);
    }

    #[test]
    fn savings_entry_alt_total() {
        // 100 + 2000 + 3 * 30
        assert_eq!(sample_entry().alt_total(), 2190);
    }

    #[test]
    fn savings_entry_saved_is_difference() {
        let e = sample_entry();
        assert_eq!(e.saved(), e.alt_total() - e.rlm_total());
    }

    #[test]
    fn savings_entry_saved_saturates_when_rlm_exceeds_alt() {
        let e = SavingsEntry {
            command: "expensive".into(),
            rlm_input: 0,
            rlm_output: 5_000,
            rlm_calls: 1,
            alt_input: 0,
            alt_output: 100,
            alt_calls: 1,
            files_touched: 1,
        };
        assert_eq!(e.saved(), 0);
    }

    #[test]
    fn cost_saved_microdollars_uses_input_rate_for_tokens_and_output_rate_for_calls() {
        let e = sample_entry();
        // alt_cost:
        //   alt_input * 3 = 100 * 3 = 300
        //   alt_output * 3 = 2000 * 3 = 6000
        //   alt_calls * CALL_OVERHEAD * 15 = 3 * 30 * 15 = 1350
        //   total = 7650
        // rlm_cost:
        //   10*3 + 50*3 + 1*30*15 = 30 + 150 + 450 = 630
        // saved = 7020
        assert_eq!(e.cost_saved_microdollars(), 7020);
    }

    #[test]
    fn cost_saved_microdollars_saturates_when_rlm_is_more_expensive() {
        let e = SavingsEntry {
            command: "bad".into(),
            rlm_input: 1_000,
            rlm_output: 1_000,
            rlm_calls: 10,
            alt_input: 0,
            alt_output: 0,
            alt_calls: 0,
            files_touched: 0,
        };
        assert_eq!(e.cost_saved_microdollars(), 0);
    }
}
