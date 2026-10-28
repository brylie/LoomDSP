//! The Transport module in the engine (`docs/design/clock.md`, Controls
//! live on the Transport module, Starting and stopping, rules 8, 10, 11).

mod common;

use common::*;
use loom_core::{EngineConfig, EngineParts, InputRef, LoomError, ModuleId, OutputSafety, TRANSPORT_GRID};

/// Frames in one bar at 120 BPM in 4/4 and 48 kHz.
const BAR: usize = 96_000;

/// An engine at 48 kHz with the safety stage off, `block`-frame blocks, and
/// a Transport. `build` adds the rest of the patch.
fn engine(block: usize, build: impl FnOnce(&mut loom_core::Controller, ModuleId)) -> (EngineParts, ModuleId) {
    let config = EngineConfig {
        max_block: block,
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = loom_core::create_engine(loom_modules::registry(), config);
    let transport = parts.controller.add_module("loom.clock.transport").unwrap();
    build(&mut parts.controller, transport);
    parts.controller.commit().unwrap();
    (parts, transport)
}

/// Sends `port` of `module` to the left output and `right` of `other` to
/// the right.
fn listen(c: &mut loom_core::Controller, left: (ModuleId, &str), right: (ModuleId, &str)) {
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(left.0, left.1, out, "left").unwrap();
    c.connect_named(right.0, right.1, out, "right").unwrap();
}

/// Frames where `signal` is high.
fn highs(signal: &[f32]) -> Vec<usize> {
    (0..signal.len()).filter(|&i| signal[i] > 0.5).collect()
}

/// A patch has one Transport: a second is refused, and the one it has
/// cannot be removed.
#[test]
fn a_patch_has_exactly_one_transport() {
    let (mut parts, transport) = engine(128, |_, _| {});
    let c = &mut parts.controller;
    assert_eq!(c.transport_module(), Some(transport));
    assert!(matches!(
        c.add_module("loom.clock.transport"),
        Err(LoomError::SecondTransport(id)) if id == transport
    ));
    assert!(matches!(
        c.remove_module(transport),
        Err(LoomError::TransportCannotBeRemoved(id)) if id == transport
    ));
    assert_eq!(c.transport_module(), Some(transport));
}

/// Run on the panel stops and starts the transport: stopping holds the
/// phases, running continues from there. Reset returns to bar 1 and fires
/// the jump trigger once.
#[test]
fn run_and_reset_on_the_panel() {
    let (mut parts, transport) = engine(256, |c, t| listen(c, (t, "bar-phase"), (t, "jump")));
    let (bar, jump) = render_with(&mut parts, 16 * 1024, 256, &[], |frame, parts| {
        let c = &mut parts.controller;
        match frame {
            2048 => drop(c.set_param_named(transport, "run", 0.0)),
            4096 => drop(c.set_param_named(transport, "run", 1.0)),
            8192 => drop(c.set_param_named(transport, "reset", 1.0)),
            9216 => drop(c.set_param_named(transport, "reset", 0.0)),
            _ => {}
        }
    });
    let held = bar[2048];
    assert!(held > bar[2047]);
    assert!(
        bar[2048..=4096].iter().all(|&x| x == held),
        "a stop holds the bar phase"
    );
    assert!(bar[4097] > held, "running continues");
    assert_eq!(bar[8192], 0.0, "reset returns to bar 1");
    assert_eq!(highs(&jump), [8192], "one jump, at the reset");
}

/// The bar and beat triggers fire on the same frames as a Phase Trigger on
/// the bar and beat phase outputs, in normal playback.
#[test]
fn bar_and_beat_triggers_agree_with_phase_trigger() {
    for (phase, trigger) in [("bar-phase", "bar"), ("beat-phase", "beat")] {
        let (mut parts, _) = engine(512, |c, t| {
            let watch = c.add_module("loom.phase.trigger").unwrap();
            c.connect_named(t, phase, watch, "phase").unwrap();
            listen(c, (t, trigger), (watch, "trigger"));
        });
        let (own, watched) = render(&mut parts, 2 * BAR + 1, 512, &[]);
        assert_eq!(highs(&own), highs(&watched), "{trigger}");
        // Bar 1 fires on frame 0 too, but the output's fade-in, which runs
        // even with the safety stage off, scales that frame to 0.
        let expected: Vec<usize> = if trigger == "bar" {
            vec![BAR, 2 * BAR]
        } else {
            (1..=8).map(|beat| beat * BAR / 4).collect()
        };
        assert_eq!(highs(&own), expected, "{trigger}");
    }
}

/// Rule 10: a new time signature while playing is a jump. The jump trigger
/// fires once and no skipped bar triggers burst out, at block sizes 1, 32,
/// and 1024.
#[test]
fn a_time_signature_change_is_one_jump() {
    let run = |block: usize| {
        let (mut parts, transport) = engine(block, |c, t| listen(c, (t, "jump"), (t, "bar")));
        render_with(&mut parts, 3 * BAR, block, &[], |frame, parts| {
            // Half way through bar 2, in buffers of every size.
            if frame == 147 * 1024 {
                parts.controller.set_param_named(transport, "numerator", 3.0).unwrap();
            }
        })
    };
    let reference = run(1024);
    for block in [1, 32] {
        assert!(run(block) == reference, "block size {block}");
    }
    let (jump, bar) = &reference;
    assert_eq!(highs(jump), [147 * 1024]);
    // Bar 2 at 96 000 (bar 1 at frame 0 is under the fade-in), then the
    // 3/4 grid from position 0: bar lines every 72 000 frames, the first
    // after the change at 216 000.
    assert_eq!(highs(bar), [BAR, 216_000]);
}

/// Rule 11: with cables on `run` and `reset`, the transport is the same at
/// block sizes 1, 32, and 1024, and an edge takes effect 1 to 32 frames
/// later, on the grid.
#[test]
fn run_and_reset_inputs_are_read_on_the_grid() {
    // Square LFOs make edges at frames that are not on the grid.
    let sources = |c: &mut loom_core::Controller| {
        let run = c.add_module("loom.mod.lfo").unwrap();
        c.set_param_named(run, "rate", 7.0).unwrap();
        c.set_param_named(run, "shape", 4.0).unwrap();
        let reset = c.add_module("loom.mod.lfo").unwrap();
        c.set_param_named(reset, "rate", 3.0).unwrap();
        c.set_param_named(reset, "shape", 4.0).unwrap();
        (run, reset)
    };
    let frames = 48_000;
    let run = |block: usize| {
        let (mut parts, _) = engine(block, |c, t| {
            let (run, reset) = sources(c);
            c.connect_named(run, "out", t, "run").unwrap();
            c.connect_named(reset, "out", t, "reset").unwrap();
            listen(c, (t, "playing"), (t, "bar-phase"));
        });
        render(&mut parts, frames, block, &[])
    };
    let reference = run(1024);
    for block in [1, 32] {
        assert!(run(block) == reference, "block size {block}");
    }

    // Where the run square changes, rendered alone.
    let (mut parts, _) = engine(1024, |c, _| {
        let (run, reset) = sources(c);
        listen(c, (run, "out"), (reset, "out"));
    });
    let (gate, _) = render(&mut parts, frames, 1024, &[]);
    // After the output's fade-in, so levels are exact.
    let start = 1024;
    let edges: Vec<usize> = (start..frames)
        .filter(|&i| (gate[i] > 0.5) != (gate[i - 1] > 0.5))
        .collect();
    let (playing, _) = &reference;
    let changes: Vec<usize> = (start..frames).filter(|&i| playing[i] != playing[i - 1]).collect();
    assert!(edges.len() >= 4, "{edges:?}");
    assert_eq!(changes.len(), edges.len());
    for (edge, change) in edges.iter().zip(&changes) {
        assert_eq!(change % TRANSPORT_GRID, 0, "on the grid");
        assert!(
            (1..=TRANSPORT_GRID).contains(&(change - edge)),
            "edge {edge}, change {change}"
        );
    }
}

/// A cable on `run` decides; removing it leaves the transport as it was,
/// and the Run toggle works again.
#[test]
fn removing_the_run_cable_leaves_the_transport_as_it_was() {
    let (mut parts, transport) = engine(256, |c, t| {
        let button = c.add_module("loom.ui.button").unwrap();
        c.connect_named(button, "gate", t, "run").unwrap();
        listen(c, (t, "playing"), (t, "bar-phase"));
    });
    // The button's gate is low, so the transport stops, though Run is on.
    let (playing, _) = render(&mut parts, 1024, 256, &[]);
    assert_eq!(*playing.last().unwrap(), 0.0);
    parts
        .controller
        .disconnect(InputRef {
            module: transport,
            port: 0,
        })
        .unwrap();
    parts.controller.commit().unwrap();
    let (playing, _) = render(&mut parts, 1024, 256, &[]);
    assert!(playing.iter().all(|&x| x == 0.0), "still stopped");
    // The toggle works again: off, as the app shows it after the cable
    // goes, then on.
    parts.controller.set_param_named(transport, "run", 0.0).unwrap();
    render(&mut parts, 256, 256, &[]);
    parts.controller.set_param_named(transport, "run", 1.0).unwrap();
    let (playing, _) = render(&mut parts, 1024, 256, &[]);
    assert!(playing.iter().all(|&x| x == 1.0), "running again");
}
