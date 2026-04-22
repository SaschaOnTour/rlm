use std::path::PathBuf;
use std::str::FromStr;

use serde::Serialize;

use crate::application::FileQuery;
use crate::db::Database;
use crate::domain::token_budget::{estimate_tokens_str, TokenEstimate};
use crate::error::{Result, RlmError};

/// Number of lines per chunk when semantic partitioning falls back to uniform splitting
/// (i.e., when the file has no indexed AST chunks).
const SEMANTIC_FALLBACK_CHUNK_SIZE: usize = 50;

/// Partitioning strategy.
#[derive(Debug, Clone, PartialEq)]
pub enum Strategy {
    /// Fixed-size line chunks.
    Uniform(usize),
    /// Split on AST boundaries (functions, classes) for code, headings for markdown.
    Semantic,
    /// Regex-based filtering before partition.
    Keyword(String),
}

impl FromStr for Strategy {
    type Err = RlmError;

    /// Parse the partition-strategy DSL: `"semantic"`, `"uniform:N"`
    /// (N ≥ 1), or `"keyword:PATTERN"`. Everything else is an
    /// `InvalidPattern` error so adapters can forward a clean
    /// message to the user.
    fn from_str(s: &str) -> Result<Self> {
        if s == "semantic" {
            return Ok(Self::Semantic);
        }
        if let Some(rest) = s.strip_prefix("uniform:") {
            let n: usize = rest.parse().map_err(|_| RlmError::InvalidPattern {
                pattern: s.to_string(),
                reason: "uniform expects a usize after the colon (e.g. 'uniform:50')".into(),
            })?;
            if n == 0 {
                return Err(RlmError::InvalidPattern {
                    pattern: s.to_string(),
                    reason: "uniform chunk size must be >= 1".into(),
                });
            }
            return Ok(Self::Uniform(n));
        }
        if let Some(rest) = s.strip_prefix("keyword:") {
            return Ok(Self::Keyword(rest.to_string()));
        }
        Err(RlmError::InvalidPattern {
            pattern: s.to_string(),
            reason: "strategy must be one of: 'semantic', 'uniform:N', 'keyword:PATTERN'".into(),
        })
    }
}

/// A partition (chunk) of content.
#[derive(Debug, Clone, Serialize)]
pub struct Partition {
    /// Partition index.
    pub index: usize,
    /// Start line.
    pub start_line: u32,
    /// End line.
    pub end_line: u32,
    /// Content of this partition.
    pub content: String,
    /// Token estimate for this partition.
    pub tokens: u64,
}

impl Partition {
    /// Create a partition, computing the token estimate from `content`.
    fn new(index: usize, start_line: u32, end_line: u32, content: String) -> Self {
        let tokens = estimate_tokens_str(&content);
        Self {
            index,
            start_line,
            end_line,
            content,
            tokens,
        }
    }
}

/// Partition result.
#[derive(Debug, Clone, Serialize)]
pub struct PartitionResult {
    pub file: String,
    pub partitions: Vec<Partition>,
    pub tokens: TokenEstimate,
}

/// Partition a file's content into chunks using the specified strategy.
pub fn partition_file(
    db: &Database,
    file_path: &str,
    strategy: &Strategy,
    project_root: &std::path::Path,
) -> Result<PartitionResult> {
    let full_path = crate::error::validate_relative_path(file_path, project_root)?;
    let source = std::fs::read_to_string(&full_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            RlmError::FileNotFound {
                path: file_path.into(),
            }
        } else {
            RlmError::from(e)
        }
    })?;

    let partitions = match strategy {
        Strategy::Uniform(chunk_size) => partition_uniform(&source, *chunk_size),
        Strategy::Semantic => partition_semantic(db, file_path, &source)?,
        Strategy::Keyword(pattern) => partition_keyword(&source, pattern)?,
    };

    let total_out: u64 = partitions.iter().map(|p| p.tokens).sum();

    Ok(PartitionResult {
        file: file_path.to_string(),
        partitions,
        tokens: TokenEstimate::new(0, total_out),
    })
}

