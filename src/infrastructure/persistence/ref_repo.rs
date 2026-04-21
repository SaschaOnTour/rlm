//! Storage operations for `Reference`.

use crate::db::Database;
use crate::domain::chunk::Reference;
use crate::error::Result;

/// Read and write access to indexed references (calls, imports, type uses).
pub trait RefRepo {
    fn insert_ref(&self, reference: &Reference) -> Result<i64>;
    fn get_refs_to(&self, target_ident: &str) -> Result<Vec<Reference>>;
    fn get_refs_from_chunk(&self, chunk_id: i64) -> Result<Vec<Reference>>;
    fn get_refs_for_file(&self, file_id: i64) -> Result<Vec<Reference>>;
}

impl RefRepo for Database {
    fn insert_ref(&self, reference: &Reference) -> Result<i64> {
        Database::insert_ref(self, reference)
    }

    fn get_refs_to(&self, target_ident: &str) -> Result<Vec<Reference>> {
        Database::get_refs_to(self, target_ident)
    }

    fn get_refs_from_chunk(&self, chunk_id: i64) -> Result<Vec<Reference>> {
        Database::get_refs_from_chunk(self, chunk_id)
    }

    fn get_refs_for_file(&self, file_id: i64) -> Result<Vec<Reference>> {
        Database::get_refs_for_file(self, file_id)
    }
}
