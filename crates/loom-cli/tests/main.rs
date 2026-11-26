//! The integration tests of loom-cli, built as one test binary.
//!
//! Each file in this folder is a module here, so the crate graph links
//! once for all of them instead of once per file (CONTRIBUTING.md,
//! Where tests live).

mod analyze;
mod capture;
mod check;
mod midi_port;
