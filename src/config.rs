use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, RlmError};

/// Default directory name for rlm index data.
const RLM_DIR: &str = ".rlm";
/// Default database filename.
const DB_FILE: &str = "index.db";
/// Config filename.
const CONFIG_FILE: &str = "config.toml";
/// Quality issues log filename.
const QUALITY_LOG_FILE: &str = "quality-issues.log";

/// Project-level configuration resolved from the working directory.
#[derive(Debug, Clone)]
pub struct Config {
    /// Root directory of the project being indexed.
    pub project_root: PathBuf,
    /// Path to the `.rlm/` directory.
    pub rlm_dir: PathBuf,
    /// Path to the `SQLite` database.
    pub db_path: PathBuf,
    /// Path to the config file.
    pub config_path: PathBuf,
    /// Path to the quality issues log.
    pub quality_log_path: PathBuf,
    /// User settings loaded from config.toml.
    pub settings: UserSettings,
}

/// User-configurable settings from .rlm/config.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UserSettings {
    /// Indexing configuration.
    pub indexing: IndexingSettings,
    /// Output configuration.
    pub output: OutputSettings,
    /// Quality logging configuration.
    pub quality: QualitySettings,
    /// Custom language mappings.
    pub languages: LanguageSettings,
}

/// Indexing-related settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexingSettings {
    /// Patterns to exclude from indexing (glob patterns).
    pub exclude_patterns: Vec<String>,
    /// Maximum file size in MB to index (files larger are skipped).
    pub max_file_size_mb: u32,
    /// Whether to use incremental indexing.
    pub incremental: bool,
}

impl Default for IndexingSettings {
    fn default() -> Self {
        Self {
            exclude_patterns: vec![
                "node_modules/".into(),
                ".git/".into(),
                "target/".into(),
                "dist/".into(),
                "__pycache__/".into(),
                ".venv/".into(),
                "vendor/".into(),
            ],
            max_file_size_mb: 10,
            incremental: true,
        }
    }
}

/// Output-related settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputSettings {
    /// Output format: "minified" (default), "pretty", or "jsonl".
    pub format: String,
    /// Whether to include token estimates in output.
    pub include_tokens: bool,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            format: "minified".into(),
            include_tokens: true,
        }
    }
}

/// Quality logging settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct QualitySettings {
    /// Whether to log all issues (including known ones).
    pub log_all_issues: bool,
    /// Custom log file path (relative to .rlm/).
    pub log_file: Option<String>,
}

/// Custom language mapping settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LanguageSettings {
    /// Custom extension to language mappings (e.g., {".tsx": "typescript"}).
    pub custom_mappings: std::collections::HashMap<String, String>,
}

impl Config {
    /// Create config for a given project root.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        let project_root = project_root.into();
        let rlm_dir = project_root.join(RLM_DIR);
        let db_path = rlm_dir.join(DB_FILE);
        let config_path = rlm_dir.join(CONFIG_FILE);
        let quality_log_path = rlm_dir.join(QUALITY_LOG_FILE);

        // Try to load settings from config.toml
        let settings = Self::load_settings(&config_path).unwrap_or_default();

