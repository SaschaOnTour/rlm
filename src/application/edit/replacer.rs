use super::validator::{validate_and_write, SyntaxGuard};
use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::error::{Result, RlmError};
use crate::ingest::scanner::ext_to_lang;

/// Look up a file and its matching chunk by symbol identifier.
///
/// Returns the resolved `Chunk` (cloned) so callers can use byte offsets, content, etc.
/// Look up a file and its matching chunk by symbol identifier, with
/// optional `--parent` disambiguation.
///
/// Resolution:
/// * `parent = None`: return the sole chunk matching `symbol`. If two
///   or more chunks share the ident, return [`RlmError::AmbiguousSymbol`]
///   with every candidate listed (parent, kind, line).
/// * `parent = Some("Foo")`: filter to chunks whose `parent` equals
///   `"Foo"`. Single match → return. Zero match → `SymbolNotFound`.
///   Multiple matches under same parent is possible in pathological
///   cases (e.g. two methods with same name in the same impl, which
///   wouldn't compile anyway) but the caller still gets
///   `AmbiguousSymbol` with the narrowed list.
pub(super) fn find_symbol_in_file(
    db: &Database,
    file_path: &str,
    symbol: &str,
    parent: Option<&str>,
) -> Result<Chunk> {
    let file = db
        .get_file_by_path(file_path)?
        .ok_or_else(|| RlmError::FileNotFound {
            path: file_path.into(),
        })?;

    let chunks = db.get_chunks_for_file(file.id)?;
    let matches: Vec<&Chunk> = chunks
        .iter()
        .filter(|c| c.ident == symbol)
        .filter(|c| match parent {
            None => true,
            Some(p) => c.parent.as_deref() == Some(p),
        })
        .collect();

    match matches.as_slice() {
        [] => Err(RlmError::SymbolNotFound {
            ident: symbol.into(),
        }),
        [only] => Ok((*only).clone()),
        many => Err(RlmError::AmbiguousSymbol(
            crate::error::AmbiguousSymbolError {
                ident: symbol.into(),
                candidates: many
                    .iter()
                    .map(|c| crate::error::SymbolCandidate {
                        parent: c.parent.clone(),
                        kind: c.kind.as_str().to_string(),
                        line: c.start_line,
                    })
                    .collect(),
            },
        )),
    }
}

/// Resolve, load, verify chunk-staleness, splice, validate and write — the
/// shared spine of `replace_symbol` / `delete_symbol`. The caller's closure
/// receives `(source, start_byte, end_byte)` and returns the post-edit file
/// content; the helper takes care of everything before (path validation,
/// staleness check) and after (Syntax Guard + atomic write). Returns the
/// resolved `Chunk` so callers can surface metadata like `old_code_len`.
// qual:allow(srp_params) reason: "db, path, ident, parent, splice, root are 6 orthogonal concerns; grouping 2 into a struct would hide call-site clarity"
fn apply_edit<F>(
    db: &Database,
    file_path: &str,
    symbol: &str,
    parent: Option<&str>,
    project_root: &std::path::Path,
    splice: F,
) -> Result<Chunk>
where
    F: FnOnce(&str, usize, usize) -> Result<String>,
{
    let full_path = crate::error::validate_relative_path(file_path, project_root)?;
    let chunk = find_symbol_in_file(db, file_path, symbol, parent)?;
    let source = std::fs::read_to_string(&full_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            RlmError::FileNotFound {
                path: file_path.into(),
            }
        } else {
            RlmError::from(e)
        }
    })?;

    let start = chunk.start_byte as usize;
    let end = chunk.end_byte as usize;
    if start > source.len() || end > source.len() {
        return Err(RlmError::EditConflict);
    }
    let actual = source.get(start..end).ok_or(RlmError::EditConflict)?;
    if actual != chunk.content {
        return Err(RlmError::EditConflict);
    }

    let modified = splice(&source, start, end)?;

    let ext = full_path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let lang = ext_to_lang(ext);
    let guard = SyntaxGuard::new();
    validate_and_write(&guard, lang, &modified, &full_path)?;
    Ok(chunk)
}

/// Result of a successful `replace_symbol` call.
#[derive(Debug)]
pub struct ReplaceOutcome {
    /// Length of the old code that was replaced (in bytes).
    pub old_code_len: usize,
}

/// Replace an AST node (function, struct, etc.) by identifier.
///
/// `file_path` is the project-relative path (as stored in the DB).
/// `project_root` is used to resolve the absolute path for disk I/O.
// qual:allow(srp_params) reason: "db, path, ident, parent, code, root are 6 orthogonal concerns"
pub fn replace_symbol(
    db: &Database,
    file_path: &str,
    symbol: &str,
    parent: Option<&str>,
    new_code: &str,
    project_root: &std::path::Path,
) -> Result<ReplaceOutcome> {
    let chunk = apply_edit(
        db,
        file_path,
        symbol,
        parent,
        project_root,
        |source, start, end| {
            let mut modified = String::with_capacity(source.len() - (end - start) + new_code.len());
            modified.push_str(source.get(..start).ok_or(RlmError::EditConflict)?);
            modified.push_str(new_code);
            modified.push_str(source.get(end..).ok_or(RlmError::EditConflict)?);
            Ok(modified)
        },
    )?;
    Ok(ReplaceOutcome {
        old_code_len: chunk.content.len(),
    })
}

