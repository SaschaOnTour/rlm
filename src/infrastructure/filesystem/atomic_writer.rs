//! Atomic write primitive: O_EXCL tempfile + rename.
//!
//! Used by every write path that must not leave a partial file behind:
//! `rlm setup` for `settings.json` and `CLAUDE.local.md`, and
//! `application::edit::validator::validate_and_write` for source-file
//! edits. Uses `OpenOptions::create_new` so we never follow a
//! pre-existing symlink or overwrite an attacker-seeded file at the
//! temp path; retries with a monotonic counter suffix if the chosen
//! temp name is already taken; on Windows the rename step removes any
//! existing target first because the platform `rename` will otherwise
//! refuse to overwrite.

use std::path::Path;

pub use crate::error::AtomicWriteError;

/// Upper bound on temp-filename collision retries (pid + nanos + counter).
/// Collisions are effectively impossible within this budget in practice.
const MAX_TEMP_ATTEMPTS: u32 = 128;

/// Prefix used for all atomic-write temp files. Keeping it shared means
/// any cleanup sweep ("delete stale `.rlm_tmp_*`") catches both setup and
/// edit scratch files without bespoke patterns per call site.
const TEMP_PREFIX: &str = ".rlm_tmp";

/// Atomically replace `path` with `content`.
///
/// Creates the parent directory if missing, writes to a temp file in the
/// same directory via `OpenOptions::create_new`, flushes and closes, then
/// renames into place. On Windows the existing target is removed first
/// (see [`replace_file`]).
pub fn write_atomic(path: &Path, content: &[u8]) -> Result<(), AtomicWriteError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let now_nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    for attempt in 0..MAX_TEMP_ATTEMPTS {
        let temp = parent.join(format!(
            "{TEMP_PREFIX}_{}_{now_nanos}_{attempt}",
            std::process::id(),
        ));
        if try_write_once(&temp, path, content)? {
            return Ok(());
        }
    }
    Err(AtomicWriteError::Exhausted {
        attempts: MAX_TEMP_ATTEMPTS,
    })
}

/// One attempt at atomic write. Returns `Ok(true)` on success, `Ok(false)` if
/// the temp name already existed (caller retries), `Err` on any other failure.
fn try_write_once(temp: &Path, target: &Path, content: &[u8]) -> Result<bool, AtomicWriteError> {
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)
    {
        Ok(mut file) => {
            if let Err(e) = file.write_all(content) {
                drop(file);
                let _ = std::fs::remove_file(temp);
                return Err(e.into());
            }
            drop(file);
            if let Err(e) = replace_file(temp, target) {
                let _ = std::fs::remove_file(temp);
                return Err(e.into());
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Cross-platform file replacement: Unix `rename` atomically overwrites,
/// Windows `rename` requires explicit target removal first.
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if to.exists() {
            std::fs::remove_file(to)?;
        }
    }
    std::fs::rename(from, to)
}

#[cfg(test)]
#[path = "atomic_writer_tests.rs"]
mod tests;