        Self {
            project_root,
            rlm_dir,
            db_path,
            config_path,
            quality_log_path,
            settings,
        }
    }

    /// Create config from the current working directory.
    pub fn from_cwd() -> Result<Self> {
        let cwd = std::env::current_dir()
            .map_err(|e| RlmError::Config(format!("cannot get cwd: {e}")))?;
        Ok(Self::new(cwd))
    }

    /// Load settings from config.toml if it exists.
    fn load_settings(config_path: &Path) -> Option<UserSettings> {
        if !config_path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(config_path).ok()?;
        toml::from_str(&content).ok()
    }

    /// Save current settings to config.toml.
    pub fn save_settings(&self) -> Result<()> {
        self.ensure_rlm_dir()?;
        let content = toml::to_string_pretty(&self.settings)
            .map_err(|e| RlmError::Config(format!("failed to serialize settings: {e}")))?;
        std::fs::write(&self.config_path, content)?;
        Ok(())
    }

    /// Ensure the `.rlm/` directory exists.
    pub fn ensure_rlm_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.rlm_dir)?;
        Ok(())
    }

    /// Check whether the index database exists.
    #[must_use]
    pub fn index_exists(&self) -> bool {
        self.db_path.exists()
    }

    /// Convert an absolute path to a project-relative path string.
    #[must_use]
    pub fn relative_path(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.project_root)
            .unwrap_or(abs)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Check if a path should be excluded based on settings.
    #[must_use]
    pub fn should_exclude(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        for pattern in &self.settings.indexing.exclude_patterns {
            // Simple glob matching for common patterns
            let pattern = pattern.trim_end_matches('/');
            if path_str.contains(pattern) {
                return true;
            }
        }
        false
    }

    /// Check if a file is too large to index based on settings.
    #[must_use]
    pub fn is_file_too_large(&self, size_bytes: u64) -> bool {
        let max_bytes = u64::from(self.settings.indexing.max_file_size_mb) * 1024 * 1024;
        size_bytes > max_bytes
    }

    /// Get the effective quality log path.
    #[must_use]
    pub fn get_quality_log_path(&self) -> PathBuf {
        if let Some(custom) = &self.settings.quality.log_file {
            self.rlm_dir.join(custom)
        } else {
            self.quality_log_path.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn config_new_sets_paths() {
        let cfg = Config::new("/tmp/project");
        assert_eq!(cfg.project_root, PathBuf::from("/tmp/project"));
        assert_eq!(cfg.rlm_dir, PathBuf::from("/tmp/project/.rlm"));
        assert_eq!(cfg.db_path, PathBuf::from("/tmp/project/.rlm/index.db"));
    }

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
    fn relative_path_strips_prefix() {
        let cfg = Config::new("/tmp/project");
        let rel = cfg.relative_path(Path::new("/tmp/project/src/main.rs"));
        assert_eq!(rel, "src/main.rs");
    }

    #[test]
    fn save_and_load_settings() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::new(tmp.path());

        // Modify settings
        cfg.settings.indexing.max_file_size_mb = 25;
        cfg.settings.output.format = "pretty".to_string();
        cfg.settings.quality.log_all_issues = true;

        // Save settings
        cfg.save_settings().unwrap();
        assert!(cfg.config_path.exists());

        // Create new config from same path (should load saved settings)
        let cfg2 = Config::new(tmp.path());
        assert_eq!(cfg2.settings.indexing.max_file_size_mb, 25);
        assert_eq!(cfg2.settings.output.format, "pretty");
        assert!(cfg2.settings.quality.log_all_issues);
    }

    #[test]
    fn default_settings() {
        let settings = UserSettings::default();

        // Check indexing defaults
        assert!(settings.indexing.incremental);
        assert_eq!(settings.indexing.max_file_size_mb, 10);
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
        assert_eq!(settings.output.format, "minified");
        assert!(settings.output.include_tokens);

        // Check quality defaults
        assert!(!settings.quality.log_all_issues);
        assert!(settings.quality.log_file.is_none());

        // Check language defaults
        assert!(settings.languages.custom_mappings.is_empty());
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
    fn is_file_too_large() {
        let cfg = Config::new("/tmp/project");

        // Default max is 10 MB
        let max_bytes = 10 * 1024 * 1024;

        assert!(!cfg.is_file_too_large(max_bytes - 1));
        assert!(!cfg.is_file_too_large(max_bytes));
        assert!(cfg.is_file_too_large(max_bytes + 1));
        assert!(cfg.is_file_too_large(100 * 1024 * 1024)); // 100 MB
    }

    #[test]
    fn custom_quality_log_path() {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::new(tmp.path());

        // Default path
        assert_eq!(cfg.get_quality_log_path(), cfg.quality_log_path);

        // Custom path
        cfg.settings.quality.log_file = Some("custom-quality.log".to_string());
        let expected = cfg.rlm_dir.join("custom-quality.log");
        assert_eq!(cfg.get_quality_log_path(), expected);
    }

    #[test]
    fn config_path_normalization() {
        let cfg = Config::new("/tmp/project");

        // Windows-style path should be normalized
        let rel = cfg.relative_path(Path::new("/tmp/project/src\\nested\\file.rs"));
        assert!(!rel.contains('\\'));
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
        assert_eq!(cfg.settings.indexing.max_file_size_mb, 10);
        assert!(cfg.settings.indexing.incremental);
    }
}
