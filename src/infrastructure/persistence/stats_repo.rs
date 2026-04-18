//! Index-wide statistics and integrity checks.

use crate::db::queries::{IndexStats, VerifyReport};
use crate::db::Database;
use crate::error::Result;

/// Aggregate queries across the whole index (counts, languages, orphan checks).
pub trait StatsRepo {
    fn stats(&self) -> Result<IndexStats>;
    fn verify_integrity(&self) -> Result<VerifyReport>;
    fn fix_orphans(&self) -> Result<(u64, u64)>;
}

impl StatsRepo for Database {
    fn stats(&self) -> Result<IndexStats> {
        Database::stats(self)
    }

    fn verify_integrity(&self) -> Result<VerifyReport> {
        Database::verify_integrity(self)
    }

    fn fix_orphans(&self) -> Result<(u64, u64)> {
        Database::fix_orphans(self)
    }
}
