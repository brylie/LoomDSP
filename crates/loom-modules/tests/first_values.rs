//! A module added with values other than its built-in ones starts at them,
//! with no glide (live patching spec, rule 7, issue #230). The controller
//! holds a new module until the commit, sets its values directly, and calls
//! `reset()` once before it goes to the engine. Paste, duplicate, undo of a
//! delete, and loading a patch all add modules this way.

use loom_core::{EngineConfig, EngineParts, OutputSafety, ProbeKind, create_engine};
use loom_modules::filter::SVF;
use loom_test::{Harness, Signal};

const SAMPLE_RATE: f32 = 48_000.0;

/// Frames rendered after the commit.
const FRAMES: usize = 1_024;

/// An engine with every built-in module and no output safety.
fn engine() -> EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        scope_capacity: 4_096,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    create_engine(loom_modules::registry(), config)
}

/// Host buffer sizes the tests run at (rule 7 asks for these).
const BLOCKS: [usize; 3] = [1, 77, 512];

/// Runs the engine for `frames` frames in buffers of up to `block`.
fn run(parts: &mut EngineParts, frames: usize, block: usize) {
    let mut left = vec![0.0; block];
    let mut right = vec![0.0; block];
    let mut done = 0;
    while done < frames {
        let n = block.min(frames - done);
        parts.engine.process(&mut left[..n], &mut right[..n]);
        done += n;
    }
}

/// A Constant at 0.5 feeding input `port` of a new `kind` with `param` set
/// to `value`, added after the engine has played for a while, as a paste
/// would add it. Returns what output `output` of the new module played
/// from its first frame, in host buffers of `block` frames.
fn added_while_playing(kind: &str, param: &str, value: f32, port: &str, output: &str, block: usize) -> Vec<f32> {
    let mut parts = engine();
    // Something plays first, so the new modules join a running engine.
    let running = parts.controller.add_module("loom.osc.sine").unwrap();
    let speaker = parts.controller.add_module("loom.io.audio-out").unwrap();
    let out = parts.controller.output_ref(running, "out").unwrap();
    let left = parts.controller.input_ref(speaker, "in").unwrap();
    parts.controller.connect(out, left).unwrap();
    parts.controller.commit().unwrap();
    run(&mut parts, 4_800, block);

    let c = &mut parts.controller;
    let constant = c.add_module("loom.math.constant").unwrap();
    c.set_param_named(constant, "value", 0.5).unwrap();
    let module = c.add_module(kind).unwrap();
    // `set_params`, as paste and undo set a module's values.
    // Ranges first, as paste sets them, so the range does not clamp the value.
    let descriptor = c.graph().node(module).unwrap().descriptor;
    let index = descriptor.param_index(param).unwrap();
    if let Some(&range) = descriptor.params[index]
        .options
        .ranges
        .iter()
        .find(|r| (r.min..=r.max).contains(&value))
    {
        c.set_range(module, index, range).unwrap();
    }
    c.set_params(module, &[(index, value)]).unwrap();
    let from = c.output_ref(constant, "out").unwrap();
    let to = c.input_ref(module, port).unwrap();
    c.connect(from, to).unwrap();
    let probe = c
        .add_probe(c.output_ref(module, output).unwrap(), ProbeKind::Scope)
        .unwrap();
    c.commit().unwrap();
    run(&mut parts, FRAMES, block);
    let mut samples = Vec::new();
    for chunk in parts.telemetry.scope() {
        if chunk.probe == probe {
            samples.extend_from_slice(chunk.samples());
        }
    }
    assert_eq!(samples.len(), FRAMES);
    samples
}

/// Multiply with gain 3 plays 3 times its input from its first frame, not a
/// glide up from its built-in gain of 1.
#[test]
fn a_new_multiply_starts_at_its_gain() {
    for block in BLOCKS {
        let out = added_while_playing("loom.math.mul", "gain", 3.0, "a", "out", block);
        assert!(out.iter().all(|&x| x == 1.5), "block {block}: {:?}", &out[..8]);
    }
}

/// An SVF at 200 Hz plays from its first frame what a new instance at
/// 200 Hz plays in the harness, which starts settled. Without the reset it
/// would sweep down from its built-in 1000 Hz for about 10 ms.
#[test]
fn a_new_svf_starts_at_its_cutoff() {
    let settled = Harness::new(&SVF)
        .frames(FRAMES)
        .param("cutoff", 200.0)
        .input("in", Signal::constant(0.5))
        .render();
    for block in BLOCKS {
        let out = added_while_playing("loom.filter.svf", "cutoff", 200.0, "in", "lowpass", block);
        assert_eq!(out, settled.output("lowpass"), "block {block}");
    }
}
