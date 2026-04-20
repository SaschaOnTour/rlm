//! Storage operations for `Chunk`.

use crate::db::Database;
use crate::domain::chunk::Chunk;
use crate::error::Result;

/// Read and write access to indexed chunks.
pub trait ChunkRepo {
    fn insert_chunk(&self, chunk: &Chunk) -> Result<i64>;
    fn delete_chunks_for_file(&self, file_id: i64) -> Result<()>;
    fn get_chunks_for_file(&self, file_id: i64) -> Result<Vec<Chunk>>;
    fn get_chunks_by_ident(&self, ident: &str) -> Result<Vec<Chunk>>;
    fn get_chunk_by_id(&self, id: i64) -> Result<Option<Chunk>>;
    fn get_all_chunks(&self) -> Result<Vec<Chunk>>;
}

impl ChunkRepo for Database {
    fn insert_chunk(&self, chunk: &Chunk) -> Result<i64> {
        Database::insert_chunk(self, chunk)
    }

    fn delete_chunks_for_file(&self, file_id: i64) -> Result<()> {
        Database::delete_chunks_for_file(self, file_id)
    }

    fn get_chunks_for_file(&self, file_id: i64) -> Result<Vec<Chunk>> {
        Database::get_chunks_for_file(self, file_id)
    }

    fn get_chunks_by_ident(&self, ident: &str) -> Result<Vec<Chunk>> {
        Database::get_chunks_by_ident(self, ident)
    }

    fn get_chunk_by_id(&self, id: i64) -> Result<Option<Chunk>> {
        Database::get_chunk_by_id(self, id)
    }

    fn get_all_chunks(&self) -> Result<Vec<Chunk>> {
        Database::get_all_chunks(self)
    }
}
