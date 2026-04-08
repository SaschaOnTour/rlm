use rusqlite::params;

use crate::error::Result;
use crate::models::chunk::{RefKind, Reference};

use super::super::Database;

impl Database {
    /// Insert a reference.
    pub fn insert_ref(&self, reference: &Reference) -> Result<i64> {
        self.conn().execute(
            "INSERT INTO refs (chunk_id, target_ident, ref_kind, line, col) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                reference.chunk_id,
                reference.target_ident,
                reference.ref_kind.as_str(),
                reference.line,
                reference.col,
            ],
        )?;
        Ok(self.conn().last_insert_rowid())
    }

    /// Find all references to a given identifier.
    pub fn get_refs_to(&self, target_ident: &str) -> Result<Vec<Reference>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, chunk_id, target_ident, ref_kind, line, col FROM refs WHERE target_ident = ?1",
        )?;
        Self::map_refs(&mut stmt, params![target_ident])
    }

    /// Get all references from a chunk.
    pub fn get_refs_from_chunk(&self, chunk_id: i64) -> Result<Vec<Reference>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, chunk_id, target_ident, ref_kind, line, col FROM refs WHERE chunk_id = ?1",
        )?;
        Self::map_refs(&mut stmt, params![chunk_id])
    }

    /// Get all references for a file (via its chunks).
    pub fn get_refs_for_file(&self, file_id: i64) -> Result<Vec<Reference>> {
        let mut stmt = self.conn().prepare(
            "SELECT r.id, r.chunk_id, r.target_ident, r.ref_kind, r.line, r.col
             FROM refs r
             JOIN chunks c ON r.chunk_id = c.id
             WHERE c.file_id = ?1
             ORDER BY r.line",
        )?;
        Self::map_refs(&mut stmt, params![file_id])
    }

    fn map_refs(
        stmt: &mut rusqlite::Statement,
        params: impl rusqlite::Params,
    ) -> Result<Vec<Reference>> {
        let rows = stmt.query_map(params, |row| {
            Ok(Reference {
                id: row.get(0)?,
                chunk_id: row.get(1)?,
                target_ident: row.get(2)?,
                ref_kind: RefKind::parse(row.get::<_, String>(3)?.as_str()),
                line: row.get(4)?,
                col: row.get(5)?,
            })
        })?;
        let mut refs = Vec::new();
        for r in rows {
            refs.push(r?);
        }
        Ok(refs)
    }
}
