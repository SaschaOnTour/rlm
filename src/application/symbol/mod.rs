//! Symbol-scoped analyses (refs, signature, callgraph, impact, context,
//! type_info, scope).
//!
//! Slice 3.5 introduces a shared `SymbolQuery` trait that captures the
//! common shape (db + symbol → typed result + file_count for the
//! savings middleware). Slice 3.6 migrates the remaining operations
//! onto the trait.
