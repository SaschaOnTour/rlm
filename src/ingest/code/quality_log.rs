//! Quality logging for parse issues.
//!
//! Logs parse quality issues to `.rlm/quality-issues.log` in JSONL format.
//! This helps track tree-sitter grammar limitations and detect regressions.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ParseQuality;

/// A single quality issue entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityIssue {
    /// Timestamp in ISO 8601 format.
    pub ts: String,
    /// Relative file path.
    pub file: String,
    /// Language identifier.
    pub lang: String,
    /// Type of issue: "`error_node`", "`incomplete_parse`", "`fallback_recommended`".
    pub issue: String,
    /// Line number where the issue was detected (1-based).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Context around the error (~50 chars).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    /// Whether this is a known/expected issue.
    #[serde(default)]
    pub known: bool,
    /// Name of the #[ignore] test that covers this issue (if known).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
}

impl QualityIssue {
    /// Create a new issue from parse quality information.
    #[must_use]
    pub fn from_parse_quality(
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

/// Quality log writer that appends issues to a JSONL file.
pub struct QualityLogger {
    log_path: std::path::PathBuf,
    log_all: bool,
}

impl QualityLogger {
    /// Create a new logger for the given log path.
    pub fn new(log_path: impl Into<std::path::PathBuf>, log_all: bool) -> Self {
        Self {
            log_path: log_path.into(),
            log_all,
        }
    }

    /// Log an issue to the JSONL file.
    pub fn log(&self, issue: &QualityIssue) -> std::io::Result<()> {
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

    /// Log multiple issues.
    pub fn log_all_issues(&self, issues: &[QualityIssue]) -> std::io::Result<()> {
        for issue in issues {
            self.log(issue)?;
        }
        Ok(())
    }

    /// Clear the log file.
    pub fn clear(&self) -> std::io::Result<()> {
        if self.log_path.exists() {
            std::fs::remove_file(&self.log_path)?;
        }
        Ok(())
    }
}

/// Read and parse quality issues from a log file.
pub fn read_quality_log(path: &Path) -> std::io::Result<Vec<QualityIssue>> {
    if !path.exists() {
        return Ok(vec![]);
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut issues = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(issue) = serde_json::from_str::<QualityIssue>(&line) {
            issues.push(issue);
        }
    }

    Ok(issues)
}

/// Filter issues to only unknown ones.
#[must_use]
pub fn filter_unknown(issues: Vec<QualityIssue>) -> Vec<QualityIssue> {
    issues.into_iter().filter(|i| !i.known).collect()
}

/// Get summary statistics from issues.
#[must_use]
pub fn summarize_issues(issues: &[QualityIssue]) -> IssueSummary {
    let total = issues.len();
    let unknown = issues.iter().filter(|i| !i.known).count();
    let known = total - unknown;

    let mut by_lang: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut by_type: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for issue in issues {
        *by_lang.entry(issue.lang.clone()).or_default() += 1;
        *by_type.entry(issue.issue.clone()).or_default() += 1;
    }

    IssueSummary {
        total,
        unknown,
        known,
        by_lang,
        by_type,
    }
}

/// Summary statistics for quality issues.
#[derive(Debug, Clone, Serialize)]
pub struct IssueSummary {
    pub total: usize,
    pub unknown: usize,
    pub known: usize,
    pub by_lang: std::collections::HashMap<String, usize>,
    pub by_type: std::collections::HashMap<String, usize>,
}

/// Generate an ISO 8601 timestamp.
fn chrono_timestamp() -> String {
    // Simple timestamp without chrono dependency
    use std::time::SystemTime;
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Convert to basic ISO format (not perfect but good enough)
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Approximate year/month/day calculation
    let mut days = days_since_epoch as i64;
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let months_days: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for &m_days in &months_days {
        if days < m_days {
            break;
        }
        days -= m_days;
        month += 1;
    }
    let day = days + 1;

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Extract ~50 characters of context around a line.
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

/// Known issues registry.
/// These patterns match issues that are expected due to tree-sitter grammar limitations.
pub static KNOWN_ISSUES: &[KnownIssuePattern] = &[
    KnownIssuePattern {
        lang: "java",
        patterns: &["record ", "sealed ", "permits "],
        test_name: "java_records",
        reason: "Java records not supported by tree-sitter-java 0.23",
    },
    KnownIssuePattern {
        lang: "java",
        patterns: &["switch (", "case ", "->", "yield "],
        test_name: "java_pattern_switch",
        reason: "Java pattern switch not supported",
    },
    KnownIssuePattern {
        lang: "csharp",
        patterns: &["record ", "record struct"],
        test_name: "csharp_records",
        reason: "C# records not supported",
    },
    KnownIssuePattern {
        lang: "csharp",
        patterns: &["class ", "struct "],
        test_name: "csharp_primary_constructors",
        reason: "C# 12 primary constructors not supported",
    },
    KnownIssuePattern {
        lang: "php",
        patterns: &["enum "],
        test_name: "php_enums",
        reason: "PHP 8.1 enums have limited support",
    },
];

/// A pattern that matches known issues.
pub struct KnownIssuePattern {
    pub lang: &'static str,
    pub patterns: &'static [&'static str],
    pub test_name: &'static str,
    pub reason: &'static str,
}

impl KnownIssuePattern {
    /// Check if this pattern matches the given issue context.
    #[must_use]
    pub fn matches(&self, lang: &str, context: Option<&str>) -> bool {
        if self.lang != lang {
            return false;
        }
        let ctx = match context {
            Some(c) => c,
            None => return false,
        };
        self.patterns.iter().any(|p| ctx.contains(p))
    }
}

/// Check if an issue is known and annotate it.
pub fn annotate_known_issues(issues: &mut [QualityIssue]) {
    for issue in issues {
        for pattern in KNOWN_ISSUES {
            if pattern.matches(&issue.lang, issue.context.as_deref()) {
                issue.known = true;
                issue.test = Some(pattern.test_name.to_string());
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn quality_issue_from_complete() {
        let issues =
            QualityIssue::from_parse_quality("test.rs", "rust", &ParseQuality::Complete, None);
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
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("quality.log");
        let logger = QualityLogger::new(&log_path, true);

        let issue = QualityIssue {
            ts: "2026-01-28T12:00:00Z".to_string(),
            file: "test.rs".to_string(),
            lang: "rust".to_string(),
            issue: "error_node".to_string(),
            line: Some(5),
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
        for i in 0..3 {
            let issue = QualityIssue {
                ts: "2026-01-28T12:00:00Z".to_string(),
                file: format!("test{}.rs", i),
                lang: "rust".to_string(),
                issue: "error_node".to_string(),
                line: Some(i as u32 + 1),
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
        assert_eq!(issues.len(), 3);

        let summary = summarize_issues(&issues);
        assert_eq!(summary.total, 3);
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
}
