//! Tests and test-only helpers for `quality_log.rs`.
//!
//! Moved out of `quality_log.rs` in slice 4.9. The original file
//! carried a large block of `#[cfg(test)]`-annotated module-level
//! helpers (date arithmetic, `chrono_timestamp`, `extract_context`,
//! plus test-only `impl QualityIssue` and `impl QualityLogger`
//! blocks) in addition to the `mod tests` body. All of that now
//! lives here and is wired back in via
//! `#[cfg(test)] #[path = "quality_log_tests.rs"] mod tests;`.
//! The `#[cfg(test)]` annotations are redundant inside a file that
//! is already only compiled under that cfg, so they have been
//! stripped.

use super::super::ParseQuality;
use super::*;
use std::fs::OpenOptions;
use std::io::Write;

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

impl QualityLogger {
    /// Log an issue to the JSONL file.
    fn log(&self, issue: &QualityIssue) -> std::io::Result<()> {
        // Skip known issues unless log_all is enabled
        if issue.known && !self.log_all {
            return Ok(());
        }

        // Ensure parent directory exists
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)?;

        let json = serde_json::to_string(issue).unwrap_or_default();
        writeln!(file, "{json}")?;
        Ok(())
    }
}

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

/// Format date components as an ISO 8601 timestamp (integration: calls only).
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

use tempfile::TempDir;

/// Number of issues to log in the read_and_summarize test.
const ISSUES_TO_LOG: u32 = 3;

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

#[test]
fn logger_writes_jsonl() {
    const ERROR_LINE: u32 = 5;

    let tmp = TempDir::new().unwrap();
    let log_path = tmp.path().join("quality.log");
    let logger = QualityLogger::new(&log_path, true);

    let issue = QualityIssue {
        ts: "2026-01-28T12:00:00Z".to_string(),
        file: "test.rs".to_string(),
        lang: "rust".to_string(),
        issue: "error_node".to_string(),
        line: Some(ERROR_LINE),
        context: Some("bad syntax".to_string()),
        known: false,
        test: None,
    };

    logger.log(&issue).unwrap();

    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("test.rs"));
    assert!(content.contains("error_node"));
}

#[test]
fn read_and_summarize() {
    let tmp = TempDir::new().unwrap();
    let log_path = tmp.path().join("quality.log");
    let logger = QualityLogger::new(&log_path, true);

    // Log a few issues
    for i in 0..ISSUES_TO_LOG {
        let issue = QualityIssue {
            ts: "2026-01-28T12:00:00Z".to_string(),
            file: format!("test{}.rs", i),
            lang: "rust".to_string(),
            issue: "error_node".to_string(),
            line: Some(i + 1),
            context: None,
            known: i == 0, // First one is known
            test: if i == 0 {
                Some("test_known".to_string())
            } else {
                None
            },
        };
        logger.log(&issue).unwrap();
    }

    let issues = read_quality_log(&log_path).unwrap();
    assert_eq!(issues.len(), ISSUES_TO_LOG as usize);

    let summary = summarize_issues(&issues);
    assert_eq!(summary.total, ISSUES_TO_LOG as usize);
    assert_eq!(summary.known, 1);
    assert_eq!(summary.unknown, 2);
}

#[test]
fn annotate_known_java_record() {
    let mut issues = vec![QualityIssue {
        ts: "2026-01-28T12:00:00Z".to_string(),
        file: "User.java".to_string(),
        lang: "java".to_string(),
        issue: "error_node".to_string(),
        line: Some(5),
        context: Some("public record User(String name)".to_string()),
        known: false,
        test: None,
    }];

    annotate_known_issues(&mut issues);

    assert!(issues[0].known);
    assert_eq!(issues[0].test, Some("java_records".to_string()));
}

#[test]
fn filter_unknown_issues() {
    let issues = vec![
        QualityIssue {
            ts: "t".to_string(),
            file: "a.rs".to_string(),
            lang: "rust".to_string(),
            issue: "e".to_string(),
            line: None,
            context: None,
            known: true,
            test: None,
        },
        QualityIssue {
            ts: "t".to_string(),
            file: "b.rs".to_string(),
            lang: "rust".to_string(),
            issue: "e".to_string(),
            line: None,
            context: None,
            known: false,
            test: None,
        },
    ];

    let unknown = filter_unknown(issues);
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].file, "b.rs");
}
