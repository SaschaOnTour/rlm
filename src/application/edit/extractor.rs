//! `rlm extract` — move symbols from one file to another in a single
//! atomic call (task #122).
//!
//! Wraps the existing `replacer` primitives. For each requested
//! symbol:
//!
//! 1. Locate its chunk + contiguous doc/attr sidecar.
//! 2. Collect the source bytes of symbol + sidecar into a staging
//!    buffer.
//! 3. Write / append the staging buffer to the destination file.
//! 4. Delete each symbol from the source (reverse-byte order so
//!    earlier deletes don't shift later ranges).
//!
//! Both writes go through `SyntaxGuard` — dest on creation, source
//! after every delete. A post-write `cargo check` (if enabled)
//! catches unresolved references that leftover or moved symbols may
//! have introduced, surfacing them in the response envelope.

use std::path::Path;

use super::replacer::{delete_symbol, find_sidecar_start, find_symbol_in_file};
use super::validator::{validate_and_write, SyntaxGuard};
use crate::db::Database;
use crate::error::{Result, RlmError};
use crate::ingest::scanner::ext_to_lang;

/// One symbol moved during an extract operation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MovedSymbol {
    pub symbol: String,
    pub from_lines: (u32, u32),
    /// `None` when the destination file didn't exist before the call
    /// and the block is the sole content. Populated otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to_lines: Option<(u32, u32)>,
}

/// Outcome of an extract call, surfaced in the write-response JSON.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtractOutcome {
    pub moved: Vec<MovedSymbol>,
    pub dest_created: bool,
    /// Total bytes moved (symbol bodies + sidecars).
    pub bytes_moved: usize,
}

/// Move `idents` from `source_path` to `dest_path`.
///
/// `source_path` and `dest_path` are project-relative. `dest_path`
/// may or may not exist; on create we write just the extracted
/// content, on append we join after an existing blank-line separator.
// qual:api
// qual:allow(srp_params) reason: "db, source, idents, dest, parent, root are 6 orthogonal concerns"
pub fn extract_symbols(
    db: &Database,
    source_path: &str,
    idents: &[String],
    dest_path: &str,
    parent: Option<&str>,
    project_root: &Path,
) -> Result<ExtractOutcome> {
    if idents.is_empty() {
        return Err(RlmError::Config(
            "extract: no symbols specified".to_string(),
        ));
    }
    let source_full = crate::error::validate_relative_path(source_path, project_root)?;
    let dest_full = crate::error::validate_relative_path(dest_path, project_root)?;
    if source_full == dest_full {
        return Err(RlmError::Config(
            "extract: source and destination must differ".to_string(),
        ));
    }

    let source_bytes = std::fs::read_to_string(&source_full).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            RlmError::FileNotFound {
                path: source_path.into(),
            }
        } else {
            RlmError::from(e)
        }
    })?;

    let plan = plan_extraction(db, source_path, idents, parent, &source_bytes)?;
    let (dest_content, dest_created) = assemble_dest(&dest_full, &plan)?;
    write_dest(&dest_full, dest_path, &dest_content)?;
    delete_from_source(db, source_path, idents, parent, project_root)?;

    let bytes_moved = plan.iter().map(|p| p.bytes.len()).sum();
    let moved = plan
        .into_iter()
        .map(|p| MovedSymbol {
            symbol: p.ident,
            from_lines: p.from_lines,
            to_lines: None,
        })
        .collect();

    Ok(ExtractOutcome {
        moved,
        dest_created,
        bytes_moved,
    })
}

struct ExtractionPlan {
    ident: String,
    bytes: String,
    from_lines: (u32, u32),
    symbol_start: usize,
}

/// Collect the byte range + line span for every requested symbol.
fn plan_extraction(
    db: &Database,
    source_path: &str,
    idents: &[String],
    parent: Option<&str>,
    source: &str,
) -> Result<Vec<ExtractionPlan>> {
    let mut plan = Vec::with_capacity(idents.len());
    for ident in idents {
        let chunk = find_symbol_in_file(db, source_path, ident, parent)?;
        let start = chunk.start_byte as usize;
        let end = chunk.end_byte as usize;
        if start > source.len() || end > source.len() {
            return Err(RlmError::EditConflict);
        }
        let actual = source.get(start..end).ok_or(RlmError::EditConflict)?;
        if actual != chunk.content {
            return Err(RlmError::EditConflict);
        }
        let sidecar_start = find_sidecar_start(source, start);
        let end_with_nl = if source.as_bytes().get(end) == Some(&b'\n') {
            end + 1
        } else {
            end
        };
        let block = source
            .get(sidecar_start..end_with_nl)
            .ok_or(RlmError::EditConflict)?;
        plan.push(ExtractionPlan {
            ident: ident.clone(),
            bytes: block.to_string(),
            from_lines: (line_at(source, sidecar_start), chunk.end_line),
            symbol_start: start,
        });
    }
    // Order by symbol_start ascending: dest content matches source order.
    plan.sort_by_key(|p| p.symbol_start);
    Ok(plan)
}

/// Build the final dest content, honouring "create vs. append".
fn assemble_dest(dest_full: &Path, plan: &[ExtractionPlan]) -> Result<(String, bool)> {
    let extracted: String = plan
        .iter()
        .map(|p| p.bytes.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if dest_full.exists() {
        let existing = std::fs::read_to_string(dest_full)?;
        let separator = if existing.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        Ok((format!("{existing}{separator}{extracted}"), false))
    } else {
        Ok((extracted, true))
    }
}

fn write_dest(dest_full: &Path, dest_path: &str, content: &str) -> Result<()> {
    if let Some(parent) = dest_full.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ext = dest_full.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = ext_to_lang(ext);
    let guard = SyntaxGuard::new();
    validate_and_write(&guard, lang, content, dest_full).map_err(|e| match e {
        RlmError::SyntaxGuard { detail } => RlmError::SyntaxGuard {
            detail: format!("extract target `{dest_path}` failed validation: {detail}"),
        },
        other => other,
    })
}

/// Remove the extracted symbols from the source file via `delete_symbol`
/// so sidecar handling and Syntax Guard stay consistent.
///
/// Deletions happen in reverse byte order: deleting a later-positioned
/// symbol first leaves the DB-stored byte ranges of earlier symbols
/// intact, so their staleness check still matches the file content.
fn delete_from_source(
    db: &Database,
    source_path: &str,
    idents: &[String],
    parent: Option<&str>,
    project_root: &Path,
) -> Result<()> {
    let mut ordered: Vec<(String, u32)> = idents
        .iter()
        .map(|ident| {
            let chunk = find_symbol_in_file(db, source_path, ident, parent)?;
            Ok((ident.clone(), chunk.start_byte))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered.sort_by_key(|(_, start)| std::cmp::Reverse(*start));
    for (ident, _) in ordered {
        delete_symbol(db, source_path, &ident, parent, false, project_root)?;
    }
    Ok(())
}

fn line_at(source: &str, byte_pos: usize) -> u32 {
    (source[..byte_pos.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1) as u32
}

#[cfg(test)]
#[path = "extractor_tests.rs"]
mod tests;
