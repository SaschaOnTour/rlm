//! Pure math and data types for token-savings accounting.
//!
//! DB recording, cost estimation against an index, and reporting live in
//! `crate::application::savings`. This module contains only the numbers,
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

/// CC calls needed to replicate an `extract`:
/// Read(src) + Edit(src, remove) + Read(dest) + Edit(dest, append).
/// When the destination is new a `Write` substitutes for
/// `Read(dest) + Edit(dest)` — still 4 calls worst-case, still 3 at
/// best, but the 4-call model is the safe upper bound we charge CC.
pub const CC_CALLS_EXTRACT: u64 = 4;

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
#[path = "savings_tests.rs"]
mod tests;