/// Uniform partitioning: fixed-size line chunks.
fn partition_uniform(source: &str, chunk_size: usize) -> Vec<Partition> {
    let lines: Vec<&str> = source.lines().collect();
    let mut partitions = Vec::new();

    for (i, chunk) in lines.chunks(chunk_size).enumerate() {
        let content = chunk.join("\n");
        let start_line = (i * chunk_size) as u32 + 1;
        let end_line = start_line + chunk.len() as u32 - 1;

        partitions.push(Partition::new(i, start_line, end_line, content));
    }

    partitions
}

/// Semantic partitioning: use AST boundaries from the index.
fn partition_semantic(db: &Database, file_path: &str, source: &str) -> Result<Vec<Partition>> {
    let file = db.get_file_by_path(file_path)?;

    if let Some(file) = file {
        let chunks = db.get_chunks_for_file(file.id)?;
        if !chunks.is_empty() {
            return Ok(chunks
                .iter()
                .enumerate()
                .map(|(i, c)| Partition::new(i, c.start_line, c.end_line, c.content.clone()))
                .collect());
        }
    }

    // Fallback to uniform if no chunks found
    Ok(partition_uniform(source, SEMANTIC_FALLBACK_CHUNK_SIZE))
}

/// Raw partition data before token estimation.
struct RawPartition {
    start_line: u32,
    end_line: u32,
    content: String,
}

impl RawPartition {
    fn new(start_line: u32, end_line: u32, content: String) -> Self {
        Self {
            start_line,
            end_line,
            content,
        }
    }
}

/// Split source lines by regex matches into raw partitions (operation: logic only).
///
/// Matching lines become their own partitions; non-matching lines are grouped
/// between matches.  No own-crate function calls.
fn split_by_keyword(lines: &[&str], re: &regex::Regex) -> Vec<RawPartition> {
    let mut raw = Vec::new();
    let mut current_lines: Vec<String> = Vec::new();
    let mut start_line = 0u32;

    for (i, line) in lines.iter().enumerate() {
        if re.is_match(line) {
            // Save accumulated non-matching lines
            if !current_lines.is_empty() {
                let content = current_lines.join("\n");
                raw.push(RawPartition::new(start_line + 1, i as u32, content));
                current_lines.clear();
            }
            // Matching line as its own partition
            raw.push(RawPartition::new(
                i as u32 + 1,
                i as u32 + 1,
                line.to_string(),
            ));
            start_line = i as u32 + 1;
        } else {
            if current_lines.is_empty() {
                start_line = i as u32;
            }
            current_lines.push(line.to_string());
        }
    }

    // Add remaining lines
    if !current_lines.is_empty() {
        let content = current_lines.join("\n");
        let end = start_line + current_lines.len() as u32;
        raw.push(RawPartition::new(start_line + 1, end, content));
    }

    raw
}

/// Keyword partitioning: filter lines by regex, then partition remaining (integration).
fn partition_keyword(source: &str, pattern: &str) -> Result<Vec<Partition>> {
    let re = regex::Regex::new(pattern).map_err(|e| RlmError::InvalidPattern {
        pattern: pattern.to_string(),
        reason: e.to_string(),
    })?;

    let lines: Vec<&str> = source.lines().collect();
    let raw = split_by_keyword(&lines, &re);

    let partitions = raw
        .into_iter()
        .enumerate()
        .map(|(i, r)| Partition::new(i, r.start_line, r.end_line, r.content))
        .collect();

    Ok(partitions)
}

/// `partition <path> <strategy>` as a [`FileQuery`].
///
/// Strategy and project root travel on the struct rather than as
/// execute parameters so the trait signature stays uniform across all
/// file queries.
pub struct PartitionQuery {
    pub strategy: Strategy,
    pub project_root: PathBuf,
}

impl FileQuery for PartitionQuery {
    type Output = PartitionResult;
    const COMMAND: &'static str = "partition";

    fn execute(&self, db: &Database, path: &str) -> Result<Self::Output> {
        partition_file(db, path, &self.strategy, &self.project_root)
    }
}

#[cfg(test)]
#[path = "partition_tests.rs"]
mod tests;
