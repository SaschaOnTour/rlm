//! `rlm setup` — automate Claude Code integration.
//!
//! Handles three concerns for the project under `project_dir`:
//! 1. `.claude/settings.json` — add rlm permissions + `mcpServers.rlm` entry,
//!    preserving any existing user config via array-merge with dedup.
//! 2. `CLAUDE.local.md` — insert a delimited rlm workflow block between
//!    `<!-- rlm:begin -->` / `<!-- rlm:end -->` markers. Preserves content
//!    outside the markers; re-running updates the block in place.
//! 3. Initial index — run `rlm index` if `.rlm/index.db` is missing.
//!
//! No PostToolUse hook is installed: the self-healing index
//! (`crate::indexer::staleness`) picks up external edits at each tool call.

// qual:allow(srp_module) reason: "cohesive 'setup' domain — settings.json + CLAUDE.local.md + initial index orchestration all serve one user-facing command. Splitting would fragment the domain artificially."

use std::path::Path;

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::config::Config;
use crate::error::{Result, RlmError};

/// Directory under `project_dir` holding Claude Code settings.
const CLAUDE_DIR: &str = ".claude";
/// Settings file that `rlm setup` creates/updates (team-shared, committed).
const SETTINGS_FILE: &str = "settings.json";
/// Per-project instructions file that `rlm setup` augments.
const CLAUDE_LOCAL_MD: &str = "CLAUDE.local.md";

/// Delimiter marking the start of the rlm-managed block in `CLAUDE.local.md`.
const MARKER_BEGIN: &str = "<!-- rlm:begin -->";
/// Delimiter marking the end of the rlm-managed block in `CLAUDE.local.md`.
const MARKER_END: &str = "<!-- rlm:end -->";

/// Which operation `rlm setup` should perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupMode {
    /// Apply the rlm configuration, creating or updating as needed.
    Apply,
    /// Dry-run: report what would change, write nothing to disk.
    Check,
    /// Remove all rlm entries from settings and the CLAUDE.local.md block.
    Remove,
}

/// Outcome of a single setup step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupAction {
    /// File or entry did not exist; we created it.
    Created,
    /// File or entry existed; we merged/updated it.
    Updated,
    /// Already in the desired state; no change.
    Skipped,
    /// Entry existed and was removed (only valid in `Remove` mode).
    Removed,
    /// `Check` mode: would create.
    WouldCreate,
    /// `Check` mode: would update.
    WouldUpdate,
    /// `Check` mode: would remove.
    WouldRemove,
    /// Entry was not present to begin with (e.g. `Remove` on a clean project).
    NotPresent,
}

/// Aggregate result of `run_setup`.
#[derive(Debug, Clone, Serialize)]
pub struct SetupReport {
    pub settings_json: SetupAction,
    pub claude_local_md: SetupAction,
    pub initial_index: SetupAction,
}

/// Orchestrate all setup steps for the given mode.
// qual:allow(iosp) reason: "integration: dispatches to the three setup steps"
pub fn run_setup(project_dir: &Path, mode: SetupMode) -> Result<SetupReport> {
    let settings_json = setup_settings_json(project_dir, mode)?;
    let claude_local_md = setup_claude_local_md(project_dir, mode)?;
    let initial_index = setup_initial_index(project_dir, mode)?;
    Ok(SetupReport {
        settings_json,
        claude_local_md,
        initial_index,
    })
}

/// Write or update `.claude/settings.json` with the rlm configuration fragment.
// qual:allow(iosp) reason: "integration: read → merge → write pipeline"
pub fn setup_settings_json(project_dir: &Path, mode: SetupMode) -> Result<SetupAction> {
    let path = project_dir.join(CLAUDE_DIR).join(SETTINGS_FILE);
    let existing = read_settings(&path)?;
    let rlm = rlm_defaults();

    match mode {
        SetupMode::Apply | SetupMode::Check => {
            let merged = merge_settings(&existing, &rlm);
            let action = classify_settings_action(&path, &existing, &merged, mode);
            if matches!(mode, SetupMode::Apply) && !matches!(action, SetupAction::Skipped) {
                write_settings_atomic(&path, &merged)?;
            }
            Ok(action)
        }
        SetupMode::Remove => {
            if !path.exists() {
                return Ok(SetupAction::NotPresent);
            }
            let stripped = strip_rlm_from_settings(&existing);
            if stripped == existing {
                return Ok(SetupAction::Skipped);
            }
            write_settings_atomic(&path, &stripped)?;
            Ok(SetupAction::Removed)
        }
    }
}

/// Upsert the rlm-managed block in `CLAUDE.local.md`.
pub fn setup_claude_local_md(project_dir: &Path, mode: SetupMode) -> Result<SetupAction> {
    let path = project_dir.join(CLAUDE_LOCAL_MD);
    upsert_claude_local_md(&path, mode)
}

