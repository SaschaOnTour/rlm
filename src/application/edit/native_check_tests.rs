//! Tests for `native_check.rs` (task #115).
//!
//! These invoke real `cargo check` subprocesses against tiny tempdir
//! projects. They are slow(ish) — typically 1-3s each on incremental
//! runs — so the surface is kept small: one positive, one
//! syntax-failure, one name-resolution failure, plus guards against
//! running without a Cargo.toml and when disabled via config.

use super::{run_check, BuildReport};
use crate::config::EditSettings;
use std::fs;
use tempfile::TempDir;

/// Set up a minimal Cargo project with the given `lib.rs` content.
/// The Cargo.toml has no dependencies so `cargo check` stays fast and
/// fully offline.
fn setup_cargo_project(lib_rs: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "rlm_native_check_probe"
version = "0.0.1"
edition = "2021"
"#,
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), lib_rs).unwrap();
    dir
}

fn default_settings() -> EditSettings {
    EditSettings {
        native_check: true,
        native_check_timeout_secs: 30,
    }
}

#[test]
fn rust_check_passes_on_valid_code() {
    let dir = setup_cargo_project("pub fn ok() -> i32 { 42 }\n");
    let report: BuildReport = run_check(dir.path(), "rust", &default_settings())
        .expect("check should run for rust + Cargo.toml");
    assert!(
        report.passed,
        "expected pass, got errors: {:?}",
        report.errors
    );
    assert!(report.errors.is_empty());
    assert_eq!(report.checker, "cargo check");
}

#[test]
fn rust_check_fails_on_syntax_error() {
    let dir = setup_cargo_project("pub fn broken() -> i32 { \n");
    let report = run_check(dir.path(), "rust", &default_settings()).expect("check should run");
    assert!(!report.passed);
    assert!(
        !report.errors.is_empty(),
        "expected at least one error on syntax-broken input"
    );
}

#[test]
fn rust_check_fails_on_name_resolution_error() {
    // The `&bn` case from #113: syntactically valid (`bn` is a valid
    // ident), semantically broken (unresolved name). Syntax Guard's
    // blind spot, this check's whole reason for being.
    let dir = setup_cargo_project("pub fn x() -> Option<&'static u8> { Some(&bn) }\n");
    let report = run_check(dir.path(), "rust", &default_settings()).expect("check should run");
    assert!(
        !report.passed,
        "name-resolution error should fail the check"
    );
    let joined = report
        .errors
        .iter()
        .map(|e| e.message.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("bn") || joined.contains("cannot find") || joined.contains("not found"),
        "expected error to mention the missing ident, got: {joined}"
    );
}

#[test]
fn rust_check_returns_none_without_cargo_toml() {
    let dir = tempfile::tempdir().unwrap();
    let report = run_check(dir.path(), "rust", &default_settings());
    assert!(
        report.is_none(),
        "no Cargo.toml → no check; got: {report:?}"
    );
}

#[test]
fn rust_check_returns_none_when_disabled_in_config() {
    let dir = setup_cargo_project("pub fn ok() -> i32 { 42 }\n");
    let disabled = EditSettings {
        native_check: false,
        native_check_timeout_secs: 10,
    };
    let report = run_check(dir.path(), "rust", &disabled);
    assert!(report.is_none(), "disabled config → no check");
}

#[test]
fn check_returns_none_for_unsupported_lang() {
    // Java, C#, etc. are out of scope for this slice.
    let dir = setup_cargo_project("pub fn ok() -> i32 { 42 }\n");
    let report = run_check(dir.path(), "java", &default_settings());
    assert!(report.is_none());
}
