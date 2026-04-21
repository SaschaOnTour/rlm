//! Interface adapters — translate external inputs/outputs to/from application calls.
//!
//! The cross-cutting operation-pipeline middleware (`record_operation`
//! and friends) used to live here under `shared/` but was 0.5.0-moved
//! into `application::middleware` — it's application-layer logic that
//! adapters route through, not an interface-specific concern. `cli/`
//! holds the decomposed `setup/` flow.

pub mod cli;
