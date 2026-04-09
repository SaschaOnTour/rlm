/// SQL statements for creating the rlm schema.
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

-- Token savings tracking
CREATE TABLE IF NOT EXISTS savings (
    id INTEGER PRIMARY KEY,
    command TEXT NOT NULL,
    output_tokens INTEGER NOT NULL,
    alternative_tokens INTEGER NOT NULL,
    files_touched INTEGER NOT NULL DEFAULT 0,
    rlm_input_tokens INTEGER NOT NULL DEFAULT 0,
    alt_input_tokens INTEGER NOT NULL DEFAULT 0,
    rlm_calls INTEGER NOT NULL DEFAULT 1,
    alt_calls INTEGER NOT NULL DEFAULT 1,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_savings_command ON savings(command);
CREATE INDEX IF NOT EXISTS idx_savings_created ON savings(created_at);
";

/// Migration for existing databases that lack the V2 savings columns.
///
/// Each statement is run individually; "duplicate column" errors are ignored
/// for idempotency.
pub const MIGRATE_SAVINGS_V2: &str = "\
ALTER TABLE savings ADD COLUMN rlm_input_tokens INTEGER NOT NULL DEFAULT 0;\
ALTER TABLE savings ADD COLUMN alt_input_tokens INTEGER NOT NULL DEFAULT 0;\
ALTER TABLE savings ADD COLUMN rlm_calls INTEGER NOT NULL DEFAULT 1;\
ALTER TABLE savings ADD COLUMN alt_calls INTEGER NOT NULL DEFAULT 1;\
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

    #[test]
    fn savings_v2_migration_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_SCHEMA).unwrap();
        // Run migration twice — second run hits "duplicate column" but must not panic.
        for _ in 0..2 {
            for sql in MIGRATE_SAVINGS_V2.split(';') {
                let trimmed = sql.trim();
                if !trimmed.is_empty() {
                    let _ = conn.execute(trimmed, []);
                }
            }
        }
        // Verify new columns exist
        conn.execute(
            "INSERT INTO savings (command, output_tokens, alternative_tokens, rlm_input_tokens, alt_input_tokens, rlm_calls, alt_calls) \
             VALUES ('test', 10, 20, 30, 60, 1, 2)",
            [],
        )
        .unwrap();
    }
}