/// Run `rlm index` if the index database is missing.
///
/// In `Remove` mode we preserve the index (it's data, not config) and return
/// `Skipped`. In `Check` mode we report what would happen without writing.
// qual:allow(iosp) reason: "integration: mode dispatch + existence check + index run"
pub fn setup_initial_index(project_dir: &Path, mode: SetupMode) -> Result<SetupAction> {
    let config = Config::new(project_dir);
    let index_exists = config.index_exists();
    match mode {
        SetupMode::Remove => Ok(SetupAction::Skipped),
        SetupMode::Check => {
            if index_exists {
                Ok(SetupAction::Skipped)
            } else {
                Ok(SetupAction::WouldCreate)
            }
        }
        SetupMode::Apply => {
            if index_exists {
                Ok(SetupAction::Skipped)
            } else {
                crate::indexer::run_index(&config, None)?;
                Ok(SetupAction::Created)
            }
        }
    }
}

// -- Helpers ---------------------------------------------------------------

/// Decide which `SetupAction` corresponds to a merge, based on before/after state.
fn classify_settings_action(
    path: &Path,
    existing: &Value,
    merged: &Value,
    mode: SetupMode,
) -> SetupAction {
    let file_missing = !path.exists();
    if file_missing {
        return match mode {
            SetupMode::Check => SetupAction::WouldCreate,
            _ => SetupAction::Created,
        };
    }
    if existing == merged {
        return SetupAction::Skipped;
    }
    match mode {
        SetupMode::Check => SetupAction::WouldUpdate,
        _ => SetupAction::Updated,
    }
}

/// Read and parse a settings file. Returns `{}` for missing or empty files.
///
/// Refuses unparseable or non-object JSON with an error — overwriting a broken
/// user file would destroy their config. The caller (user) must fix the file
/// before re-running setup. Uses `ErrorKind::NotFound` matching (not
/// `path.exists()`) so permission / I/O errors surface instead of being
/// silently treated as "file missing" and then overwritten.
fn read_settings(path: &Path) -> Result<Value> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Value::Object(Map::new()));
        }
        Err(e) => return Err(e.into()),
    };
    if bytes.is_empty() {
        return Ok(Value::Object(Map::new()));
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(value @ Value::Object(_)) => Ok(value),
        Ok(_) => Err(RlmError::Other(format!(
            "{} is not a JSON object — rlm refuses to overwrite it. Remove or replace the file before re-running setup.",
            path.display()
        ))),
        Err(e) => Err(RlmError::Other(format!(
            "{} is not valid JSON ({e}) — rlm refuses to overwrite it. Fix the file before re-running setup.",
            path.display()
        ))),
    }
}

/// Atomic write via tempfile + rename. Emits pretty-printed JSON for hand-editability.
///
/// Cross-platform: Unix `rename` atomically replaces. Windows `rename` fails
/// when the target exists, so we remove the target first (brief non-atomic
/// window, acceptable for per-project config files).
fn write_settings_atomic(path: &Path, v: &Value) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".rlm_setup_tmp_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let body = serde_json::to_string_pretty(v)?;
    std::fs::write(&temp, body)?;
    if let Err(e) = replace_file(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(e.into());
    }
    Ok(())
}

/// Canonical JSON fragment that `rlm setup` contributes.
///
/// Permissions cover the default non-edit MCP tools (16 total — all except
/// `replace` and `insert`) plus commonly-run CLI bash invocations. `index`
/// and `verify` are included even though they can write; `replace` and
/// `insert` are intentionally NOT auto-allowed so direct source edits stay
/// under explicit user control.
#[must_use]
pub fn rlm_defaults() -> Value {
    json!({
        "permissions": {
            "allow": rlm_permission_entries(),
        },
        "mcpServers": {
            "rlm": {
                "command": "rlm",
                "args": ["mcp"]
            }
        }
    })
}

/// The exact list of rlm-owned permission strings. Exposed for `strip`.
#[must_use]
fn rlm_permission_entries() -> Vec<Value> {
    [
        "mcp__rlm__index",
        "mcp__rlm__search",
        "mcp__rlm__read",
        "mcp__rlm__overview",
        "mcp__rlm__refs",
        "mcp__rlm__stats",
        "mcp__rlm__partition",
        "mcp__rlm__summarize",
        "mcp__rlm__diff",
        "mcp__rlm__context",
        "mcp__rlm__deps",
        "mcp__rlm__scope",
        "mcp__rlm__files",
        "mcp__rlm__verify",
        "mcp__rlm__savings",
        "mcp__rlm__supported",
        "Bash(rlm index:*)",
        "Bash(rlm search:*)",
        "Bash(rlm overview:*)",
    ]
    .into_iter()
    .map(|s| Value::String(s.to_string()))
    .collect()
}