/// Delete a symbol by identifier, collapsing the trailing newline so the
/// symbol's empty line does not linger. Reuses `replace_symbol`'s staleness
/// checks and Syntax Guard.
// qual:allow(srp_params) reason: "db, path, ident, parent, keep_docs, root are 6 orthogonal concerns"
pub fn delete_symbol(
    db: &Database,
    file_path: &str,
    symbol: &str,
    parent: Option<&str>,
    keep_docs: bool,
    project_root: &std::path::Path,
) -> Result<DeleteOutcome> {
    let mut sidecar: Option<(u32, u32)> = None;
    let chunk = apply_edit(
        db,
        file_path,
        symbol,
        parent,
        project_root,
        |source, start, end| {
            // Expand `start` backward over contiguous doc comments / attributes
            // unless the caller opted out with `keep_docs`.
            let effective_start = if keep_docs {
                start
            } else {
                find_sidecar_start(source, start)
            };
            if effective_start < start {
                let (l1, l2) = byte_range_to_lines(source, effective_start, start);
                sidecar = Some((l1, l2));
            }

            let end_with_nl = if source.as_bytes().get(end) == Some(&b'\n') {
                end + 1
            } else {
                end
            };
            let mut modified =
                String::with_capacity(source.len() - (end_with_nl - effective_start));
            modified.push_str(
                source
                    .get(..effective_start)
                    .ok_or(RlmError::EditConflict)?,
            );
            modified.push_str(source.get(end_with_nl..).ok_or(RlmError::EditConflict)?);
            Ok(modified)
        },
    )?;
    Ok(DeleteOutcome {
        old_code_len: chunk.content.len(),
        sidecar_lines: sidecar,
    })
}

/// Result of a successful `delete_symbol` call.
#[derive(Debug)]
pub struct DeleteOutcome {
    /// Length of the deleted code (in bytes). Measures the symbol's
    /// original byte range only — sidecar bytes removed alongside are
    /// not counted here.
    pub old_code_len: usize,
    /// If the sidecar (doc comments / attributes) above the symbol
    /// was also removed, reports the 1-based inclusive line range of
    /// that block. `None` when no sidecar existed or when
    /// `keep_docs = true` suppressed removal.
    pub sidecar_lines: Option<(u32, u32)>,
}

/// Preview a replacement without writing (returns the diff).
pub fn preview_replace(
    db: &Database,
    file_path: &str,
    symbol: &str,
    parent: Option<&str>,
    new_code: &str,
) -> Result<ReplaceDiff> {
    let chunk = find_symbol_in_file(db, file_path, symbol, parent)?;

    Ok(ReplaceDiff {
        file: file_path.to_string(),
        symbol: symbol.to_string(),
        old_code: chunk.content.clone(),
        new_code: new_code.to_string(),
        start_line: chunk.start_line,
        end_line: chunk.end_line,
    })
}

/// A diff showing what would change.
#[derive(Debug, Clone)]
pub struct ReplaceDiff {
    pub file: String,
    pub symbol: String,
    pub old_code: String,
    pub new_code: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// Serialize with backward-compatible `old_lines: [start, end]` instead of separate fields.
impl serde::Serialize for ReplaceDiff {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ReplaceDiff", 5)?;
        state.serialize_field("file", &self.file)?;
        state.serialize_field("symbol", &self.symbol)?;
        state.serialize_field("old_code", &self.old_code)?;
        state.serialize_field("new_code", &self.new_code)?;
        state.serialize_field("old_lines", &[self.start_line, self.end_line])?;
        state.end()
    }
}

#[cfg(test)]
#[path = "replacer_edge_tests.rs"]
mod edge_tests;
#[cfg(test)]
#[path = "replacer_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "delete_symbol_tests.rs"]
mod delete_tests;

/// Walk backward from `symbol_start` extending over any contiguous
/// doc-comment (`///`, `//!`) and attribute (`#[...]`) lines. Returns
/// the earliest byte offset of the sidecar block, or `symbol_start`
/// when no sidecar precedes the symbol.
///
/// A blank line between the sidecar and the symbol ends extension —
/// orphaned doc blocks separated by whitespace are treated as
/// belonging to whatever came above.
pub(super) fn find_sidecar_start(source: &str, symbol_start: usize) -> usize {
    let mut extended_start = symbol_start;
    loop {
        let Some(prev_line_start) = start_of_previous_line(source, extended_start) else {
            return extended_start;
        };
        let line = &source[prev_line_start..extended_start];
        let trimmed = line.trim_end_matches('\n').trim_start();
        if is_sidecar_line(trimmed) {
            extended_start = prev_line_start;
        } else {
            return extended_start;
        }
    }
}

fn start_of_previous_line(source: &str, pos: usize) -> Option<usize> {
    if pos == 0 {
        return None;
    }
    // `pos` is at the start of a line (chunk.start_byte convention).
    // Previous line runs from its own start up to (and including) the
    // `\n` at `pos - 1`. So we need the `\n` before that one, or 0.
    let before = &source.as_bytes()[..pos.saturating_sub(1)];
    match before.iter().rposition(|&b| b == b'\n') {
        Some(nl) => Some(nl + 1),
        None => Some(0),
    }
}

fn is_sidecar_line(trimmed: &str) -> bool {
    trimmed.starts_with("///") || trimmed.starts_with("//!") || trimmed.starts_with("#[")
}

/// Convert a `[start..end)` byte range into 1-based inclusive line
/// numbers. Used by `delete_symbol` to report which lines the sidecar
/// occupied.
fn byte_range_to_lines(source: &str, start: usize, end: usize) -> (u32, u32) {
    let line_at = |byte_pos: usize| -> u32 {
        (source[..byte_pos.min(source.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
            + 1) as u32
    };
    let l1 = line_at(start);
    let l2 = line_at(end.saturating_sub(1)).max(l1);
    (l1, l2)
}
