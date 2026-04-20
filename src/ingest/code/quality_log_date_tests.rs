//! Date-helper + `QualityIssue::from_parse_quality` tests for
//! `quality_log.rs`.
//!
//! Split out of `quality_log_tests.rs` to isolate the pure-date-arithmetic
//! cluster (leap-year logic, epoch → YMD, ISO8601 formatting) together with
//! the `from_parse_quality` adapter that depends on it. The remaining
//! logger / read / summarize / annotate tests stay in `quality_log_tests.rs`.

use super::super::ParseQuality;
use super::QualityIssue;

const SECONDS_PER_DAY: u64 = 86400;
const SECONDS_PER_HOUR: u64 = 3600;
const SECONDS_PER_MINUTE: u64 = 60;
const EPOCH_YEAR: i64 = 1970;
const DAYS_IN_LEAP_YEAR: i64 = 366;
const DAYS_IN_YEAR: i64 = 365;
const DAYS_IN_JAN: i64 = 31;
const DAYS_IN_FEB: i64 = 28;
const DAYS_IN_FEB_LEAP: i64 = 29;
const DAYS_IN_MAR: i64 = 31;
const DAYS_IN_APR: i64 = 30;
const DAYS_IN_MAY: i64 = 31;
const DAYS_IN_JUN: i64 = 30;
const DAYS_IN_JUL: i64 = 31;
const DAYS_IN_AUG: i64 = 31;
const DAYS_IN_SEP: i64 = 30;
const DAYS_IN_OCT: i64 = 31;
const DAYS_IN_NOV: i64 = 30;
const DAYS_IN_DEC: i64 = 31;
const LEAP_YEAR_DIVISOR: i64 = 4;
const CENTURY_DIVISOR: i64 = 100;
const QUAD_CENTURY_DIVISOR: i64 = 400;

/// Date components extracted from epoch seconds.
struct DateComponents {
    year: i64,
    month: i64,
    day: i64,
    hours: u64,
    minutes: u64,
    seconds: u64,
}

/// Check if a year is a leap year.
fn is_leap_year(year: i64) -> bool {
    (year % LEAP_YEAR_DIVISOR == 0 && year % CENTURY_DIVISOR != 0)
        || (year % QUAD_CENTURY_DIVISOR == 0)
}

/// Convert a day-of-year count to (year, remaining_days).
fn days_to_year(total_days: u64) -> (i64, i64) {
    let mut days = total_days as i64;
    let mut year = EPOCH_YEAR;
    loop {
        let dy = if is_leap_year(year) {
            DAYS_IN_LEAP_YEAR
        } else {
            DAYS_IN_YEAR
        };
        if days < dy {
            break;
        }
        days -= dy;
        year += 1;
    }
    (year, days)
}

/// Index of December (0-based) used as fallback when day count exceeds all months.
const DECEMBER_INDEX: usize = 11;

/// Convert remaining days within a year to (month, day), both 1-based.
fn days_to_month_day(mut days: i64, leap: bool) -> (i64, i64) {
    let feb = if leap { DAYS_IN_FEB_LEAP } else { DAYS_IN_FEB };
    let md = [
        DAYS_IN_JAN,
        feb,
        DAYS_IN_MAR,
        DAYS_IN_APR,
        DAYS_IN_MAY,
        DAYS_IN_JUN,
        DAYS_IN_JUL,
        DAYS_IN_AUG,
        DAYS_IN_SEP,
        DAYS_IN_OCT,
        DAYS_IN_NOV,
        DAYS_IN_DEC,
    ];
    let mi = md
        .iter()
        .scan(0i64, |a, &m| {
            *a += m;
            Some(*a)
        })
        .position(|c| days < c)
        .unwrap_or(DECEMBER_INDEX);
    days -= md[..mi].iter().sum::<i64>();
    ((mi as i64) + 1, days + 1)
}

/// Convert epoch seconds to date components.
fn epoch_secs_to_date(secs: u64) -> DateComponents {
    let days_since_epoch = secs / SECONDS_PER_DAY;
    let time_of_day = secs % SECONDS_PER_DAY;
    let hours = time_of_day / SECONDS_PER_HOUR;
    let minutes = (time_of_day % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
    let seconds = time_of_day % SECONDS_PER_MINUTE;

    let (year, remaining_days) = days_to_year(days_since_epoch);
    let (month, day) = days_to_month_day(remaining_days, is_leap_year(year));

    DateComponents {
        year,
        month,
        day,
        hours,
        minutes,
        seconds,
    }
}

/// Format date components as an ISO 8601 timestamp.
fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let d = epoch_secs_to_date(duration.as_secs());
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        d.year, d.month, d.day, d.hours, d.minutes, d.seconds
    )
}

fn extract_context(source: &str, line: u32) -> Option<String> {
    let lines: Vec<&str> = source.lines().collect();
    let idx = (line as usize).saturating_sub(1);
    lines.get(idx).map(|l| {
        let trimmed = l.trim();
        if trimmed.len() > 50 {
            format!("{}...", &trimmed[..47])
        } else {
            trimmed.to_string()
        }
    })
}

impl QualityIssue {
    /// Create a new issue from parse quality information.
    #[must_use]
    fn from_parse_quality(
        file: &str,
        lang: &str,
        quality: &ParseQuality,
        source: Option<&str>,
    ) -> Vec<Self> {
        let ts = chrono_timestamp();
        match quality {
            ParseQuality::Complete => vec![],
            ParseQuality::Partial { error_lines, .. } => {
                if error_lines.is_empty() {
                    vec![Self {
                        ts,
                        file: file.to_string(),
                        lang: lang.to_string(),
                        issue: "incomplete_parse".to_string(),
                        line: None,
                        context: None,
                        known: false,
                        test: None,
                    }]
                } else {
                    error_lines
                        .iter()
                        .map(|&line| {
                            let context = source.and_then(|s| extract_context(s, line));
                            Self {
                                ts: chrono_timestamp(),
                                file: file.to_string(),
                                lang: lang.to_string(),
                                issue: "error_node".to_string(),
                                line: Some(line),
                                context,
                                known: false,
                                test: None,
                            }
                        })
                        .collect()
                }
            }
            ParseQuality::Failed { reason } => vec![Self {
                ts,
                file: file.to_string(),
                lang: lang.to_string(),
                issue: "parse_failed".to_string(),
                line: None,
                context: Some(reason.chars().take(50).collect()),
                known: false,
                test: None,
            }],
        }
    }
}

#[test]
fn quality_issue_from_complete() {
    let issues = QualityIssue::from_parse_quality("test.rs", "rust", &ParseQuality::Complete, None);
    assert!(issues.is_empty());
}

#[test]
fn quality_issue_from_partial() {
    let quality = ParseQuality::Partial {
        error_count: 2,
        error_lines: vec![5, 10],
    };
    let source = "line 1\nline 2\nline 3\nline 4\nline 5 with error\n";
    let issues = QualityIssue::from_parse_quality("test.rs", "rust", &quality, Some(source));
    assert_eq!(issues.len(), 2);
    assert_eq!(issues[0].issue, "error_node");
    assert_eq!(issues[0].line, Some(5));
}
