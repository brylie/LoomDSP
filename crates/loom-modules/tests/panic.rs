//! Panic in the engine. Each test is an acceptance criterion of
//! `docs/design/panic.md`.

mod common;

use common::*;
use loom_core::safety::{FADE_SECONDS, samples};
use loom_core::{EngineConfig, EngineParts, MidiEvent, ModuleId, OutputSafety, PanicTrigger, ProbeKind, create_engine};
use loom_test::analysis::{max_step, peak};

const BLOCKS: [usize; 3] = [1, 77, 512];

/// Frames in one 10 ms fade.
fn fade() -> usize {
    samples(FADE_SECONDS, SAMPLE_RATE)
}

/// An empty engine with the safety stage in `mode`.
fn engine(mode: OutputSafety) -> EngineParts {
    let config = EngineConfig {
        output_safety: mode,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    create_engine(loom_modules::registry(), config)
}

/// Patches `source`'s `out` into both sides of an Audio Out, and commits.
fn play(parts: &mut EngineParts, source: ModuleId, port: &str) {
    let c = &mut parts.controller;
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(source, port, out, "in").unwrap();
    c.commit().unwrap();
}

/// A held note: MIDI Note's pitch plays a sine, its gate opens an ADSR,
/// and the ADSR sets the sine's level into Audio Out. Returns the ADSR.
fn held_note(mode: OutputSafety) -> (EngineParts, ModuleId) {
    let mut parts = engine(mode);
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    let adsr = c.add_module("loom.env.envelope").unwrap();
    let phase = c.add_module("loom.phase.generator").unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    let vca = c.add_module("loom.math.mul").unwrap();
    c.connect_named(note, "gate", adsr, "gate").unwrap();
    c.connect_named(note, "pitch", phase, "pitch").unwrap();
    c.connect_named(phase, "phase", sine, "phase").unwrap();
    c.connect_named(sine, "out", vca, "a").unwrap();
    c.connect_named(adsr, "out", vca, "b").unwrap();
    play(&mut parts, vca, "out");
    assert!(parts.midi.send(MidiEvent::note_on(0, 0, 60, 100)));
    (parts, adsr)
}

/// A fixed tone: Phase Generator into Sine, into Audio Out.
fn tone(mode: OutputSafety) -> EngineParts {
    let mut parts = engine(mode);
    let c = &mut parts.controller;
    let phase = c.add_module("loom.phase.generator").unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    c.connect_named(phase, "phase", sine, "phase").unwrap();
    play(&mut parts, sine, "out");
    parts
}

/// Renders `frames` frames in host buffers of `block` frames. A buffer
/// also ends at `at`, so `act` runs before the buffer that starts there.
fn render_around(
    parts: &mut EngineParts,
    frames: usize,
    block: usize,
    at: usize,
    mut act: impl FnMut(&mut EngineParts),
) -> Vec<f32> {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut position = 0;
    while position < frames {
        let mut n = block.min(frames - position);
        if position < at {
            n = n.min(at - position);
        }
        if position == at {
            act(parts);
        }
        parts
            .engine
            .process(&mut left[position..position + n], &mut right[position..position + n]);
        position += n;
    }
    assert_eq!(left, right, "both sides play the same");
    left
}

/// Renders with a panic at frame `at`.
fn render_panic(parts: &mut EngineParts, frames: usize, block: usize, at: usize) -> Vec<f32> {
    render_around(parts, frames, block, at, |parts| parts.controller.panic().unwrap())
}

/// The largest step a fade over `steady` audio may have: its own largest
/// step, plus one fade step times its peak (audio QA, as for the safety
/// stage's fades).
fn step_limit(steady: &[f32]) -> f32 {
    max_step(steady) + peak(steady) / fade() as f32 + 1e-6
}

/// A held note stops: the output reaches silence within 10 ms with no
/// click, the gate is low and the envelope at 0, so it stays silent after
/// the fade-in too.
#[test]
fn a_held_note_stops_without_a_click() {
    let at = 24_000;
    let frames = at + 6 * fade();
    for mode in [OutputSafety::LowLatency, OutputSafety::Off] {
        let (mut parts, _) = held_note(mode);
        let out = render_panic(&mut parts, frames, 128, at);
        assert!(
            peak(&out[at - fade()..at]) > 0.1,
            "{mode}: the note plays before the panic"
        );
        assert_eq!(out[at + fade()], 0.0, "{mode}: silent within 10 ms");
        let limit = step_limit(&out[at - fade()..at]);
        let step = max_step(&out[at - 1..=at + fade()]);
        assert!(step <= limit, "{mode}: fade-out step {step} > {limit}");
        assert!(
            out[at + fade()..].iter().all(|&x| x == 0.0),
            "{mode}: the gate is low and the envelope is at 0"
        );
    }
}

/// A delay with feedback that rings is cleared: after the fade its output
/// is 0 and stays 0.
#[test]
fn a_ringing_delay_is_cleared() {
    let mut parts = engine(OutputSafety::Off);
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    let adsr = c.add_module("loom.env.envelope").unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let delay = c.add_module("loom.time.delay").unwrap();
    let feedback = c.add_module("loom.math.mul").unwrap();
    c.connect_named(note, "gate", adsr, "gate").unwrap();
    c.connect_named(adsr, "out", add, "a").unwrap();
    c.connect_named(delay, "out", feedback, "a").unwrap();
    c.set_param_named(feedback, "gain", 0.9).unwrap();
    c.connect_named(feedback, "out", add, "b").unwrap();
    c.connect_named(add, "out", delay, "in").unwrap();
    c.set_param_named(delay, "time", 0.05).unwrap();
    play(&mut parts, delay, "out");
    assert!(parts.midi.send(MidiEvent::note_on(0, 0, 60, 100)));
    let at = 48_000;
    let out = render_around(&mut parts, at + 48_000, 128, 4_800, |parts| {
        assert!(parts.midi.send(MidiEvent::note_off(0, 0, 60)));
    });
    // Ringing still, well after the note ended: then panic.
    assert!(peak(&out[at - 4_800..at]) > 0.05, "the delay rings");
    let mut parts_after = parts;
    let after = render_panic(&mut parts_after, 48_000, 128, 0);
    assert!(after[fade()..].iter().all(|&x| x == 0.0), "the delay is clear");
}

/// The transport keeps running at a continuous position: a Phase Scale
/// that follows the bar reads the same with and without a panic.
#[test]
fn the_transport_keeps_running() {
    let render = |panic: bool| {
        let mut parts = engine(OutputSafety::Off);
        let scale = parts.controller.add_module("loom.phase.scale").unwrap();
        let bar = parts.controller.output_ref(scale, "phase").unwrap();
        parts.controller.add_probe(bar, ProbeKind::Scope).unwrap();
        play(&mut parts, scale, "phase");
        let mut bars = Vec::new();
        let at = 10_000;
        render_around(&mut parts, 30_000, 256, at, |parts| {
            if panic {
                parts.controller.panic().unwrap();
            }
        });
        for chunk in parts.telemetry.scope() {
            bars.extend_from_slice(&chunk.samples[..chunk.len as usize]);
        }
        bars
    };
    let still = render(false);
    assert_eq!(still.len(), 30_000);
    assert!(still[29_999] > still[0], "the bar moves");
    assert_eq!(render(true), still, "panic does not touch the transport");
}

/// The sound returns over 10 ms, and a tone at a fixed pitch restarts from
/// phase 0: after the reset frame the output is what a new engine plays.
#[test]
fn a_tone_restarts_from_phase_zero_and_fades_in() {
    for mode in [OutputSafety::LowLatency, OutputSafety::Clean, OutputSafety::Off] {
        let fresh = render_panic(&mut tone(mode), 4 * fade(), 128, usize::MAX);
        let at = 10_000;
        let out = render_panic(&mut tone(mode), at + 4 * fade(), 128, at);
        // The fade-in of the new engine is complete by now, so the fade
        // out takes exactly one fade, and the reset frame is `at + fade`.
        let reset = at + fade();
        assert_eq!(&out[reset..], &fresh[..out.len() - reset], "{mode}");
        assert_eq!(out[reset], 0.0, "{mode}: the fade-in starts at 0");
    }
}

/// Output with a panic at frame N is the same at block sizes 1, 77, and
/// 512, including when N falls inside the engine's internal blocks.
#[test]
fn panic_is_the_same_at_every_block_size() {
    for at in [5_000, 5_077] {
        let renders: Vec<Vec<f32>> = BLOCKS
            .iter()
            .map(|&block| {
                let (mut parts, _) = held_note(OutputSafety::LowLatency);
                let phase = parts.controller.add_module("loom.phase.generator").unwrap();
                let sine = parts.controller.add_module("loom.osc.sine").unwrap();
                parts.controller.connect_named(phase, "phase", sine, "phase").unwrap();
                parts.controller.commit().unwrap();
                render_panic(&mut parts, at + 4 * fade(), block, at)
            })
            .collect();
        for (render, block) in renders.iter().zip(BLOCKS) {
            assert_eq!(
                render, &renders[0],
                "block size {block} matches block size 1, panic at {at}"
            );
        }
    }
}

/// A panic while the output fades out before a stop does not bring the
/// sound back: the engine stays silent.
#[test]
fn panic_after_a_fade_out_stays_silent() {
    let mut parts = tone(OutputSafety::Off);
    let at = 10_000;
    let out = render_around(&mut parts, at + 4 * fade(), 128, at, |parts| {
        parts.controller.fade_out().unwrap();
        parts.controller.panic().unwrap();
    });
    assert!(out[at + fade()..].iter().all(|&x| x == 0.0));
    assert!(parts.engine.is_silent());
}

/// A second panic during the fade is ignored: the silence is not longer.
#[test]
fn a_second_panic_during_the_fade_is_ignored() {
    let at = 10_000;
    let once = render_panic(&mut tone(OutputSafety::Off), at + 4 * fade(), 64, at);
    let mut parts = tone(OutputSafety::Off);
    let twice = render_around(&mut parts, at + 4 * fade(), 64, at, |parts| {
        parts.controller.panic().unwrap();
        parts.controller.panic().unwrap();
    });
    assert_eq!(once, twice);
    // And mashing it during the fade, one buffer later, changes nothing.
    let mut parts = tone(OutputSafety::Off);
    let mut mashed = render_panic(&mut parts, at + 64, 64, at);
    mashed.extend(render_panic(&mut parts, 4 * fade() - 64, 64, 0));
    assert_eq!(once, mashed);
}

/// The MIDI trigger: a control change of 127 on CC 123 runs panic, and a
/// value of 0 does nothing.
#[test]
fn a_midi_trigger_runs_panic() {
    let at = 10_000;
    let trigger = PanicTrigger::Cc {
        channel: 0,
        controller: 123,
    };
    let run = |value: u8| {
        let mut parts = tone(OutputSafety::Off);
        parts.controller.set_panic_trigger(Some(trigger)).unwrap();
        render_around(&mut parts, at + 4 * fade(), 128, at, |parts| {
            assert!(parts.midi.send(MidiEvent::control_change(0, 0, 123, value)));
        })
    };
    let reset = at + fade();
    let fired = run(127);
    assert_eq!(fired[reset], 0.0, "panic ran");
    let ignored = run(0);
    assert!(
        ignored[reset - 10..reset + 10].iter().any(|&x| x.abs() > 0.1),
        "no panic"
    );
}
