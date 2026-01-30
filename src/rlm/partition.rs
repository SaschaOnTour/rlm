use serde::Serialize;

use crate::db::Database;
use crate::error::{Result, RlmError};
use crate::models::token_estimate::{estimate_tokens_str, TokenEstimate};

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

/// A partition (chunk) of content.
#[derive(Debug, Clone, Serialize)]
pub struct Partition {
    /// Partition index.
    #[serde(rename = "i")]
    pub index: usize,
    /// Start line.
    #[serde(rename = "sl")]
    pub start_line: u32,
    /// End line.
    #[serde(rename = "el")]
    pub end_line: u32,
    /// Content of this partition.
    #[serde(rename = "c")]
    pub content: String,
    /// Token estimate for this partition.
    #[serde(rename = "t")]
    pub tokens: u64,
}

/// Partition result.
#[derive(Debug, Clone, Serialize)]
pub struct PartitionResult {
    #[serde(rename = "f")]
    pub file: String,
    #[serde(rename = "p")]
    pub partitions: Vec<Partition>,
    #[serde(rename = "t")]
    pub tokens: TokenEstimate,
}

/// Partition a file's content into chunks using the specified strategy.
pub fn partition_file(
    db: &Database,
    file_path: &str,
    strategy: &Strategy,
    project_root: &std::path::Path,
) -> Result<PartitionResult> {
    let full_path = project_root.join(file_path);
    let source = std::fs::read_to_string(&full_path).map_err(|_| RlmError::FileNotFound {
        path: file_path.into(),
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
        let tokens = estimate_tokens_str(&content);

        partitions.push(Partition {
            index: i,
            start_line,
            end_line,
            content,
            tokens,
        });
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
                .map(|(i, c)| Partition {
                    index: i,
                    start_line: c.start_line,
                    end_line: c.end_line,
                    content: c.content.clone(),
                    tokens: estimate_tokens_str(&c.content),
                })
                .collect());
        }
    }

    // Fallback to uniform if no chunks found
    Ok(partition_uniform(source, 50))
}

/// Keyword partitioning: filter lines by regex, then partition remaining.
fn partition_keyword(source: &str, pattern: &str) -> Result<Vec<Partition>> {
    let re =
        regex::Regex::new(pattern).map_err(|e| RlmError::Other(format!("invalid regex: {e}")))?;

    let lines: Vec<&str> = source.lines().collect();
    let mut partitions = Vec::new();
    let mut current_lines = Vec::new();
    let mut start_line = 0u32;

    for (i, line) in lines.iter().enumerate() {
        if re.is_match(line) {
            // Save accumulated non-matching lines as a partition
            if !current_lines.is_empty() {
                let content = current_lines.join("\n");
                partitions.push(Partition {
                    index: partitions.len(),
                    start_line: start_line + 1,
                    end_line: i as u32,
                    content: content.clone(),
                    tokens: estimate_tokens_str(&content),
                });
                current_lines.clear();
            }
            // Add matching line as its own partition
            let content = line.to_string();
            partitions.push(Partition {
                index: partitions.len(),
                start_line: i as u32 + 1,
                end_line: i as u32 + 1,
                content: content.clone(),
                tokens: estimate_tokens_str(&content),
            });
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
        partitions.push(Partition {
            index: partitions.len(),
            start_line: start_line + 1,
            end_line: end,
            content: content.clone(),
            tokens: estimate_tokens_str(&content),
        });
    }

    Ok(partitions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_partition() {
        let source = "line1\nline2\nline3\nline4\nline5";
        let parts = partition_uniform(source, 2);
        assert_eq!(parts.len(), 3); // 2+2+1
        assert_eq!(parts[0].start_line, 1);
        assert_eq!(parts[0].end_line, 2);
        assert_eq!(parts[2].start_line, 5);
        assert_eq!(parts[2].end_line, 5);
    }

    #[test]
    fn keyword_partition() {
        let source = "normal\n// TODO: fix\nnormal\n// TODO: another\nend";
        let parts = partition_keyword(source, "TODO").unwrap();
        // Should have partitions separating TODO lines
        assert!(parts.iter().any(|p| p.content.contains("TODO: fix")));
        assert!(parts.iter().any(|p| p.content.contains("TODO: another")));
    }

    #[test]
    fn semantic_partition_fallback() {
        let db = Database::open_in_memory().unwrap();
        let source = "line1\nline2\nline3";
        // No file in DB, should fallback to uniform
        let parts = partition_semantic(&db, "nonexistent.rs", source).unwrap();
        assert!(!parts.is_empty());
    }
}