/// Merge `rlm` defaults into `existing` settings.
///
/// Rules:
/// - `permissions.allow` — dedup-by-string-value array merge; appends only missing entries.
/// - `mcpServers.rlm` — overwrite. Sibling `mcpServers.*` entries are preserved.
/// - All other keys in `existing` are left untouched.
#[must_use]
pub fn merge_settings(existing: &Value, rlm: &Value) -> Value {
    let mut out = existing.clone();
    merge_permissions(&mut out, rlm);
    merge_mcp_servers(&mut out, rlm);
    out
}

fn merge_permissions(out: &mut Value, rlm: &Value) {
    let Some(rlm_allow) = rlm
        .get("permissions")
        .and_then(|p| p.get("allow"))
        .and_then(|a| a.as_array())
    else {
        return;
    };

    if !out.is_object() {
        *out = Value::Object(Map::new());
    }
    let Value::Object(root) = out else { return };

    let perms = root
        .entry("permissions".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !perms.is_object() {
        *perms = Value::Object(Map::new());
    }
    let Value::Object(perms_obj) = perms else {
        return;
    };

    let allow = perms_obj
        .entry("allow".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !allow.is_array() {
        *allow = Value::Array(Vec::new());
    }
    let Value::Array(allow_arr) = allow else {
        return;
    };

    for entry in rlm_allow {
        if !allow_arr
            .iter()
            .any(|existing_entry| existing_entry == entry)
        {
            allow_arr.push(entry.clone());
        }
    }
}

fn merge_mcp_servers(out: &mut Value, rlm: &Value) {
    let Some(rlm_rlm) = rlm.get("mcpServers").and_then(|m| m.get("rlm")).cloned() else {
        return;
    };

    if !out.is_object() {
        *out = Value::Object(Map::new());
    }
    let Value::Object(root) = out else { return };

    let servers = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !servers.is_object() {
        *servers = Value::Object(Map::new());
    }
    let Value::Object(servers_map) = servers else {
        return;
    };

    servers_map.insert("rlm".to_string(), rlm_rlm);
}

/// Inverse of `merge_settings` — remove every rlm-owned entry, leave the rest.
#[must_use]
pub fn strip_rlm_from_settings(existing: &Value) -> Value {
    let mut out = existing.clone();
    strip_permissions(&mut out);
    strip_mcp_servers(&mut out);
    out
}

fn strip_permissions(out: &mut Value) {
    // Compare string-to-string instead of Value-to-Value; a non-string user
    // entry (unusual but legal JSON) is preserved untouched because it can't
    // be one of our rlm permission strings.
    let rlm_entries: std::collections::HashSet<String> = rlm_permission_entries()
        .into_iter()
        .filter_map(|entry| entry.as_str().map(str::to_owned))
        .collect();
    if let Some(allow) = out
        .get_mut("permissions")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut("allow"))
        .and_then(Value::as_array_mut)
    {
        allow.retain(|entry| entry.as_str().is_none_or(|s| !rlm_entries.contains(s)));
    }
}

fn strip_mcp_servers(out: &mut Value) {
    if let Some(servers) = out.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.remove("rlm");
    }
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
            classify_markdown_action(&existing, &next, path.exists(), mode)
                .map(Ok)
                .unwrap_or(Ok(SetupAction::Skipped))
        }
        SetupMode::Apply => {
            let next = build_updated_markdown(&existing);
            let action = classify_markdown_action(&existing, &next, path.exists(), mode)
                .unwrap_or(SetupAction::Skipped);
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
    let new_block = render_claude_local_md_section();
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
        (None, _) => append_block(existing, &new_block),
    };
    normalize_trailing_newline(&mut out);
    out
}

/// Append `new_block` to `existing`, separated by a blank line if `existing` is non-empty.
fn append_block(existing: &str, new_block: &str) -> String {
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(new_block);
    out
}

/// Ensure the string ends with exactly one `\n`. Required for idempotency —
/// slicing between markers can leave `\n\n` or no newline depending on input.
fn normalize_trailing_newline(s: &mut String) {
    while s.ends_with("\n\n") {
        s.pop();
    }
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
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
) -> Option<SetupAction> {
    if !file_existed {
        return Some(match mode {
            SetupMode::Check => SetupAction::WouldCreate,
            _ => SetupAction::Created,
        });
    }
    if existing == next {
        return Some(SetupAction::Skipped);
    }
    Some(match mode {
        SetupMode::Check => SetupAction::WouldUpdate,
        _ => SetupAction::Updated,
    })
}

