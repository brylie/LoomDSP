//! Built-in DSP modules for LoomDSP.
//!
//! The Phase 0 set is small on purpose: enough to build a playable synth from
//! primitives and to test feedback loops. Each module follows the same rules
//! as third-party modules (FR-SDK-01).
//!
//! Module IDs are stable. Never rename a released ID, port, or parameter.
//! Add a new version instead.

#![warn(missing_docs)]

use loom_core::{LoomError, ModuleDescriptor, Registry};
use std::sync::Arc;

pub mod env;
pub mod filter;
pub mod io;
pub mod math;
pub mod midi;
pub mod osc;
pub mod phase;

/// Every built-in module.
pub static ALL: &[&ModuleDescriptor] = &[
    &phase::PHASE,
    &osc::SINE,
    &osc::SAW,
    &osc::NOISE,
    &filter::SVF,
    &env::ADSR,
    &math::ADD,
    &math::MUL,
    &math::CONSTANT,
    &io::UNIT_DELAY,
    &io::AUDIO_OUT,
    &midi::NOTE_IN,
];

/// Adds every built-in module to a registry.
pub fn register_all(registry: &mut Registry) -> Result<(), LoomError> {
    for descriptor in ALL {
        registry.register(descriptor)?;
    }
    Ok(())
}

/// A registry with every built-in module.
pub fn registry() -> Arc<Registry> {
    let mut registry = Registry::new();
    register_all(&mut registry).expect("built-in modules are valid and unique");
    Arc::new(registry)
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_modules_register() {
        assert_eq!(super::registry().len(), super::ALL.len());
    }
}
