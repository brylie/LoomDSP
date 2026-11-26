//! The integration tests of loom-app, built as one test binary.
//!
//! Each file in this folder is a module here, so the crate graph links
//! once for all of them instead of once per file (CONTRIBUTING.md,
//! Where tests live).

mod dump_ui;
mod layout_text;
mod purity;
mod screenshot;
mod ui_smoke;
