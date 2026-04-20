mod chunks;
mod files;
mod refs;
mod savings;
mod search;
mod stats;

pub use files::IndexedFileMeta;
pub use refs::RefWithContext;
pub use savings::SavingsQueryRow;
pub use stats::{IndexStats, VerifyReport};

#[cfg(test)]
#[path = "mod_chunk_tests.rs"]
mod chunk_tests;
#[cfg(test)]
#[path = "mod_fixtures_tests.rs"]
mod test_fixtures;
#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
