mod chunks;
pub mod files;
mod refs;
mod savings;
mod search;
mod stats;

pub use files::IndexedFileMeta;
pub use savings::SavingsQueryRow;
pub use stats::{IndexStats, VerifyReport};

#[cfg(test)]
mod tests {
    use crate::db::Database;
    use crate::models::chunk::{Chunk, ChunkKind, RefKind, Reference};
    use crate::models::file::FileRecord;

    const SAMPLE_FILE_SIZE: u64 = 1024;
    const SAMPLE_END_LINE: u32 = 10;
    const SAMPLE_END_BYTE: u32 = 200;
    const UPDATED_FILE_SIZE: u64 = 2048;
    const FTS_SEARCH_LIMIT: usize = 10;
    const REF_COL: u32 = 4;

    fn test_db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn sample_file() -> FileRecord {
        FileRecord::new(
            "src/main.rs".into(),
            "abc123".into(),
            "rust".into(),
            SAMPLE_FILE_SIZE,
        )
    }

    fn sample_chunk(file_id: i64) -> Chunk {
        Chunk {
            id: 0,
            file_id,
            start_line: 1,
            end_line: SAMPLE_END_LINE,
            start_byte: 0,
            end_byte: SAMPLE_END_BYTE,
            kind: ChunkKind::Function,
            ident: "main".into(),
            parent: None,
            signature: Some("fn main()".into()),
            visibility: Some("pub".into()),
            ui_ctx: None,
            doc_comment: None,
            attributes: None,
            content: "fn main() {\n    println!(\"hello\");\n}".into(),
        }
    }

    #[test]
    fn upsert_file_and_retrieve() {
        let db = test_db();
        let f = sample_file();
        let id = db.upsert_file(&f).unwrap();
        assert!(id > 0);
        let got = db.get_file_by_path("src/main.rs").unwrap().unwrap();
        assert_eq!(got.hash, "abc123");
    }

    #[test]
    fn upsert_file_updates_existing() {
        let db = test_db();
        let f = sample_file();
        db.upsert_file(&f).unwrap();
        let f2 = FileRecord::new(
            "src/main.rs".into(),
            "def456".into(),
            "rust".into(),
            UPDATED_FILE_SIZE,
        );
        db.upsert_file(&f2).unwrap();
        let got = db.get_file_by_path("src/main.rs").unwrap().unwrap();
        assert_eq!(got.hash, "def456");
    }

    #[test]
    fn insert_chunk_and_retrieve() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        let cid = db.insert_chunk(&c).unwrap();
        assert!(cid > 0);
        let chunks = db.get_chunks_for_file(fid).unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ident, "main");
    }

    #[test]
    fn search_fts_finds_content() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        db.insert_chunk(&c).unwrap();
        let results = db.search_fts("main", FTS_SEARCH_LIMIT).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].ident, "main");
    }

    #[test]
    fn insert_ref_and_find() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        let cid = db.insert_chunk(&c).unwrap();
        let r = Reference {
            id: 0,
            chunk_id: cid,
            target_ident: "println".into(),
            ref_kind: RefKind::Call,
            line: 2,
            col: REF_COL,
        };
        db.insert_ref(&r).unwrap();
        let refs = db.get_refs_to("println").unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].ref_kind, RefKind::Call);
    }

    #[test]
    fn stats_returns_counts() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        db.insert_chunk(&c).unwrap();
        let stats = db.stats().unwrap();
        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.chunk_count, 1);
        assert_eq!(stats.languages.len(), 1);
    }

    #[test]
    fn delete_file_cascades() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        db.insert_chunk(&c).unwrap();
        db.delete_file(fid).unwrap();
        let files = db.get_all_files().unwrap();
        assert!(files.is_empty());
        let chunks = db.get_all_chunks().unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn get_chunks_by_ident_works() {
        let db = test_db();
        let f = sample_file();
        let fid = db.upsert_file(&f).unwrap();
        let c = sample_chunk(fid);
        db.insert_chunk(&c).unwrap();
        let chunks = db.get_chunks_by_ident("main").unwrap();
        assert_eq!(chunks.len(), 1);
    }
}
