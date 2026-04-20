//! Shared fixture for `application::index` companion test files.
//!
//! Used by `mod_tests` / `mod_reindex_tests` / `staleness_tests` to build
//! an indexed tempdir once instead of duplicating the setup in every file.

use crate::application::index::run_index;
use crate::config::Config;
use crate::db::Database;
use std::fs;
use tempfile::TempDir;

/// Write `files` into a fresh tempdir under `src/`, index it, and return
/// the handles. Each `files` tuple is `(relative_name_inside_src, content)`.
pub(crate) fn setup_indexed(files: &[(&str, &str)]) -> (TempDir, Config, Database) {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    for (name, content) in files {
        fs::write(src.join(name), content).unwrap();
    }
    let config = Config::new(tmp.path());
    run_index(&config, None).unwrap();
    let db = Database::open(&config.db_path).unwrap();
    (tmp, config, db)
}
