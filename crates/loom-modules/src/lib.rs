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

pub mod clock;
pub mod display;
pub mod env;
pub mod filter;
pub mod io;
pub mod level;
pub mod lfo;
pub mod logic;
pub mod math;
pub mod midi;
pub mod mix;
pub mod osc;
pub mod phase;
pub mod rand;
#[cfg(feature = "test-modules")]
pub mod test_modules;
pub mod time;
pub mod transport;
pub mod ui;

/// Every built-in module.
pub static ALL: &[&ModuleDescriptor] = &[
    &phase::PHASE,
    &phase::PHASE_TRIGGER,
    &phase::WRAP,
    &phase::SCALE,
    &lfo::LFO,
    &osc::SINE,
    &osc::SAW,
    &osc::PULSE,
    &osc::TRIANGLE,
    &osc::NOISE,
    &filter::SVF,
    &filter::ONEPOLE,
    &filter::LADDER,
    &env::ENVELOPE,
    &math::ADD,
    &math::MUL,
    &math::SUB,
    &math::DIV,
    &math::CONSTANT,
    &math::PITCH_CONSTANT,
    &math::CONVERT,
    &level::AMP,
    &level::CONSTANT,
    &level::CLAMP,
    &level::SCALE,
    &mix::MIXER,
    &mix::CROSSFADE,
    &mix::JOIN,
    &mix::SPLIT,
    &io::UNIT_DELAY,
    &io::AUDIO_OUT,
    &io::AUDIO_IN,
    &midi::NOTE_IN,
    &midi::CONTROLLER_IN,
    &display::METER,
    &display::SCOPE,
    &clock::CLOCK,
    &transport::TRANSPORT,
    &logic::DIVIDE,
    &logic::COMPARE,
    &logic::LOGIC,
    &logic::MEMORY,
    &rand::SAMPLE_HOLD,
    &level::SLEW,
    &ui::BUTTON,
    &time::DELAY,
    &time::ALLPASS,
];

// Every built-in port and parameter has a label and a description, or this
// crate does not compile.
const _: () = loom_core::assert_documented(ALL);

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
mod tests;
