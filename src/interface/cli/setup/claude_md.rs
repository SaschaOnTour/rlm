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
/// Body of the rlm-managed block in CLAUDE.local.md. Surrounded by
/// [`MARKER_BEGIN`] / [`MARKER_END`] at render time. Kept as a const
/// so the render function stays short (SRP_FN) and so tests can
/// assert individual sections without running the full render path.
const CLAUDE_MD_BODY: &str = r#"
## rlm Workflow Instructions

### Exploration (progressive disclosure)
1. `rlm overview --detail minimal` — project map (~50 tokens)
2. `rlm search <query>` — full-text across symbols + content
3. `rlm read <path> --symbol <name>` — surgical reads; add `--metadata` for signature + call-count
4. `rlm refs <symbol>` — semantic impact analysis (not a grep)
5. `rlm context <symbol> --graph` — body + callers + callees + type info in one call

### Editing (AST-based, Syntax Guard + native-compiler validated)
- `rlm replace <path> --symbol <name> --code-file /tmp/patch.rs`
- `rlm insert <path> --code-file /tmp/snippet.rs --position 'after:42'`
- `rlm delete <path> --symbol <name>` — takes docs/attrs with it by default
- `rlm extract <src> --symbols A,B,C --to <dest>` — atomic module split
- Use `--preview` on replace for non-trivial edits

### After every write: read the response
Every `replace` / `insert` / `delete` / `extract` returns a rich JSON
envelope. Read it before the next action — it replaces multiple
follow-up tool calls.

- `build.passed` — `cargo check` / `tsc` result. If `false`, fix the listed
  errors (file + line + message) **before** moving on.
- `test_impact.run_tests` — tests covering the changed symbol.
- `test_impact.test_command` — ready-to-copy shell command to run them.
- `test_impact.no_tests_warning` — fires when Direct ∪ Transitive coverage
  is empty. Write a test before shipping; naming-convention candidates
  in `run_tests` are speculative, not confirmed coverage.
- `test_impact.similar_symbols` — lexically close idents elsewhere.
  Check these for consistent parallel changes or typo catches.
- `deleted.sidecar_lines` — extra lines removed (the doc/attr block).

### Test discipline (do this automatically)
1. If `test_impact.test_command` is present → run it right after the edit.
2. If `test_impact.no_tests_warning` is present → write the missing test
   before your next change.
3. If `similar_symbols` is populated → decide whether those symbols
   need a parallel change; otherwise call it out explicitly.
4. If `build.errors` is non-empty → fix them; do not continue otherwise.

### Using rlm effectively (lessons the hard way)
- **Never run `rlm index` manually after `rlm replace/insert/delete/extract`.**
  They auto-reindex (look for `reindexed: true`). The staleness check also
  catches external edits (`Edit`, `cargo fmt`, git operations) at the next
  read automatically. Manual index calls are pure overhead.
- **Prefer `--code-file /tmp/patch.rs` over `--code-stdin` with heredoc**
  when the code contains `'` / `{` / `"`. Claude Code's shell-obfuscation
  heuristic may flag mixed heredocs for approval; a file path sidesteps
  the heuristic entirely.
- **Don't pipe JSON through `python3 -m json.tool`.** Default output is
  TOON after `rlm setup` (token-dense). Use `--format pretty` if you
  need human-readable JSON; `--format json` for minified.
- **On `AmbiguousSymbol` errors, read the candidate list in the response
  and pass `--parent <name>`.** Don't guess — the error already tells
  you which containers exist.
- **Before a write, inspect:** `rlm read --symbol X --metadata` gives
  the signature + call count. Cheaper than a wrong edit + compile-fix
  round-trip.
- **Write targets:** use `rlm replace` for named symbols, `rlm delete`
  for named symbols, `rlm extract` to move, `rlm insert` for new code.
  Avoid `Edit` / `Write` tools on indexed code unless the change isn't
  symbol-addressable (imports, module docstrings, dispatch arms).

### Concurrency
- Read-only rlm tools are parallel-friendly (`readOnlyHint=true`); the
  self-healing refresh may trigger index-DB writes to reconcile drift.
- For strict parallel read-only usage, set `RLM_SKIP_REFRESH=1`.
- `replace` / `insert` / `delete` / `extract` / `index` always run sequentially.

### Parse-quality fallback
- Inspect the `q` field; if `fallback_recommended: true`, use Claude
  Code's native `Read` / `Grep` for the affected lines.

### Self-healing Index
- rlm picks up external file changes automatically on the next tool call.
- Parser upgrades (new rlm version) auto-trigger reindex on first open.
- Set `RLM_SKIP_REFRESH=1` to bypass the check in performance-sensitive scripts.
"#;

/// Upsert the rlm-managed block in `CLAUDE.local.md`.
pub fn setup_claude_local_md(project_dir: &Path, mode: SetupMode) -> Result<SetupAction> {
    let path = project_dir.join(CLAUDE_LOCAL_MD);
    upsert_claude_local_md(&path, mode)
}

/// Update the rlm block in `CLAUDE.local.md`, creating the file if needed.
///
/// Uses `ErrorKind::NotFound` matching (not `path.exists()`) so permission /
/// I/O errors surface instead of being silently treated as "file missing".
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
    let body = format!("{MARKER_BEGIN}{CLAUDE_MD_BODY}{MARKER_END}\n");
    if eol == "\r\n" {
        body.replace('\n', "\r\n")
    } else {
        body
    }
}

#[cfg(test)]
#[path = "claude_md_edge_tests.rs"]
mod edge_tests;
#[cfg(test)]
#[path = "claude_md_tests.rs"]
mod tests;
