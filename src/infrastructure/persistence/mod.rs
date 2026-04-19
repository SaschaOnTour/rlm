//! Persistence abstractions.
//!
//! Each `*Repo` trait defines the contract for a single entity's storage
//! operations. The current `db::Database` (rusqlite-backed SQLite) is the
//! only implementation today; later slices may add in-memory fakes to
//! decouple tests from the SQL layer.
//!
//! Traits use the same types `Database` currently exposes (legacy
//! `models::*` + `db::queries::*` DTOs) to keep this slice strictly
//! additive. A later slice migrates trait signatures to domain entities
//! along with the body migration.

pub mod chunk_repo;
pub mod file_repo;
pub mod migrations;
pub mod ref_repo;
pub mod savings_repo;
pub mod search_repo;
pub mod stats_repo;

pub use chunk_repo::ChunkRepo;
pub use file_repo::FileRepo;
pub use ref_repo::RefRepo;
pub use savings_repo::SavingsRepo;
pub use search_repo::SearchRepo;
pub use stats_repo::StatsRepo;
