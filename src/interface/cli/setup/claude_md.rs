//! `CLAUDE.local.md` marker-block upsert for `rlm setup`.
//!
//! Preserves user content outside `<!-- rlm:begin -->` / `<!-- rlm:end -->`
//! markers and rewrites the block in place on repeat runs. Self-healing:
//! if a user deletes the end marker, the next run treats `begin..EOF` as
//! the corrupt block and replaces it cleanly.

use std::path::Path;

use crate::error::Result;
use crate::infrastructure::filesystem::atomic_writer::write_atomic;

use super::orchestrator::{SetupAction, SetupMode};

/// Per-project instructions file that `rlm setup` augments.
const CLAUDE_LOCAL_MD: &str = "CLAUDE.local.md";

/// Delimiter marking the start of the rlm-managed block in `CLAUDE.local.md`.
const MARKER_BEGIN: &str = "<!-- rlm:begin -->";
/// Delimiter marking the end of the rlm-managed block in `CLAUDE.local.md`.
const MARKER_END: &str = "<!-- rlm:end -->";

/// Upsert the rlm-managed block in `CLAUDE.local.md`.
pub fn setup_claude_local_md(project_dir: &Path, mode: SetupMode) -> Result<SetupAction> {
    let path = project_dir.join(CLAUDE_LOCAL_MD);
    upsert_claude_local_md(&path, mode)
}

/// Update the rlm block in `CLAUDE.local.md`, creating the file if needed.
///
/// Uses `ErrorKind::NotFound` matching (not `path.exists()`) so permission /
/// I/O errors surface instead of being silently treated as "file missing".
// qual:allow(iosp) reason: "integration: read existing file → build new content → write"
fn upsert_claude_local_md(path: &Path, mode: SetupMode) -> Result<SetupAction> {
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let has_block = existing.contains(MARKER_BEGIN);

    match mode {
        SetupMode::Remove => {
            if !path.exists() {
                return Ok(SetupAction::NotPresent);
            }
            if !has_block {
                return Ok(SetupAction::Skipped);
            }
            let cleaned = remove_marker_block(&existing);
            if cleaned == existing {
                return Ok(SetupAction::Skipped);
            }
            write_text_atomic(path, &cleaned)?;
            Ok(SetupAction::Removed)
        }
        SetupMode::Check => {
            let next = build_updated_markdown(&existing);
            Ok(classify_markdown_action(
                &existing,
                &next,
                path.exists(),
                mode,
            ))
        }
        SetupMode::Apply => {
            let next = build_updated_markdown(&existing);
            let action = classify_markdown_action(&existing, &next, path.exists(), mode);
            if !matches!(action, SetupAction::Skipped) {
                write_text_atomic(path, &next)?;
            }
            Ok(action)
        }
    }
}

/// Return the full file content with the rlm block replaced or appended.
#[must_use]
fn build_updated_markdown(existing: &str) -> String {
    let eol = detect_eol(existing);
    let new_block = render_claude_local_md_section(eol);
    let mut out = match (existing.find(MARKER_BEGIN), existing.find(MARKER_END)) {
        (Some(start), Some(end)) if start < end => {
            // Well-formed block: replace content between markers.
            let end_of_marker = end + MARKER_END.len();
            let mut s = String::with_capacity(existing.len() + new_block.len());
            s.push_str(&existing[..start]);
            s.push_str(&new_block);
            s.push_str(&existing[end_of_marker..]);
            s
        }
        (Some(start), _) => {
            // Corrupt block: begin marker present without a matching end
            // (or markers in the wrong order). Treat everything from the
            // begin marker to EOF as the corrupt block and replace it.
            // This keeps repeat runs self-healing — a manually broken file
            // gets cleanly restored instead of accumulating duplicate blocks.
            let mut s = String::with_capacity(start + new_block.len());
            s.push_str(&existing[..start]);
            s.push_str(&new_block);
            s
        }
        (None, _) => append_block(existing, &new_block, eol),
    };
    normalize_trailing_newline(&mut out, eol);
    out
}

/// Pick the EOL style to use for any new content we write.
///
/// Returns `"\r\n"` as soon as `existing` contains a CRLF (a single
/// hit is a reliable Windows-authored-file signal in practice) and
/// `"\n"` otherwise — including for empty files, which is the sane
/// default for rlm's own rendered block.
fn detect_eol(existing: &str) -> &'static str {
    if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Append `new_block` to `existing`, separated by a blank line if `existing` is non-empty.
