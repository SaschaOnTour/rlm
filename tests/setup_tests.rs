//! End-to-end integration tests for `rlm setup`.
//!
//! Covers: create, `--check` dry-run, `--remove` cleanup, idempotency,
//! and merge-with-existing-user-config.

use rlm::setup::{run_setup, setup_settings_json, SetupAction, SetupMode};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn read_settings(dir: &Path) -> Value {
    let bytes = fs::read(dir.join(".claude/settings.json")).expect("settings.json exists");
    serde_json::from_slice(&bytes).expect("valid JSON")
}

fn make_project() -> TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    // Create a tiny Rust file so the initial-index step has something to index.
    fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    dir
}

#[test]
fn setup_creates_settings_json_from_scratch() {
    let dir = make_project();
    let report = run_setup(dir.path(), SetupMode::Apply).unwrap();

    assert_eq!(report.settings_json, SetupAction::Created);
    assert_eq!(report.claude_local_md, SetupAction::Created);
    assert_eq!(report.initial_index, SetupAction::Created);

    let s = read_settings(dir.path());
    let allow = s["permissions"]["allow"].as_array().unwrap();
    assert!(allow.iter().any(|v| v == "mcp__rlm__search"));
    assert!(s["mcpServers"]["rlm"].is_object());
    assert!(dir.path().join("CLAUDE.local.md").exists());
    assert!(dir.path().join(".rlm/index.db").exists());
}

#[test]
fn setup_check_mode_writes_nothing() {
    let dir = make_project();
    let report = run_setup(dir.path(), SetupMode::Check).unwrap();

    assert_eq!(report.settings_json, SetupAction::WouldCreate);
    assert_eq!(report.claude_local_md, SetupAction::WouldCreate);
    assert_eq!(report.initial_index, SetupAction::WouldCreate);

    assert!(!dir.path().join(".claude/settings.json").exists());
    assert!(!dir.path().join("CLAUDE.local.md").exists());
    assert!(!dir.path().join(".rlm/index.db").exists());
}

#[test]
fn setup_remove_strips_rlm_entries_only() {
    let dir = make_project();
    // Pre-populate settings.json with a user permission, then apply rlm.
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    fs::write(
        dir.path().join(".claude/settings.json"),
        serde_json::to_string_pretty(&json!({
            "permissions": { "allow": ["Bash(git diff:*)"] },
            "mcpServers": { "other": { "command": "other", "args": [] } }
        }))
        .unwrap(),
    )
    .unwrap();

    run_setup(dir.path(), SetupMode::Apply).unwrap();

    // Remove.
    let report = run_setup(dir.path(), SetupMode::Remove).unwrap();
    assert_eq!(report.settings_json, SetupAction::Removed);
    assert_eq!(report.claude_local_md, SetupAction::Removed);
    assert_eq!(report.initial_index, SetupAction::Skipped);

    let s = read_settings(dir.path());
    let allow = s["permissions"]["allow"].as_array().unwrap();
    // User permission preserved.
    assert!(allow.iter().any(|v| v == "Bash(git diff:*)"));
    // All rlm permissions gone.
    assert!(!allow
        .iter()
        .any(|v| v.as_str().unwrap_or("").starts_with("mcp__rlm__")));
    // rlm MCP server gone, user server preserved.
    assert!(s["mcpServers"].get("rlm").is_none());
    assert!(s["mcpServers"]["other"].is_object());
}

#[test]
fn setup_is_idempotent() {
    let dir = make_project();
    run_setup(dir.path(), SetupMode::Apply).unwrap();
    let bytes1 = fs::read(dir.path().join(".claude/settings.json")).unwrap();
    let md1 = fs::read_to_string(dir.path().join("CLAUDE.local.md")).unwrap();

    let report2 = run_setup(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(report2.settings_json, SetupAction::Skipped);
    assert_eq!(report2.claude_local_md, SetupAction::Skipped);
    assert_eq!(report2.initial_index, SetupAction::Skipped);

    let bytes2 = fs::read(dir.path().join(".claude/settings.json")).unwrap();
    let md2 = fs::read_to_string(dir.path().join("CLAUDE.local.md")).unwrap();
    assert_eq!(bytes1, bytes2, "settings.json byte-identical on repeat");
    assert_eq!(md1, md2, "CLAUDE.local.md byte-identical on repeat");
}

#[test]
fn setup_merges_with_existing_user_config() {
    let dir = make_project();
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    fs::write(
        dir.path().join(".claude/settings.json"),
        serde_json::to_string_pretty(&json!({
            "permissions": {
                "allow": ["Bash(git:*)", "mcp__user_tool__foo"]
            },
            "mcpServers": {
                "other": { "command": "other", "args": [] }
            },
            "env": { "MY_VAR": "x" }
        }))
        .unwrap(),
    )
    .unwrap();

    let action = setup_settings_json(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Updated);

    let s = read_settings(dir.path());
    let allow = s["permissions"]["allow"].as_array().unwrap();
    // User entries survive.
    assert!(allow.iter().any(|v| v == "Bash(git:*)"));
    assert!(allow.iter().any(|v| v == "mcp__user_tool__foo"));
    // rlm entries added.
    assert!(allow.iter().any(|v| v == "mcp__rlm__search"));
    // User MCP server + rlm MCP server coexist.
    assert!(s["mcpServers"]["other"].is_object());
    assert!(s["mcpServers"]["rlm"].is_object());
    // Unrelated top-level keys preserved.
    assert_eq!(s["env"]["MY_VAR"], "x");
}

#[test]
fn setup_refuses_to_overwrite_invalid_json() {
    // Regression: invalid user JSON must not be silently replaced.
    let dir = make_project();
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    let invalid = "{ not valid json,,,";
    fs::write(dir.path().join(".claude/settings.json"), invalid).unwrap();

    let result = run_setup(dir.path(), SetupMode::Apply);
    assert!(result.is_err(), "setup must error on invalid JSON");

    // Original content preserved.
    let after = fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap();
    assert_eq!(after, invalid);
}

#[test]
fn setup_refuses_non_object_json() {
    // Regression: a JSON array / string / etc. is not a settings object.
    let dir = make_project();
    fs::create_dir_all(dir.path().join(".claude")).unwrap();
    fs::write(
        dir.path().join(".claude/settings.json"),
        "[\"array\", \"not\", \"object\"]",
    )
    .unwrap();

    let result = run_setup(dir.path(), SetupMode::Apply);
    assert!(result.is_err(), "setup must error on non-object JSON root");
}

#[test]
fn setup_preserves_markdown_outside_markers() {
    let dir = make_project();
    let pre = "# Project\n\nExisting notes.\n\n## Todo\n- Item\n";
    fs::write(dir.path().join("CLAUDE.local.md"), pre).unwrap();

    run_setup(dir.path(), SetupMode::Apply).unwrap();

    let after = fs::read_to_string(dir.path().join("CLAUDE.local.md")).unwrap();
    assert!(after.starts_with("# Project"));
    assert!(after.contains("Existing notes."));
    assert!(after.contains("## Todo"));
    assert!(after.contains("<!-- rlm:begin -->"));
    assert!(after.contains("<!-- rlm:end -->"));
    assert!(after.contains("rlm Workflow Instructions"));
}
