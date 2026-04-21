//! Settings / default / size tests for `config.rs`.
//!
//! Moved from the inline `#[cfg(test)] mod tests { ... }` block
//! into this companion file to match the Phase-4 convention
//! across the whole codebase. Wired back in via
//! `#[cfg(test)] #[path = "config_tests.rs"] mod tests;`.
//!
//! Path / exclude-pattern tests live in the sibling `config_path_tests.rs`.

use super::{Config, UserSettings, BYTES_PER_MB, DEFAULT_MAX_FILE_SIZE_MB, TEST_MAX_FILE_SIZE_MB};
use tempfile::TempDir;

#[test]
fn ensure_rlm_dir_creates_directory() {
    let tmp = TempDir::new().unwrap();
    let cfg = Config::new(tmp.path());
    assert!(!cfg.rlm_dir.exists());
    cfg.ensure_rlm_dir().unwrap();
    assert!(cfg.rlm_dir.exists());
}

#[test]
fn index_exists_returns_false_when_missing() {
    let tmp = TempDir::new().unwrap();
    let cfg = Config::new(tmp.path());
    assert!(!cfg.index_exists());
}

#[test]
fn save_and_load_settings() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = Config::new(tmp.path());

    // Modify settings
    cfg.settings.indexing.max_file_size_mb = TEST_MAX_FILE_SIZE_MB;
    cfg.settings.output.format = "pretty".to_string();
    cfg.settings.quality.log_all_issues = true;

    // Save settings
    cfg.save_settings().unwrap();
    assert!(cfg.config_path.exists());

    // Create new config from same path (should load saved settings)
    let cfg2 = Config::new(tmp.path());
    assert_eq!(
        cfg2.settings.indexing.max_file_size_mb,
        TEST_MAX_FILE_SIZE_MB
    );
    assert_eq!(cfg2.settings.output.format, "pretty");
    assert!(cfg2.settings.quality.log_all_issues);
}

#[test]
fn default_settings() {
    let settings = UserSettings::default();

    // Check indexing defaults
    assert!(settings.indexing.incremental);
    assert_eq!(settings.indexing.max_file_size_mb, DEFAULT_MAX_FILE_SIZE_MB);
    assert!(settings
        .indexing
        .exclude_patterns
        .contains(&"node_modules/".to_string()));
    assert!(settings
        .indexing
        .exclude_patterns
        .contains(&".git/".to_string()));
    assert!(settings
        .indexing
        .exclude_patterns
        .contains(&"target/".to_string()));

    // Check output defaults
    assert_eq!(settings.output.format, "json");

    // Check quality defaults
    assert!(!settings.quality.log_all_issues);
    assert!(settings.quality.log_file.is_none());

    // Check language defaults
    assert!(settings.languages.custom_mappings.is_empty());
}

#[test]
fn is_file_too_large() {
    let tmp = TempDir::new().unwrap();
    let cfg = Config::new(tmp.path());

    // Default max is DEFAULT_MAX_FILE_SIZE_MB
    let max_bytes = u64::from(DEFAULT_MAX_FILE_SIZE_MB) * BYTES_PER_MB;

    assert!(!cfg.is_file_too_large(max_bytes - 1));
    assert!(!cfg.is_file_too_large(max_bytes));
    assert!(cfg.is_file_too_large(max_bytes + 1));
    assert!(cfg.is_file_too_large(100 * BYTES_PER_MB)); // 100 MB
}

#[test]
fn load_invalid_config_uses_defaults() {
    let tmp = TempDir::new().unwrap();
    let rlm_dir = tmp.path().join(".rlm");
    std::fs::create_dir_all(&rlm_dir).unwrap();

    // Write invalid TOML
    let config_path = rlm_dir.join("config.toml");
    std::fs::write(&config_path, "invalid toml {{{{").unwrap();

    // Should fall back to defaults
    let cfg = Config::new(tmp.path());
    assert_eq!(
        cfg.settings.indexing.max_file_size_mb,
        DEFAULT_MAX_FILE_SIZE_MB
    );
    assert!(cfg.settings.indexing.incremental);
}
