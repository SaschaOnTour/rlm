//! Tests for `settings.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "settings_tests.rs"] mod tests;`.

use super::{merge_settings, rlm_defaults, strip_rlm_from_settings, Value};
use serde_json::json;

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
