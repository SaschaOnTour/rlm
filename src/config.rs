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
    /// Write-side post-edit checks (cargo check / tsc / etc.).
    pub edit: EditSettings,
}

/// Post-write validation settings. Controls the native-checker pass
/// that runs after every `rlm replace/insert/delete` to catch
/// name-resolution and type errors that tree-sitter (Syntax Guard)
/// can't see.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditSettings {
    /// When true, run the language's native checker (cargo check for
    /// Rust) after every write and include the result in the write
    /// response's `build` field. Default: true.
    pub native_check: bool,
    /// Timeout (seconds) for the native checker. First-compile runs
    /// may exceed this; on timeout the response reports
    /// `build.errors[0].message = "timed out after Ns"` rather than
    /// hanging indefinitely. Default: 10.
    pub native_check_timeout_secs: u64,
}

impl Default for EditSettings {
    fn default() -> Self {
        Self {
            native_check: true,
            native_check_timeout_secs: 10,
        }
    }
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
    /// Output format: "json" (default), "pretty", or "toon".
    pub format: String,
}

impl Default for OutputSettings {
    fn default() -> Self {
        Self {
            format: "json".into(),
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

    /// Get the effective quality log path.
    ///
    /// Rejects custom paths that are absolute or contain `..` to prevent
    /// writing outside the `.rlm/` directory.
    #[must_use]
    pub fn get_quality_log_path(&self) -> PathBuf {
        if let Some(custom) = &self.settings.quality.log_file {
            match crate::error::validate_relative_path(custom, &self.rlm_dir) {
                Ok(path) => path,
                Err(_) => self.quality_log_path.clone(), // fallback to default
            }
        } else {
            self.quality_log_path.clone()
        }
    }
}

#[cfg(test)]
/// Maximum file size in MB used in test assertions.
const TEST_MAX_FILE_SIZE_MB: u32 = 25;

#[cfg(test)]
/// Default maximum file size in MB from `IndexingSettings::default()`.
const DEFAULT_MAX_FILE_SIZE_MB: u32 = 10;

#[cfg(test)]
/// Bytes per megabyte (1024 * 1024).
const BYTES_PER_MB: u64 = 1024 * 1024;

#[cfg(test)]
impl Config {
    fn save_settings(&self) -> Result<()> {
        self.ensure_rlm_dir()?;
        let content = toml::to_string_pretty(&self.settings)
            .map_err(|e| RlmError::Config(format!("failed to serialize settings: {e}")))?;
        std::fs::write(&self.config_path, content)?;
        Ok(())
    }

    #[must_use]
    fn relative_path(&self, abs: &Path) -> String {
        abs.strip_prefix(&self.project_root)
            .unwrap_or(abs)
            .to_string_lossy()
            .replace('\\', "/")
    }

    #[must_use]
    fn should_exclude(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();
        for pattern in &self.settings.indexing.exclude_patterns {
            let pattern = pattern.trim_end_matches('/');
            if path_str.contains(pattern) {
                return true;
            }
        }
        false
    }

    #[must_use]
    fn is_file_too_large(&self, size_bytes: u64) -> bool {
        let max_bytes = u64::from(self.settings.indexing.max_file_size_mb) * BYTES_PER_MB;
        size_bytes > max_bytes
    }
}

#[cfg(test)]
#[path = "config_path_tests.rs"]
mod path_tests;
#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