fn write_text_atomic(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".rlm_setup_md_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&temp, content)?;
    if let Err(e) = replace_file(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(RlmError::Io(e));
    }
    Ok(())
}

/// Cross-platform file replacement: Unix `rename` atomically overwrites,
/// Windows `rename` requires explicit target removal first.
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if to.exists() {
            std::fs::remove_file(to)?;
        }
    }
    std::fs::rename(from, to)
}

/// The rlm-managed block, marker-wrapped.
#[must_use]
fn render_claude_local_md_section() -> String {
    format!(
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
- Inspect the `q` field; if `fallback_recommended: true`, fall back to native \
Read/Grep for affected lines.

### Self-healing Index
- rlm picks up external file changes automatically on the next tool call.
- Set `RLM_SKIP_REFRESH=1` to bypass the check in performance-sensitive scripts.
{MARKER_END}
"
    )
}

// -- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rlm_defaults_has_expected_shape() {
        let v = rlm_defaults();
        let allow = v
            .get("permissions")
            .and_then(|p| p.get("allow"))
            .and_then(|a| a.as_array())
            .expect("permissions.allow is an array");
        assert!(!allow.is_empty());
        assert!(allow.iter().any(|e| e == "mcp__rlm__search"));
        assert!(!allow.iter().any(|e| e == "mcp__rlm__replace"));
        assert!(!allow.iter().any(|e| e == "mcp__rlm__insert"));
        let server = v
            .get("mcpServers")
            .and_then(|m| m.get("rlm"))
            .expect("mcpServers.rlm present");
        assert_eq!(server.get("command"), Some(&Value::String("rlm".into())));
    }

    #[test]
    fn rlm_defaults_is_stable_across_calls() {
        // Idempotency depends on deterministic default output.
        let a = serde_json::to_string(&rlm_defaults()).unwrap();
        let b = serde_json::to_string(&rlm_defaults()).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn merge_settings_creates_from_empty() {
        let existing = json!({});
        let merged = merge_settings(&existing, &rlm_defaults());
        assert!(
            merged["permissions"]["allow"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e == "mcp__rlm__search"),
            "rlm permissions added"
        );
        assert!(merged["mcpServers"]["rlm"].is_object());
    }

    #[test]
    fn merge_settings_preserves_unrelated_user_entries() {
        let existing = json!({
            "permissions": {
                "allow": ["Bash(git diff:*)", "mcp__user_tool__foo"]
            },
            "mcpServers": {
                "other": { "command": "other", "args": [] }
            },
            "env": {
                "MY_VAR": "x"
            }
        });
        let merged = merge_settings(&existing, &rlm_defaults());
        let allow = merged["permissions"]["allow"].as_array().unwrap();
        assert!(allow.iter().any(|e| e == "Bash(git diff:*)"));
        assert!(allow.iter().any(|e| e == "mcp__user_tool__foo"));
        assert!(allow.iter().any(|e| e == "mcp__rlm__search"));
        assert!(merged["mcpServers"]["other"].is_object());
        assert!(merged["mcpServers"]["rlm"].is_object());
        assert_eq!(merged["env"]["MY_VAR"], "x");
    }

    #[test]
    fn merge_settings_is_idempotent() {
        let existing = json!({});
        let once = merge_settings(&existing, &rlm_defaults());
        let twice = merge_settings(&once, &rlm_defaults());
        assert_eq!(once, twice);
    }

    #[test]
    fn strip_removes_rlm_entries_only() {
        let initial = json!({
            "permissions": {
                "allow": ["Bash(git:*)", "mcp__user__x"]
            }
        });
        let merged = merge_settings(&initial, &rlm_defaults());
        let stripped = strip_rlm_from_settings(&merged);
        let allow = stripped["permissions"]["allow"].as_array().unwrap();
        assert!(allow.iter().any(|e| e == "Bash(git:*)"));
        assert!(allow.iter().any(|e| e == "mcp__user__x"));
        assert!(!allow.iter().any(|e| e == "mcp__rlm__search"));
        assert!(stripped["mcpServers"].get("rlm").is_none());
    }

    #[test]
    fn strip_preserves_sibling_mcp_servers() {
        let input = json!({
            "mcpServers": {
                "rlm": { "command": "rlm", "args": ["mcp"] },
                "other": { "command": "other", "args": [] }
            }
        });
        let stripped = strip_rlm_from_settings(&input);
        assert!(stripped["mcpServers"]["other"].is_object());
        assert!(stripped["mcpServers"].get("rlm").is_none());
    }

    #[test]
    fn render_block_contains_markers() {
        let section = render_claude_local_md_section();
        assert!(section.starts_with(MARKER_BEGIN));
        assert!(section.trim_end().ends_with(MARKER_END));
        assert!(section.contains("rlm Workflow Instructions"));
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
