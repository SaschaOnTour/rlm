//! `.claude/settings.json` merge and strip for `rlm setup`.
//!
//! Contributes a rlm-owned fragment — permissions allow-list + `mcpServers.rlm`
//! — into the user's Claude Code settings file without clobbering any unrelated
//! keys they have there. `strip_rlm_from_settings` is the inverse used by
//! `rlm setup --remove`.

use std::path::Path;

use serde_json::{json, Map, Value};

use crate::error::Result;
use crate::infrastructure::filesystem::atomic_writer::write_atomic;

use super::orchestrator::{SetupAction, SetupError, SetupMode};

/// Directory under `project_dir` holding Claude Code settings.
const CLAUDE_DIR: &str = ".claude";
/// Settings file that `rlm setup` creates/updates (team-shared, committed).
const SETTINGS_FILE: &str = "settings.json";

/// Write or update `.claude/settings.json` with the rlm configuration fragment.
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
        Ok(_) => Err(SetupError::NotJsonObject {
            path: path.display().to_string(),
        }
        .into()),
        Err(source) => Err(SetupError::InvalidJson {
            path: path.display().to_string(),
            source,
        }
        .into()),
    }
}

/// Atomic write for settings.json — pretty-printed JSON for hand-editability.
fn write_settings_atomic(path: &Path, v: &Value) -> Result<()> {
    let body = serde_json::to_string_pretty(v)?;
    write_atomic(path, body.as_bytes())?;
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

    // Bail if the target root isn't an object — don't clobber user data.
    // `read_settings` guarantees Object for fresh loads; guard is defensive.
    let Value::Object(root) = out else { return };

    // Insert an empty object only when "permissions" is absent. If the user
    // has `permissions` as a non-object value (string/array/number), leave
    // it alone and skip the rlm merge rather than silently replacing.
    let perms = root
        .entry("permissions".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(perms_obj) = perms else {
        return;
    };

    let allow = perms_obj
        .entry("allow".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    let Value::Array(allow_arr) = allow else {
        // Non-array `allow` stays untouched. User data preservation trumps
        // silent merge.
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

    let Value::Object(root) = out else { return };

    let servers = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers_map) = servers else {
        // Non-object `mcpServers`: preserve user value, skip the rlm entry.
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

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
