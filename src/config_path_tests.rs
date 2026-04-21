//! Path / exclude-pattern tests for `config.rs`.
//!
//! Split out of `config_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Settings load/save and
//! size / default tests stay in `config_tests.rs`; this file covers
//! how `Config` resolves relative paths, normalizes separators, and
//! applies exclude-glob filtering.

use super::Config;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[test]
fn config_new_sets_paths() {
    // Build expected paths via the same `.join` that `Config::new` uses
    // internally, so the assertions hold regardless of platform path
    // separator (Windows `\` vs. Unix `/`).
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let cfg = Config::new(root);
    assert_eq!(cfg.project_root, PathBuf::from(root));
    assert_eq!(cfg.rlm_dir, root.join(".rlm"));
    assert_eq!(cfg.db_path, root.join(".rlm").join("index.db"));
}

#[test]
fn relative_path_strips_prefix() {
    let tmp = TempDir::new().unwrap();
    let cfg = Config::new(tmp.path());
    let abs = tmp.path().join("src").join("main.rs");
    let rel = cfg.relative_path(&abs);
    // `relative_path` normalises backslashes to forward slashes, so the
    // expected value is stable across platforms.
    assert_eq!(rel, "src/main.rs");
}

#[test]
fn should_exclude_patterns() {
    let tmp = TempDir::new().unwrap();
    let cfg = Config::new(tmp.path());
    let root = tmp.path();

    // Default patterns should match (contains-based, separator-agnostic)
    assert!(cfg.should_exclude(&root.join("node_modules").join("foo.js")));
    assert!(cfg.should_exclude(&root.join(".git").join("config")));
    assert!(cfg.should_exclude(&root.join("target").join("debug").join("app")));
    assert!(cfg.should_exclude(&root.join("dist").join("bundle.js")));
    assert!(cfg.should_exclude(&root.join("__pycache__").join("mod.pyc")));

    // Non-excluded paths should not match
    assert!(!cfg.should_exclude(&root.join("src").join("main.rs")));
    assert!(!cfg.should_exclude(&root.join("lib").join("utils.js")));
}

#[test]
fn custom_quality_log_path() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::new(tmp.path());

    // Default path
    assert_eq!(cfg.get_quality_log_path(), cfg.quality_log_path);

    // Ensure .rlm/ exists so validate_relative_path can canonicalize
    cfg.ensure_rlm_dir().unwrap();

    // Custom path — validate_relative_path returns canonical form
    cfg.settings.quality.log_file = Some("custom-quality.log".to_string());
    let expected = cfg
        .rlm_dir
        .canonicalize()
        .unwrap()
        .join("custom-quality.log");
    assert_eq!(cfg.get_quality_log_path(), expected);
}

#[test]
fn config_path_normalization() {
    // Root itself uses a platform-native separator via TempDir; the test
    // still builds the input with literal backslashes to verify that
    // `relative_path` normalises them in the output regardless of OS.
    let tmp = TempDir::new().unwrap();
    let cfg = Config::new(tmp.path());
    let input = format!("{}/src\\nested\\file.rs", tmp.path().display());
    let rel = cfg.relative_path(Path::new(&input));
    assert!(!rel.contains('\\'));
}
