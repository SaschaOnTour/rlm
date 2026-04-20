//! Tests for `savings.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "savings_tests.rs"] mod tests;`.

use super::{savings_pct, with_line_overhead, SavingsEntry};
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
