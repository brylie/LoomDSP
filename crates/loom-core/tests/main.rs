//! The integration tests of loom-core, built as one test binary.
//!
//! Each file in this folder is a module here, so the crate graph links
//! once for all of them instead of once per file (CONTRIBUTING.md,
//! Where tests live).
//!
//! `displays.rs`, `stereo_no_alloc.rs`, and `transport_no_alloc.rs` stay
//! binaries of their own: each installs a global allocator, and an
//! allocator applies to its whole binary.

mod common;
mod stereo_modules;

mod midi_clock;
mod modulation_non_finite;
mod output_warnings;
mod reach;
mod stereo;
mod transport;
