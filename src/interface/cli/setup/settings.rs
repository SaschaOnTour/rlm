//! `.claude/settings.json` merge and strip for `rlm setup`.
//!
//! Contributes a rlm-owned fragment — permissions allow-list + `mcpServers.rlm`
//! — into the user's Claude Code settings file without clobbering any unrelated
//! keys they have there. `strip_rlm_from_settings` is the inverse used by
//! `rlm setup --remove`.

use std::path::Path;

use serde_json::{json, Map, Value};

use crate::error::Result;

use super::orchestrator::write_atomic;
use super::orchestrator::{SetupAction, SetupError, SetupMode};

/// Directory under `project_dir` holding Claude Code settings.
const CLAUDE_DIR: &str = ".claude";
/// Settings file that `rlm setup` creates/updates (team-shared, committed).
const SETTINGS_FILE: &str = "settings.json";

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
    write_atomic(path, body.as_bytes())
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
    fn merge_preserves_non_object_permissions_value() {
        // Regression: if a user has `permissions` as a non-object value
        // (unusual but legal JSON), rlm setup must NOT silently overwrite
        // it with `{}`. Leave the user value alone.
        let existing = json!({ "permissions": "something-odd" });
        let merged = merge_settings(&existing, &rlm_defaults());
        assert_eq!(
            merged["permissions"], "something-odd",
            "non-object permissions must be preserved"
        );
        // rlm's own mcpServers entry should still be added.
        assert!(merged["mcpServers"]["rlm"].is_object());
    }

    #[test]
    fn merge_preserves_non_array_permissions_allow() {
        // `permissions.allow` as a non-array value (e.g. user accidentally
        // typed a string) must stay untouched — rlm merge skips rather than
        // replacing with `[]`.
        let existing = json!({
            "permissions": { "allow": "all" }
        });
        let merged = merge_settings(&existing, &rlm_defaults());
        assert_eq!(
            merged["permissions"]["allow"], "all",
            "non-array `allow` must be preserved"
        );
    }

    #[test]
    fn merge_preserves_non_object_mcp_servers_value() {
        // Same principle for `mcpServers` — don't clobber user's unexpected type.
        let existing = json!({ "mcpServers": ["not", "an", "object"] });
        let merged = merge_settings(&existing, &rlm_defaults());
        assert!(
            merged["mcpServers"].is_array(),
            "non-object mcpServers must be preserved: {}",
            merged["mcpServers"]
        );
        // rlm's permissions should still be added at the top level.
        assert!(merged["permissions"]["allow"].is_array());
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
}