fn append_block(existing: &str, new_block: &str, eol: &str) -> String {
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    if !out.is_empty() {
        out.push_str(eol);
    }
    out.push_str(new_block);
    out
}

/// Ensure the string ends with exactly one trailing newline sequence.
///
/// Required for idempotency — slicing between markers can leave double
/// newlines of either EOL style. Trims `\r\n\r\n` (CRLF files) and `\n\n`
/// (LF files) alike, so Windows-authored `CLAUDE.local.md` stays clean
/// across repeat runs. When the input is missing a trailing newline, we
/// append one using `eol` so a CRLF file with a no-EOL footer doesn't
/// end up with a stray bare `\n` at EOF.
fn normalize_trailing_newline(s: &mut String, eol: &str) {
    loop {
        if s.ends_with("\r\n\r\n") {
            s.truncate(s.len() - 2);
        } else if s.ends_with("\n\n") {
            s.pop();
        } else {
            break;
        }
    }
    if !s.is_empty() && !s.ends_with('\n') {
        s.push_str(eol);
    }
}

/// Remove the marker block (and any leading blank line that precedes it).
///
/// Handles three cases:
/// - No begin marker → return unchanged.
/// - Well-formed block (begin before end) → remove begin..=end inclusive.
/// - Corrupt block (begin without end or wrong order) → remove begin..EOF
///   so `--remove` always restores a clean file even if the user hand-edited
///   the block into an inconsistent state.
#[must_use]
fn remove_marker_block(existing: &str) -> String {
    let Some(start) = existing.find(MARKER_BEGIN) else {
        return existing.to_string();
    };
    let cut_end = match existing.find(MARKER_END) {
        Some(end) if end > start => end + MARKER_END.len(),
        _ => existing.len(), // corrupt block: drop from begin to EOF
    };

    // Trim up to two preceding line endings so removal doesn't leave a blank gap.
    // Byte-level to handle both LF and CRLF — iterating chars would stop at `\r`
    // and leave a stray byte in Windows-authored files.
    let mut cut_start = start;
    let mut trimmed = 0;
    while trimmed < 2 {
        let prefix = &existing[..cut_start];
        if prefix.ends_with("\r\n") {
            cut_start -= 2;
        } else if prefix.ends_with('\n') {
            cut_start -= 1;
        } else {
            break;
        }
        trimmed += 1;
    }

    let mut out = String::with_capacity(existing.len());
    out.push_str(&existing[..cut_start]);
    out.push_str(&existing[cut_end..]);
    out
}

fn classify_markdown_action(
    existing: &str,
    next: &str,
    file_existed: bool,
    mode: SetupMode,
) -> SetupAction {
    if !file_existed {
        return match mode {
            SetupMode::Check => SetupAction::WouldCreate,
            _ => SetupAction::Created,
        };
    }
    if existing == next {
        return SetupAction::Skipped;
    }
    match mode {
        SetupMode::Check => SetupAction::WouldUpdate,
        _ => SetupAction::Updated,
    }
}

fn write_text_atomic(path: &Path, content: &str) -> Result<()> {
    write_atomic(path, content.as_bytes())?;
    Ok(())
}

