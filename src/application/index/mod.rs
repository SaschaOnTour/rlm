//! Indexing pipeline (scan → parse → insert stages).
//!
//! Slice 3.8 migrates `crate::indexer` into staged sub-modules so that
//! full index and single-file reindex share the same pipeline with
//! different stage inputs.
