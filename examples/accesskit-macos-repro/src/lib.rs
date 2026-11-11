//! One window with slider nodes, for showing two gaps in AccessKit's macOS
//! adapter with VoiceOver. See the README beside this crate.
//!
//! [`model`] is the plain part: the sliders, the accessibility tree built
//! from them, and what an accessibility action does to them. [`window`] is
//! the winit window that hands that tree to AccessKit.

pub mod model;
pub mod window;