/// The rlm-managed block, marker-wrapped, rendered with `eol` as the
/// line separator so it matches the surrounding file's EOL style
/// (CRLF on Windows-authored `CLAUDE.local.md`, LF everywhere else).
#[must_use]
fn render_claude_local_md_section(eol: &str) -> String {
    let body = format!(
        "{MARKER_BEGIN}
## rlm Workflow Instructions

### Exploration (progressive disclosure)
1. `rlm overview --detail minimal` — project map (~50 tokens)
2. `rlm search <query>` — full-text across symbols + content
3. `rlm read <path> --symbol <name>` — surgical reads

### Editing (AST-based, Syntax Guard-validated)
- `rlm replace <path> --symbol <name> --code '...'`
- `rlm insert <path> --code '...' --position 'after:42'`
- Use `--preview` for non-trivial edits

### Concurrency
- Read-only rlm tools are parallel-friendly (`readOnlyHint=true`), but the
  self-healing refresh may trigger index-DB writes to reconcile drift.
- For strict parallel read-only usage, set `RLM_SKIP_REFRESH=1`.
- `replace` / `insert` / `index` always run sequentially.

### Quality Check
- Inspect the `q` field; if `fallback_recommended: true`, fall back to native Read/Grep for affected lines.

### Self-healing Index
- rlm picks up external file changes automatically on the next tool call.
- Set `RLM_SKIP_REFRESH=1` to bypass the check in performance-sensitive scripts.
{MARKER_END}
"
    );
    if eol == "\r\n" {
        body.replace('\n', "\r\n")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_block_contains_markers() {
        let section = render_claude_local_md_section("\n");
        assert!(section.starts_with(MARKER_BEGIN));
        assert!(section.trim_end().ends_with(MARKER_END));
        assert!(section.contains("rlm Workflow Instructions"));
    }

    #[test]
    fn render_block_uses_requested_eol() {
        let lf = render_claude_local_md_section("\n");
        assert!(!lf.contains('\r'), "LF render must not contain \\r");

        let crlf = render_claude_local_md_section("\r\n");
        // Every LF in the body is paired with a preceding CR.
        let bytes = crlf.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'\n' {
                assert!(i > 0 && bytes[i - 1] == b'\r', "orphaned \\n at byte {i}");
            }
        }
    }

    #[test]
    fn build_markdown_preserves_crlf_in_windows_authored_file() {
        // Regression: the rendered rlm block used to be hardcoded LF,
        // so a CRLF-authored CLAUDE.local.md ended up with mixed
        // \r\n\n line endings and a noisy Windows diff. Detecting the
        // EOL from `existing` keeps the output all-CRLF.
        let existing =
            "# Project\r\n\r\n<!-- rlm:begin -->\r\nOLD\r\n<!-- rlm:end -->\r\n\r\n## Footer\r\n";
        let out = build_updated_markdown(existing);
        let bytes = out.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'\n' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\r',
                    "orphaned \\n at byte {i} in output: {out:?}"
                );
            }
        }
        assert!(out.contains("## Footer"));
        assert!(out.contains("rlm Workflow Instructions"));
    }

    #[test]
    fn build_markdown_crlf_append_when_no_markers() {
        // Same EOL concern, append-to-EOF path.
        let existing = "# Project\r\nLine 1\r\n";
        let out = build_updated_markdown(existing);
        let bytes = out.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'\n' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\r',
                    "orphaned \\n at byte {i} in output: {out:?}"
                );
            }
        }
    }

    #[test]
    fn build_markdown_appends_to_empty() {
        let out = build_updated_markdown("");
        assert!(out.contains(MARKER_BEGIN));
        assert!(out.contains(MARKER_END));
    }

    #[test]
    fn build_markdown_preserves_content_outside_markers() {
        let existing = "# Project\nSome notes.\n\n<!-- rlm:begin -->\nOLD BLOCK\n<!-- rlm:end -->\n\n## Footer\n";
        let out = build_updated_markdown(existing);
        assert!(out.starts_with("# Project"));
        assert!(out.contains("## Footer"));
        assert!(!out.contains("OLD BLOCK"));
        assert!(out.contains("rlm Workflow Instructions"));
    }

    #[test]
    fn build_markdown_appends_when_no_markers_present() {
        let existing = "# Project\nLine 1\n";
        let out = build_updated_markdown(existing);
        assert!(out.starts_with("# Project"));
        assert!(out.contains(MARKER_BEGIN));
        assert!(out.contains(MARKER_END));
    }

    #[test]
    fn remove_marker_block_strips_block() {
        let existing = "# A\n\n<!-- rlm:begin -->\nstuff\n<!-- rlm:end -->\n\n# B\n";
        let out = remove_marker_block(existing);
        assert!(!out.contains("stuff"));
        assert!(out.contains("# A"));
        assert!(out.contains("# B"));
    }

    #[test]
    fn remove_marker_block_noop_when_absent() {
        let existing = "# A\nNo rlm block here.\n";
        let out = remove_marker_block(existing);
        assert_eq!(out, existing);
    }

    #[test]
    fn build_markdown_heals_begin_without_end() {
        // Corrupt: a user hand-edited the file and deleted the end marker.
        // Instead of appending a second block, we treat begin..EOF as the
        // broken block and replace it cleanly.
        let existing = "# Project\n\n<!-- rlm:begin -->\ngarbage without end marker\n";
        let out = build_updated_markdown(existing);
        assert!(out.starts_with("# Project"));
        assert_eq!(
            out.matches(MARKER_BEGIN).count(),
            1,
            "corrupt block must be replaced, not duplicated"
        );
        assert_eq!(out.matches(MARKER_END).count(), 1);
        assert!(!out.contains("garbage without end marker"));
    }

    #[test]
    fn remove_marker_block_heals_begin_without_end() {
        // --remove on a file with corrupt block should drop begin..EOF so
        // future runs are self-healing.
        let existing = "# A\n\n<!-- rlm:begin -->\nbroken content\nno end marker ever\n";
        let out = remove_marker_block(existing);
        assert!(!out.contains(MARKER_BEGIN));
        assert!(!out.contains("broken content"));
        assert!(out.contains("# A"));
    }

    #[test]
    fn normalize_trailing_newline_handles_crlf() {
        // Regression: CRLF files ending in `\r\n\r\n` must collapse to a
        // single `\r\n`, just like LF files collapse `\n\n` to `\n`.
        let mut crlf = String::from("body\r\n\r\n");
        normalize_trailing_newline(&mut crlf, "\r\n");
        assert_eq!(crlf, "body\r\n");

        let mut crlf_multi = String::from("body\r\n\r\n\r\n");
        normalize_trailing_newline(&mut crlf_multi, "\r\n");
        assert_eq!(crlf_multi, "body\r\n");

        let mut lf = String::from("body\n\n");
        normalize_trailing_newline(&mut lf, "\n");
        assert_eq!(lf, "body\n");

        let mut already_clean_lf = String::from("body\n");
        normalize_trailing_newline(&mut already_clean_lf, "\n");
        assert_eq!(already_clean_lf, "body\n");

        let mut already_clean_crlf = String::from("body\r\n");
        normalize_trailing_newline(&mut already_clean_crlf, "\r\n");
        assert_eq!(already_clean_crlf, "body\r\n");

        let mut no_newline = String::from("body");
        normalize_trailing_newline(&mut no_newline, "\n");
        assert_eq!(no_newline, "body\n");
    }

    #[test]
    fn normalize_uses_detected_eol_on_no_newline_input() {
        // Regression: a CRLF-authored file whose footer lacked a final
        // EOL used to get a lone `\n` tacked on by normalize_trailing_newline,
        // producing mixed line endings at EOF. The detected EOL now
        // propagates so we append `\r\n` instead.
        let mut crlf_no_eol = String::from("body");
        normalize_trailing_newline(&mut crlf_no_eol, "\r\n");
        assert_eq!(crlf_no_eol, "body\r\n");
    }

    #[test]
    fn build_markdown_preserves_crlf_when_footer_has_no_final_eol() {
        // End-to-end version of the regression above: Windows-authored
        // CLAUDE.local.md whose last line (a footer after the rlm
        // block) has no trailing newline must still produce an
        // all-CRLF output after build_updated_markdown.
        let existing =
            "# Project\r\n\r\n<!-- rlm:begin -->\r\nOLD\r\n<!-- rlm:end -->\r\n\r\n## Footer";
        let out = build_updated_markdown(existing);
        let bytes = out.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'\n' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\r',
                    "orphaned \\n at byte {i} in output: {out:?}"
                );
            }
        }
        assert!(out.ends_with("\r\n"), "output should end with CRLF");
    }

    #[test]
    fn remove_marker_block_handles_crlf_line_endings() {
        // Regression: Windows-authored files with CRLF must not leave a
        // stray `\r` after trimming preceding newlines.
        let existing =
            "# Project\r\n\r\n<!-- rlm:begin -->\r\nbody\r\n<!-- rlm:end -->\r\n\r\n# After\r\n";
        let out = remove_marker_block(existing);
        assert_eq!(
            out, "# Project\r\n\r\n# After\r\n",
            "CRLF trim must not leave stray \\r; got {out:?}"
        );
        // Extra sanity: no `\r` that isn't followed by `\n`.
        let bytes = out.as_bytes();
        for i in 0..bytes.len().saturating_sub(1) {
            if bytes[i] == b'\r' {
                assert_eq!(
                    bytes[i + 1],
                    b'\n',
                    "orphaned \\r at byte {i} in output: {out:?}"
                );
            }
        }
    }

    #[test]
    fn remove_marker_block_handles_wrong_order() {
        // End before begin: also treat as corrupt, strip from the first marker.
        // (Unlikely in practice, but exercise the branch.)
        let existing = "# A\n<!-- rlm:end -->\nfoo\n<!-- rlm:begin -->\n";
        let out = remove_marker_block(existing);
        // The first-found begin is at the later position; we trim from there.
        assert!(out.contains("# A"));
        // End marker that was BEFORE begin stays because it's before our cut point.
        assert!(out.contains(MARKER_END));
        assert!(!out.contains(MARKER_BEGIN));
    }
}
