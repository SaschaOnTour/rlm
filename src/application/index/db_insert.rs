//! Database insertion: chunks and references with sorted lookup.

use crate::db::Database;
use crate::error::Result;

/// Insert chunks into the DB and return them sorted by `start_line` for fast ref lookup.
pub(super) fn insert_chunks(
    db: &Database,
    chunks: Vec<crate::models::chunk::Chunk>,
) -> Result<Vec<crate::models::chunk::Chunk>> {
    let mut inserted = Vec::with_capacity(chunks.len());
    for mut chunk in chunks {
        let cid = db.insert_chunk(&chunk)?;
        chunk.id = cid;
        inserted.push(chunk);
    }
    inserted.sort_by_key(|c| c.start_line);
    Ok(inserted)
}

/// Resolve the chunk ID for a reference using binary search over sorted chunks.
fn resolve_ref_chunk_id(inserted_chunks: &[crate::models::chunk::Chunk], line: u32) -> i64 {
    let idx = inserted_chunks.partition_point(|c| c.start_line <= line);
    if idx > 0 {
        inserted_chunks[..idx]
            .iter()
            .rev()
            .find(|c| line <= c.end_line)
            .map_or(0, |c| c.id)
    } else {
        0
    }
}

/// Insert references into the DB, resolving chunk IDs via binary search.
pub(super) fn insert_refs(
    db: &Database,
    refs: Vec<crate::models::chunk::Reference>,
    inserted_chunks: &[crate::models::chunk::Chunk],
) -> Result<usize> {
    let mut count = 0;
    for mut reference in refs {
        if reference.chunk_id == 0 {
            reference.chunk_id = resolve_ref_chunk_id(inserted_chunks, reference.line);
        }
        if reference.chunk_id > 0 {
            db.insert_ref(&reference)?;
            count += 1;
        }
    }
    Ok(count)
}
