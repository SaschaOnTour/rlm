//! Savings-report aggregation.
//!
//! Reads the raw per-command rows from the savings table and folds
//! them into a [`SavingsReport`] — the structured payload `rlm stats
//! --savings` returns. Pure math layered on top of
//! [`crate::db::Database::get_savings_by_command`]; lives in its own
//! module so [`crate::application::savings`] stays focused on
//! recording.

use crate::db::Database;
use crate::domain::savings::{savings_pct, CommandSavings, SavingsReport, CALL_OVERHEAD};
use crate::error::Result;

/// Generate a savings report, optionally filtered by date.
///
/// Derives aggregate totals from the per-command breakdown (single DB query).
pub fn get_savings_report(db: &Database, since: Option<&str>) -> Result<SavingsReport> {
    let by_cmd_raw = db.get_savings_by_command(since)?;
    // Compute input/call savings directly from raw query data (before consuming rows).
    let input_saved: u64 = by_cmd_raw
        .iter()
        .map(|r| r.alt_input_tokens.saturating_sub(r.rlm_input_tokens))
        .sum();
    let calls_saved: u64 = by_cmd_raw
        .iter()
        .map(|r| r.alt_calls.saturating_sub(r.rlm_calls))
        .sum();
    let by_cmd: Vec<CommandSavings> = by_cmd_raw
        .into_iter()
        .map(|row| {
            let cmd_saved = row.alt_tokens.saturating_sub(row.output_tokens);
            // Full round-trip totals
            let rlm_t = row.output_tokens + row.rlm_input_tokens + row.rlm_calls * CALL_OVERHEAD;
            let alt_t = row.alt_tokens + row.alt_input_tokens + row.alt_calls * CALL_OVERHEAD;
            CommandSavings {
                cmd: row.command,
                ops: row.ops,
                output: row.output_tokens,
                alternative: row.alt_tokens,
                saved: cmd_saved,
                pct: savings_pct(cmd_saved, row.alt_tokens),
                alt_calls: row.alt_calls,
                rlm_total: rlm_t,
                alt_total: alt_t,
            }
        })
        .collect();

    let ops: u64 = by_cmd.iter().map(|c| c.ops).sum();
    let output: u64 = by_cmd.iter().map(|c| c.output).sum();
    let alternative: u64 = by_cmd.iter().map(|c| c.alternative).sum();
    let saved = alternative.saturating_sub(output);
    let rlm_total: u64 = by_cmd.iter().map(|c| c.rlm_total).sum();
    let alt_total: u64 = by_cmd.iter().map(|c| c.alt_total).sum();
    let total_saved = alt_total.saturating_sub(rlm_total);
    let result_saved = saved; // output-only savings = result savings

    Ok(SavingsReport {
        ops,
        output,
        alternative,
        saved,
        pct: savings_pct(saved, alternative),
        rlm_total,
        alt_total,
        total_saved,
        total_pct: savings_pct(total_saved, alt_total),
        input_saved,
        result_saved,
        calls_saved,
        by_cmd,
    })
}
