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
