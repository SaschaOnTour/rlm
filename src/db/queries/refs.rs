use rusqlite::params;

use crate::error::Result;
use crate::models::chunk::{RefKind, Reference};

use super::super::Database;

/// A reference together with the containing chunk's identifier and its
/// file's path — the shape [`analyze_impact`] and caller-extraction in
/// [`build_callgraph`] need without running N+1 `get_chunk_by_id` /
/// `get_all_files` lookups per ref.
///
/// [`analyze_impact`]: crate::application::symbol::impact::analyze_impact
/// [`build_callgraph`]: crate::application::symbol::callgraph::build_callgraph
pub struct RefWithContext {
    pub reference: Reference,
    /// Identifier of the chunk that contains the reference.
    pub containing_symbol: String,
    /// Project-relative path of the file holding the reference.
    pub file_path: String,
}

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

    /// Batch lookup: for a given target identifier, return each reference
    /// together with the symbol and file path of its containing chunk,
    /// via a single three-way JOIN. Replaces the per-ref
    /// `get_chunk_by_id` + `get_all_files` loop in `analyze_impact` and
    /// the per-ref `get_chunk_by_id` loop in `build_callgraph`.
    pub fn get_refs_with_context(&self, target_ident: &str) -> Result<Vec<RefWithContext>> {
        let mut stmt = self.conn().prepare(
            "SELECT r.id, r.chunk_id, r.target_ident, r.ref_kind, r.line, r.col,
                    c.ident, f.path
             FROM refs r
             JOIN chunks c ON r.chunk_id = c.id
             JOIN files f ON c.file_id = f.id
             WHERE r.target_ident = ?1
             ORDER BY r.line",
        )?;

        let rows = stmt.query_map(params![target_ident], |row| {
            Ok(RefWithContext {
                reference: Reference {
                    id: row.get(0)?,
                    chunk_id: row.get(1)?,
                    target_ident: row.get(2)?,
                    ref_kind: RefKind::parse(row.get::<_, String>(3)?.as_str()),
                    line: row.get(4)?,
                    col: row.get(5)?,
                },
                containing_symbol: row.get(6)?,
                file_path: row.get(7)?,
            })
        })?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
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
