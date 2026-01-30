use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::error::Result;

/// Compute SHA-256 hash of a file's contents using streaming.
/// PERF: Uses 8KB buffer instead of reading entire file into memory.
pub fn hash_file(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::with_capacity(8192, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

/// Compute SHA-256 hash of a byte slice.
#[must_use]
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn hash_bytes_deterministic() {
        let h1 = hash_bytes(b"hello world");
        let h2 = hash_bytes(b"hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn hash_bytes_different_for_different_input() {
        let h1 = hash_bytes(b"hello");
        let h2 = hash_bytes(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_file_works() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "test content").unwrap();
        let h = hash_file(tmp.path()).unwrap();
        assert_eq!(h.len(), 64);
        assert_eq!(h, hash_bytes(b"test content"));
    }
}
