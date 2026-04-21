//! Tests for `config_format.rs` (task #121).

use super::super::orchestrator::{SetupAction, SetupMode};
use super::setup_config_format;
use std::fs;
use tempfile::TempDir;

fn read_config(dir: &TempDir) -> String {
    fs::read_to_string(dir.path().join(".rlm/config.toml")).unwrap()
}

#[test]
fn setup_creates_config_toml_with_toon_format_when_absent() {
    let dir = TempDir::new().unwrap();
    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Created);
    let body = read_config(&dir);
    assert!(
        body.contains("[output]"),
        "should write [output] section: {body:?}"
    );
    assert!(
        body.contains("format = \"toon\""),
        "should default to toon: {body:?}"
    );
}

#[test]
fn setup_adds_output_section_when_config_exists_without_it() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[indexing]\nmax_file_size_mb = 5\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Updated);

    let body = read_config(&dir);
    assert!(
        body.contains("max_file_size_mb = 5"),
        "user's existing section must be preserved: {body:?}"
    );
    assert!(
        body.contains("[output]") && body.contains("format = \"toon\""),
        "output section should have been appended: {body:?}"
    );
}

#[test]
fn setup_preserves_existing_format_preference() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[output]\nformat = \"json\"\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Skipped);

    let body = read_config(&dir);
    assert!(
        body.contains("format = \"json\""),
        "user's explicit format should remain: {body:?}"
    );
    assert!(
        !body.contains("format = \"toon\""),
        "toon must not overwrite user preference: {body:?}"
    );
}

#[test]
fn setup_remove_leaves_format_alone() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[output]\nformat = \"toon\"\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Remove).unwrap();
    assert_eq!(action, SetupAction::Skipped);

    let body = read_config(&dir);
    assert!(
        body.contains("format = \"toon\""),
        "remove must not touch format preference: {body:?}"
    );
}

#[test]
fn setup_check_reports_would_create_without_writing() {
    let dir = TempDir::new().unwrap();
    let action = setup_config_format(dir.path(), SetupMode::Check).unwrap();
    assert_eq!(action, SetupAction::WouldCreate);
    assert!(
        !dir.path().join(".rlm/config.toml").exists(),
        "check mode must not write"
    );
}

#[test]
fn setup_is_idempotent_when_toon_already_set() {
    let dir = TempDir::new().unwrap();
    setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    let first_body = read_config(&dir);

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Skipped);

    let second_body = read_config(&dir);
    assert_eq!(
        first_body, second_body,
        "second apply must not alter the file"
    );
}

/// Regression: `has_output_format` used `line.starts_with("format")`, which
/// falsely matched keys like `formatting` / `formatter` / `format_version`
/// and skipped appending the real `format = "..."` line. Caught by Copilot
/// on PR. The detector must key on the exact `format` identifier, not a
/// prefix match.
#[test]
fn setup_adds_format_when_existing_output_has_only_similar_prefix_keys() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[output]\n\
                        formatting = \"aligned\"\n\
                        formatter = \"default\"\n\
                        format_version = 2\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(
        action,
        SetupAction::Updated,
        "lookalike keys must not count as an existing `format` preference"
    );

    let body = read_config(&dir);
    assert!(
        body.contains("format = \"toon\""),
        "real `format` line should have been appended: {body:?}"
    );
    // Pre-existing keys survive.
    for kept in ["formatting", "formatter", "format_version"] {
        assert!(
            body.contains(kept),
            "pre-existing `{kept}` must be preserved: {body:?}"
        );
    }
}

/// Complementary: an actual `format = "x"` line with surrounding
/// whitespace is still detected (i.e. we don't over-tighten the fix
/// to require `format=` with no space).
#[test]
fn setup_detects_format_with_spaces_and_tabs() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[output]\n\tformat\t=\t\"json\"\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(
        action,
        SetupAction::Skipped,
        "`format = ...` with tabs/spaces must still be detected as present"
    );
}

/// Complementary: a `format = ...` line **outside** of `[output]`
/// (e.g. inside `[indexing]`) must not be treated as the output format.
#[test]
fn setup_ignores_format_key_outside_output_section() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[indexing]\nformat = \"legacy\"\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(
        action,
        SetupAction::Updated,
        "`format` under `[indexing]` is unrelated — output section must still be appended"
    );
    let body = read_config(&dir);
    assert!(body.contains("[output]"));
    assert!(body.contains("format = \"toon\""));
}

/// Regression (Copilot): if `[output]` exists but has no `format`
/// key (only look-alike keys like `formatting`), the old impl
/// appended a SECOND `[output]` table, producing invalid TOML that
/// `Config::load_settings` cannot parse. The file must stay a valid
/// TOML document with a single `[output]` section.
#[test]
fn setup_injects_format_into_existing_output_section_without_duplicating_it() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[output]\n\
                        formatting = \"aligned\"\n\
                        format_version = 2\n\
                        \n\
                        [indexing]\n\
                        max_file_size_mb = 5\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Updated);

    let body = read_config(&dir);
    assert_eq!(
        body.matches("[output]").count(),
        1,
        "must not emit a second [output] table — would be invalid TOML: {body}"
    );
    assert!(
        body.contains("format = \"toon\""),
        "format key must have been injected: {body}"
    );
    // User's pre-existing keys survive.
    assert!(body.contains("formatting = \"aligned\""));
    assert!(body.contains("format_version = 2"));
    // And the neighbouring section is intact.
    assert!(body.contains("[indexing]"));
    assert!(body.contains("max_file_size_mb = 5"));

    // Acid test: the result parses as TOML.
    let parsed: toml::Value = toml::from_str(&body).expect("result must be valid TOML");
    let output = parsed
        .get("output")
        .and_then(|v| v.as_table())
        .expect("[output] must be a table");
    assert_eq!(
        output.get("format").and_then(|v| v.as_str()),
        Some("toon"),
        "parsed [output].format should be 'toon': {output:?}"
    );
    assert_eq!(
        output.get("formatting").and_then(|v| v.as_str()),
        Some("aligned"),
        "parsed [output].formatting should be 'aligned': {output:?}"
    );
}

/// The file is written via an atomic-rename path just like the other
/// setup writers (settings.rs, claude_md.rs). We don't probe the
/// crash-during-write behaviour directly — that'd need fault
/// injection — but we pin the observable consequence: after a
/// successful setup run, no `*.tmp` / partial-file artefacts are
/// left behind in `.rlm/`. Copilot flagged the inconsistency with
/// `write_atomic` in neighbouring writers.
#[test]
fn setup_leaves_no_tempfile_artefacts_in_rlm_dir() {
    let dir = TempDir::new().unwrap();
    setup_config_format(dir.path(), SetupMode::Apply).unwrap();

    let rlm_dir = dir.path().join(".rlm");
    let entries: Vec<_> = fs::read_dir(&rlm_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    assert!(entries.iter().any(|n| n == "config.toml"));
    for name in &entries {
        assert!(
            !name.ends_with(".tmp") && !name.starts_with(".config.toml."),
            "atomic-write temp artefact left behind: {name} (all entries: {entries:?})"
        );
    }
}
