//! Fuzzes the patch loader (#44): `Patch::from_ron`, then `load_into` an
//! engine with every built-in module, then a few buffers of audio. Any
//! input must give an error or a working engine, never a panic or a hang.

#![no_main]

use libfuzzer_sys::fuzz_target;
use loom_core::{EngineConfig, create_engine};
use loom_patch::Patch;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(patch) = Patch::from_ron(text) else {
        return;
    };
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    if patch.load_into(&mut parts.controller).is_err() {
        return;
    }
    // A patch that loads must also play.
    let (mut left, mut right) = ([0.0f32; 256], [0.0f32; 256]);
    for _ in 0..4 {
        parts.engine.process(&mut left, &mut right);
    }
    assert!(left.iter().chain(&right).all(|s| s.is_finite()), "the output is finite");
});
