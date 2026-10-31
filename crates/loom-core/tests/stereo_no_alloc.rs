//! NFR-RT-06 for stereo: running per-channel copies, downmixes, and stereo
//! delays, and swapping schedules as chains turn stereo and back, never
//! allocate or free on the audio thread (stereo spec, rule 13).
//!
//! This test binary installs an allocator that aborts if any allocation or
//! deallocation happens inside `assert_no_alloc`. The check is active in
//! debug builds, which is how `cargo test` runs.

mod stereo_modules;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use loom_core::ProbeKind;
use stereo_modules::*;

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

/// Checks stereo never allocates on the audio thread.
#[test]
fn stereo_never_allocates_on_the_audio_thread() {
    let mut parts = engine(128);
    let mut left = vec![0.0; 1024];
    let mut right = vec![0.0; 1024];

    // Control thread: a stereo source, a mono source, a chain that can be
    // fed by either, a stereo loop, downmixed inputs, and an assignment.
    let c = &mut parts.controller;
    let ramp = c.add_module("test.ramp").unwrap();
    let mono = c.add_module("test.mono-ramp").unwrap();
    let pair = c.add_module("test.pair").unwrap();
    let lag = c.add_module("test.lag").unwrap();
    let sum = c.add_module("test.add").unwrap();
    let delay = c.add_module("test.delay1").unwrap();
    let read = c.add_module("test.read").unwrap();
    let split = c.add_module("test.split").unwrap();
    let out = c.add_module("test.out").unwrap();
    c.connect_named(lag, "out", sum, "a").unwrap();
    c.connect_named(sum, "out", delay, "in").unwrap();
    c.connect_named(delay, "out", sum, "b").unwrap();
    c.connect_named(sum, "out", split, "in").unwrap();
    c.connect_named(sum, "out", read, "level").unwrap();
    c.connect_named(pair, "out", read, "gate").unwrap();
    c.connect_named(pair, "out", read, "phase").unwrap();
    c.connect_named(split, "left", out, "in").unwrap();
    c.add_assignment_named(pair, "out", lag, "coef", 0.1).unwrap();
    let probe = c.output_ref(sum, "out").unwrap();
    c.add_probe(probe, ProbeKind::MeterAndScope).unwrap();

    for i in 0..200usize {
        // Control thread: allocation allowed here. Turn the chain stereo,
        // then mono, so copies are created and retired.
        let source = if i % 2 == 0 { ramp } else { mono };
        parts.controller.connect_named(source, "out", lag, "in").unwrap();
        if i % 3 == 0 {
            parts
                .controller
                .set_param_named(lag, "coef", 0.1 + (i % 5) as f32 * 0.1)
                .unwrap();
            parts.controller.reset_module(lag).unwrap();
        }
        let report = parts.controller.commit().unwrap();
        assert_eq!(report.runs_per_channel(lag), i % 2 == 0);
        parts.telemetry.meters().for_each(drop);
        parts.telemetry.scope().for_each(drop);
        let frames = [1, 32, 128, 129, 1024][i % 5];

        // Audio thread: no allocation allowed.
        assert_no_alloc(|| {
            parts.engine.process(&mut left[..frames], &mut right[..frames]);
        });
    }
}
