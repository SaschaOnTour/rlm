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

/// Regression: a prefix-match (`starts_with("format")`) incorrectly
/// matched `formatting` / `formatter` / `format_version` and skipped
/// writing the real `format = "..."` line. The detector must key on
/// the exact `format` identifier, not a prefix.
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

/// A TOML header with a trailing comment (`[output] # note`) is
/// valid TOML; the classifier must still recognise it as the
/// `[output]` section. Without that recognition, setup would append
/// a second `[output]` table and produce invalid TOML.
#[test]
fn setup_detects_output_section_with_trailing_comment() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[output]   # produced by rlm setup on 2026-04-22\n\
                        format = \"pretty\"\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(
        action,
        SetupAction::Skipped,
        "[output]-with-trailing-comment must be recognised and user's format preserved"
    );

    let body = read_config(&dir);
    assert_eq!(
        body.matches("[output]").count(),
        1,
        "must not duplicate the [output] table: {body}"
    );
    assert!(
        body.contains("format = \"pretty\""),
        "user's explicit format must survive: {body}"
    );
    // Parse-sanity: whole result is valid TOML with exactly one `output`.
    let parsed: toml::Value = toml::from_str(&body).expect("result must be valid TOML");
    assert_eq!(
        parsed
            .get("output")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("format"))
            .and_then(|v| v.as_str()),
        Some("pretty"),
    );
}

/// `Path::exists()` returns `false` on permission/I/O errors too,
/// which would send an unreadable `config.toml` down the "file
/// missing" path and clobber it. Setup must distinguish "genuinely
/// absent" (→ create) from "exists but unreadable" (→ surface the
/// error).
#[test]
#[cfg(unix)]
fn setup_propagates_read_error_instead_of_treating_unreadable_as_missing() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let config_path = dir.path().join(".rlm/config.toml");
    fs::write(&config_path, "[output]\nformat = \"json\"\n").unwrap();
    // chmod 000 — owner cannot read. Verify the chmod is actually
    // effective before asserting; under `sudo` / root / some
    // container filesystems the bits don't constrain the owner, and
    // the test-scenario would be meaningless. Standard CI escape.
    fs::set_permissions(&config_path, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_to_string(&config_path).is_ok() {
        // Permission bits don't constrain us here (e.g. root). Skip —
        // restore permissions first so tempdir cleanup works.
        let _ = fs::set_permissions(&config_path, fs::Permissions::from_mode(0o644));
        return;
    }

    let result = setup_config_format(dir.path(), SetupMode::Apply);

    // Best-effort restore so the tempdir can clean up.
    let _ = fs::set_permissions(&config_path, fs::Permissions::from_mode(0o644));

    assert!(
        result.is_err(),
        "unreadable config must surface as Err — got Ok({:?})",
        result.ok()
    );
    // The original content must still be there: we never overwrote it
    // via the "NoFile → create" path.
    let content = fs::read_to_string(&config_path).unwrap();
    assert!(
        content.contains("format = \"json\""),
        "must not have clobbered the unreadable file: {content:?}"
    );
}

/// Same pathology but the `[output]` section has NO `format` key —
/// should inject rather than duplicate.
#[test]
fn setup_injects_into_output_section_with_trailing_comment() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[output] # header comment\nformatting = \"dense\"\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Updated);

    let body = read_config(&dir);
    assert_eq!(body.matches("[output]").count(), 1);
    let parsed: toml::Value = toml::from_str(&body).expect("result must be valid TOML");
    let output = parsed
        .get("output")
        .and_then(|v| v.as_table())
        .expect("[output] must be a table");
    assert_eq!(output.get("format").and_then(|v| v.as_str()), Some("toon"));
    assert_eq!(
        output.get("formatting").and_then(|v| v.as_str()),
        Some("dense")
    );
}

/// If `[output]` exists but has no `format` key (only look-alike
/// keys like `formatting`), the writer must inject `format` INTO the
/// existing section — appending a SECOND `[output]` table produces
/// invalid TOML that `Config::load_settings` cannot parse.
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

/// The file is written via an atomic-rename path just like the
/// other setup writers (settings.rs, claude_md.rs). We don't probe
/// crash-during-write directly — that'd need fault injection — but
/// we pin the observable consequence: after a successful setup run,
/// no `*.tmp` / partial-file artefacts are left behind in `.rlm/`.
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

/// A pre-existing CRLF config keeps CRLF everywhere — the inject path
/// must not silently introduce bare LFs and produce mixed line
/// endings on Windows.
#[test]
fn setup_inject_preserves_crlf_line_endings() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[output]\r\nverbose = true\r\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Updated);

    let body = fs::read(dir.path().join(".rlm/config.toml")).unwrap();
    let body = String::from_utf8(body).unwrap();
    assert!(
        body.contains("format = \"toon\"\r\n"),
        "injected line must use CRLF to match existing file style: {body:?}"
    );
    assert!(
        !body.contains("\r\n\n") && !body.replace("\r\n", "").contains('\n'),
        "file must not contain bare LFs after inject (mixed EOL), got: {body:?}"
    );
}

/// Same guarantee for the append path: a pre-existing CRLF file
/// without `[output]` gets its appended section in CRLF too.
#[test]
fn setup_append_preserves_crlf_line_endings() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join(".rlm")).unwrap();
    let pre_existing = "[indexing]\r\nmax_file_size_mb = 5\r\n";
    fs::write(dir.path().join(".rlm/config.toml"), pre_existing).unwrap();

    let action = setup_config_format(dir.path(), SetupMode::Apply).unwrap();
    assert_eq!(action, SetupAction::Updated);

    let body = String::from_utf8(fs::read(dir.path().join(".rlm/config.toml")).unwrap()).unwrap();
    assert!(body.contains("[output]\r\n"));
    assert!(body.contains("format = \"toon\"\r\n"));
    assert!(
        !body.replace("\r\n", "").contains('\n'),
        "file must stay pure CRLF after append, got: {body:?}"
    );
}
