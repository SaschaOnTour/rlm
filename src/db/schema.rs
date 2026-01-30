/// SQL statements for creating the rlm-cli schema.
pub const CREATE_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    hash TEXT NOT NULL,
    lang TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    parse_quality TEXT DEFAULT 'complete',
    indexed_at TEXT DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS chunks (
    id INTEGER PRIMARY KEY,
    file_id INTEGER REFERENCES files(id) ON DELETE CASCADE,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    kind TEXT NOT NULL,
    ident TEXT NOT NULL,
    parent TEXT,
    signature TEXT,
    visibility TEXT,
    ui_ctx TEXT,
    doc_comment TEXT,
    attributes TEXT,
    content TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS refs (
    id INTEGER PRIMARY KEY,
    chunk_id INTEGER REFERENCES chunks(id) ON DELETE CASCADE,
    target_ident TEXT NOT NULL,
    ref_kind TEXT NOT NULL,
    line INTEGER NOT NULL,
    col INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chunks_file_id ON chunks(file_id);
CREATE INDEX IF NOT EXISTS idx_chunks_ident ON chunks(ident);
CREATE INDEX IF NOT EXISTS idx_chunks_parent ON chunks(parent);
CREATE INDEX IF NOT EXISTS idx_chunks_kind ON chunks(kind);
-- PERF: Compound index for queries filtering by file_id and kind together
CREATE INDEX IF NOT EXISTS idx_chunks_file_kind ON chunks(file_id, kind);
CREATE INDEX IF NOT EXISTS idx_refs_target ON refs(target_ident);
CREATE INDEX IF NOT EXISTS idx_refs_chunk_id ON refs(chunk_id);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    ident, signature, doc_comment, content,
    content='chunks', content_rowid='id'
);

-- Triggers to keep FTS5 in sync
CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, ident, signature, doc_comment, content)
    VALUES (new.id, new.ident, COALESCE(new.signature, ''), COALESCE(new.doc_comment, ''), new.content);
END;

CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, ident, signature, doc_comment, content)
    VALUES ('delete', old.id, old.ident, COALESCE(old.signature, ''), COALESCE(old.doc_comment, ''), old.content);
END;

CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, ident, signature, doc_comment, content)
    VALUES ('delete', old.id, old.ident, COALESCE(old.signature, ''), COALESCE(old.doc_comment, ''), old.content);
    INSERT INTO chunks_fts(rowid, ident, signature, doc_comment, content)
    VALUES (new.id, new.ident, COALESCE(new.signature, ''), COALESCE(new.doc_comment, ''), new.content);
END;
";

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn schema_creates_without_error() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_SCHEMA).unwrap();
    }

    #[test]
    fn schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_SCHEMA).unwrap();
        conn.execute_batch(CREATE_SCHEMA).unwrap();
    }
}
