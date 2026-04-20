//! Quality logging for parse issues.
//!
//! Logs parse quality issues to `.rlm/quality-issues.log` in JSONL format.
//! This helps track tree-sitter grammar limitations and detect regressions.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::{Deserialize, Serialize};

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

/// Quality log writer that appends issues to a JSONL file.
pub struct QualityLogger {
    log_path: std::path::PathBuf,
    /// Only read by `log()` which is `#[cfg(test)]`.
    #[cfg_attr(not(test), allow(dead_code))]
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
#[path = "quality_log_date_tests.rs"]
mod date_tests;
#[cfg(test)]
#[path = "quality_log_tests.rs"]
mod tests;
