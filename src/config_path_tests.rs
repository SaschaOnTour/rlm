//! Path / exclude-pattern tests for `config.rs`.
//!
//! Split out of `config_tests.rs` to keep each companion focused on a
//! smaller cluster of behaviors (SRP_MODULE). Settings load/save and
//! size / default tests stay in `config_tests.rs`; this file covers
//! how `Config` resolves relative paths, normalizes separators, and
//! applies exclude-glob filtering.

use super::{Config, Path, PathBuf};
use tempfile::TempDir;

#[test]
fn config_new_sets_paths() {
    let cfg = Config::new("/tmp/project");
    assert_eq!(cfg.project_root, PathBuf::from("/tmp/project"));
    assert_eq!(cfg.rlm_dir, PathBuf::from("/tmp/project/.rlm"));
    assert_eq!(cfg.db_path, PathBuf::from("/tmp/project/.rlm/index.db"));
}

#[test]
fn relative_path_strips_prefix() {
    let cfg = Config::new("/tmp/project");
    let rel = cfg.relative_path(Path::new("/tmp/project/src/main.rs"));
    assert_eq!(rel, "src/main.rs");
}

#[test]
fn should_exclude_patterns() {
    let cfg = Config::new("/tmp/project");

    // Default patterns should match
    assert!(cfg.should_exclude(Path::new("/tmp/project/node_modules/foo.js")));
    assert!(cfg.should_exclude(Path::new("/tmp/project/.git/config")));
    assert!(cfg.should_exclude(Path::new("/tmp/project/target/debug/app")));
    assert!(cfg.should_exclude(Path::new("/tmp/project/dist/bundle.js")));
    assert!(cfg.should_exclude(Path::new("/tmp/project/__pycache__/mod.pyc")));

    // Non-excluded paths should not match
    assert!(!cfg.should_exclude(Path::new("/tmp/project/src/main.rs")));
    assert!(!cfg.should_exclude(Path::new("/tmp/project/lib/utils.js")));
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
    let cfg = Config::new("/tmp/project");

    // Windows-style path should be normalized
    let rel = cfg.relative_path(Path::new("/tmp/project/src\\nested\\file.rs"));
    assert!(!rel.contains('\\'));
}
