//! Storage operations for savings accounting.

use crate::db::queries::SavingsQueryRow;
use crate::db::Database;
use crate::domain::savings::SavingsEntry;
use crate::error::Result;

/// Write and aggregate savings entries.
pub trait SavingsRepo {
    /// Legacy recording API — output/alternative tokens only.
    fn record_savings(
        &self,
        command: &str,
        output_tokens: u64,
        alternative_tokens: u64,
        files_touched: u64,
    ) -> Result<()>;

    /// Full V2 entry: writes all eight columns from `entry`.
    fn record_savings_v2(&self, entry: &SavingsEntry) -> Result<()>;

    fn get_savings_by_command(&self, since: Option<&str>) -> Result<Vec<SavingsQueryRow>>;

    /// Total bytes and file count under an optional path prefix.
    fn get_scoped_file_stats(&self, path_prefix: Option<&str>) -> Result<(u64, u64)>;

    /// Total bytes of files involved in a symbol (defs + refs).
    fn get_symbol_file_sizes(&self, symbol: &str) -> Result<u64>;
}

impl SavingsRepo for Database {
    fn record_savings(
        &self,
        command: &str,
        output_tokens: u64,
        alternative_tokens: u64,
        files_touched: u64,
    ) -> Result<()> {
        Database::record_savings(
            self,
            command,
            output_tokens,
            alternative_tokens,
            files_touched,
        )
    }

    fn record_savings_v2(&self, entry: &SavingsEntry) -> Result<()> {
        Database::record_savings_v2(self, entry)
    }

    fn get_savings_by_command(&self, since: Option<&str>) -> Result<Vec<SavingsQueryRow>> {
        Database::get_savings_by_command(self, since)
    }

    fn get_scoped_file_stats(&self, path_prefix: Option<&str>) -> Result<(u64, u64)> {
        Database::get_scoped_file_stats(self, path_prefix)
    }

    fn get_symbol_file_sizes(&self, symbol: &str) -> Result<u64> {
        Database::get_symbol_file_sizes(self, symbol)
    }
}
