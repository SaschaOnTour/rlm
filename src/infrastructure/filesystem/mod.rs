//! Filesystem adapters — primitives shared by every write path.
//!
//! Consolidated in slice 5.2 so the orchestrator-owned copy that lived in
//! `interface::cli::setup::orchestrator` and the ad-hoc inline version that
//! `application::edit::validator::validate_and_write` used converge on a
//! single implementation with O_EXCL open, collision-retry, and a
//! Windows-safe replace step.

pub mod atomic_writer;
