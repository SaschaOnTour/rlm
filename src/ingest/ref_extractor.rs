#[cfg(test)]
use crate::domain::chunk::{Chunk, Reference};
#[cfg(test)]
use crate::error::Result;
#[cfg(test)]
use crate::ingest::dispatcher::Dispatcher;

#[cfg(test)]
/// High-level reference extraction that delegates to the dispatcher.
pub fn extract_references(
    dispatcher: &Dispatcher,
    lang: &str,
    source: &str,
    chunks: &[Chunk],
) -> Result<Vec<Reference>> {
    dispatcher.extract_refs(lang, source, chunks)
}

#[cfg(test)]
#[path = "ref_extractor_tests.rs"]
mod tests;
