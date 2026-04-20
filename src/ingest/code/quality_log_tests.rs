//! Logger / reader / annotation tests for `quality_log.rs`.
//!
//! Moved out of `quality_log.rs` in slice 4.9. The file originally
//! carried a large block of date-arithmetic helpers + the
//! `QualityIssue::from_parse_quality` adapter alongside these tests.
//! Those helpers and their direct tests now live in the sibling
//! `quality_log_date_tests.rs`; this file keeps just the tests that
//! exercise the logger, the JSONL reader, and the annotate/filter
//! helpers on the `QualityIssue` surface.

use super::{
    annotate_known_issues, filter_unknown, read_quality_log, summarize_issues, QualityIssue,
    QualityLogger,
};
use std::fs::OpenOptions;
use std::io::Write;
use tempfile::TempDir;

/// Number of issues to log in the read_and_summarize test.
const ISSUES_TO_LOG: u32 = 3;

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
