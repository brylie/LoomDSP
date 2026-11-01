//! NFR-RT-06: the audio path must never allocate or free.
//!
//! This test binary installs an allocator that aborts if any allocation or
//! deallocation happens inside `assert_no_alloc`. Every call to
//! `Engine::process` runs inside it, while the control side edits the graph
//! between buffers (where allocation is allowed). Every output is probed, so
//! telemetry runs too, both while the UI drains it and while its queues are
//! full (FR-VIS-04, NFR-RT-07).
//!
//! `every_module_never_allocates` runs `process` for every built-in DSP
//! module, so a new module is checked as soon as it is in `ALL`, whether or
//! not an example patch uses it.
//!
//! The check is active in debug builds, which is how `cargo test` runs.
//! `assert_no_alloc` has no `AllocDisabler` in release builds, so this file is
//! empty there: a release build could not abort on allocation anyway.

#![cfg(debug_assertions)]

mod common;

use assert_no_alloc::{AllocDisabler, assert_no_alloc};
use common::*;
use loom_core::{
    EngineConfig, InputRef, MidiControl, MidiEvent, ModuleRole, OutputRef, OutputSafety, ProbeKind, TimeSignature,
    create_engine,
};
use std::io::Write;

#[global_allocator]
static ALLOCATOR: AllocDisabler = AllocDisabler;

/// Unknown-module processing and schedule swaps never allocate on the audio thread.
#[test]
fn unknown_module_never_allocates() {
    let mut parts = load("subtractive.ron");
    let id = parts
        .controller
        .add_unknown(loom_core::UnknownModule {
            kind: "future.effect@1".into(),
            label: None,
            params: Default::default(),
            ranges: Default::default(),
            inputs: vec!["in".into()],
            outputs: vec!["out".into()],
            parameters: vec![],
        })
        .unwrap();
    parts
        .controller
        .connect_named(loom_core::ModuleId(8), "out", id, "in")
        .unwrap();
    parts
        .controller
        .connect_named(id, "out", loom_core::ModuleId(9), "in")
        .unwrap();
    parts.controller.commit().unwrap();
    let mut left = [0.0; 512];
    let mut right = [0.0; 512];
    for _ in 0..4 {
        assert_no_alloc(|| parts.engine.process(&mut left, &mut right));
    }
}

/// `copy_state_from` runs on the audio thread when a module turns stereo,
/// so it must not allocate or free (stereo spec, rule 26). Checked for every
/// built-in DSP module, between two instances that have both run.
#[test]
fn copying_state_never_allocates() {
    let config = loom_core::ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 512,
    };
    for descriptor in loom_modules::ALL {
        let loom_core::ModuleRole::Dsp(factory) = descriptor.role else {
            continue;
        };
        let running = factory(&config);
        let mut copy = factory(&config);
        assert_no_alloc(|| copy.copy_state_from(&*running));
    }
}

/// `process` never allocates for any built-in DSP module, including modules
/// that no example patch uses (CONTRIBUTING.md, Rules for audio code).
///
/// Each module runs alone in its own engine, so a failure points at one
/// module. A fast LFO feeds every input, so gates cross their threshold and
/// audio inputs carry a tone. Every output has a probe, so the module is
/// scheduled (FR-GRAPH-09). Between buffers every parameter moves across its
/// range, so smoothing and the paths that react to a new value run too.
///
/// An allocation aborts the whole test binary, so no test result names the
/// module. Run with `LOOM_TRACE_NO_ALLOC=1` to print each module's ID before
/// it runs: the last ID printed is the one that allocated.
#[test]
fn every_module_never_allocates() {
    let trace = std::env::var_os("LOOM_TRACE_NO_ALLOC").is_some();
    let modules = loom_modules::ALL
        .iter()
        .filter(|descriptor| matches!(descriptor.role, ModuleRole::Dsp(_)));
    for descriptor in modules {
        // Not `eprintln!`: the test harness captures that, and an abort
        // throws captured output away.
        if trace {
            let _ = writeln!(std::io::stderr(), "every_module_never_allocates: {}", descriptor.id);
        }

        let mut parts = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(SAMPLE_RATE));
        let controller = &mut parts.controller;
        // A patch has exactly one Transport, and the engine starts with it.
        let module = match controller.transport_module() {
            Some(transport) if descriptor.id == "loom.clock.transport" => transport,
            _ => controller.add_module(descriptor.id).unwrap(),
        };
        let lfo = controller.add_module("loom.mod.lfo").unwrap();
        controller.set_param_named(lfo, "rate", 30.0).unwrap();
        let wave = controller.output_ref(lfo, "out").unwrap();
        for port in 0..descriptor.inputs.len() {
            controller.connect(wave, InputRef { module, port }).unwrap();
        }
        for port in 0..descriptor.outputs.len() {
            controller
                .add_probe(OutputRef { module, port }, ProbeKind::MeterAndScope)
                .unwrap();
        }
        controller.commit().unwrap();

        let mut left = vec![0.0; 1024];
        let mut right = vec![0.0; 1024];
        for i in 0..60usize {
            // Control thread: allocation allowed here.
            for (index, param) in descriptor.params.iter().enumerate() {
                let step = ((i + 3 * index) % 10) as f32 / 9.0;
                parts
                    .controller
                    .set_param(module, index, param.min + (param.max - param.min) * step)
                    .unwrap();
            }
            parts.controller.commit().unwrap();
            parts.telemetry.meters().for_each(drop);
            parts.telemetry.scope().for_each(drop);
            let frames = [1, 32, 128, 129, 1024][i % 5];

            // Audio thread: no allocation allowed.
            assert_no_alloc(|| {
                parts.engine.process(&mut left[..frames], &mut right[..frames]);
            });
        }
    }
}

#[test]
fn audio_thread_never_allocates() {
    for patch in [
        "sine.ron",
        "subtractive.ron",
        "fm-feedback.ron",
        "fm-feedback-delay1.ron",
        "plucked-string.ron",
    ] {
        let mut parts = load(patch);
        let outputs: Vec<OutputRef> = parts
            .controller
            .graph()
            .nodes()
            .flat_map(|(module, node)| (0..node.descriptor.outputs.len()).map(move |port| OutputRef { module, port }))
            .collect();
        for output in outputs {
            parts.controller.add_probe(output, ProbeKind::MeterAndScope).unwrap();
        }
        let mut left = vec![0.0; 1024];
        let mut right = vec![0.0; 1024];
        let mut extra = Vec::new();
        for i in 0..400usize {
            // Control thread: allocation allowed here.
            if i % 7 == 0 {
                let id = parts.controller.add_module("loom.filter.svf").unwrap();
                let lowpass = parts.controller.output_ref(id, "lowpass").unwrap();
                parts.controller.add_probe(lowpass, ProbeKind::Scope).unwrap();
                extra.push(id);
            }
            // Close the loop a buffer later, once the filter has run. The
            // inserted delay is new, so the swap starts it from the old
            // schedule's last lowpass value (live patching spec, rule 5).
            if i % 7 == 1
                && let Some(&id) = extra.last()
            {
                parts.controller.connect_named(id, "lowpass", id, "in").unwrap();
            }
            if i % 11 == 0
                && let Some(id) = extra.pop()
            {
                parts.controller.remove_module(id).unwrap();
            }
            // Assignments compile into the schedule and are filled on the
            // audio thread, so adding and removing them here must not make
            // `process` allocate.
            if i % 9 == 0
                && let Some(&id) = extra.last()
            {
                parts
                    .controller
                    .add_assignment_named(id, "lowpass", id, "cutoff", 0.2)
                    .unwrap();
            }
            // MIDI mappings compile into the schedule too, and control
            // changes set parameters on the audio thread.
            if i % 9 == 4
                && let Some(&id) = extra.last()
            {
                let control = MidiControl {
                    channel: 0,
                    controller: (i / 9 % 128) as u8,
                };
                parts.controller.map_midi_named(control, id, "cutoff").unwrap();
            }
            if i % 3 == 0 {
                parts
                    .midi
                    .send(MidiEvent::control_change(0, 0, (i / 9 % 128) as u8, (i % 128) as u8));
            }
            if i % 17 == 0 {
                let id = parts.controller.graph().assignments().next().map(|(id, _)| id);
                if let Some(id) = id {
                    parts.controller.set_assignment_depth(id, -0.15).unwrap();
                }
            }
            parts.controller.commit().unwrap();
            if i % 50 == 0 {
                parts.midi.send(MidiEvent::note_on(3, 0, 60 + (i % 12) as u8, 90));
            }
            if i % 50 == 25 {
                parts.midi.send(MidiEvent::note_off(0, 0, 60 + ((i - 25) % 12) as u8));
            }
            // Transport changes are applied on the audio thread too.
            match i % 13 {
                3 => {
                    parts.controller.set_tempo(60.0 + i as f64).unwrap();
                }
                5 => parts.controller.set_running(i % 2 == 0).unwrap(),
                8 => parts.controller.reset_transport().unwrap(),
                11 => parts.controller.set_time_signature(TimeSignature::new(7, 8)).unwrap(),
                _ => {}
            }
            // The Transport module's parameters and inputs ask the engine
            // for changes on the audio thread, and a cable on `run` splits
            // blocks on the transport grid (clock.md, rule 11).
            if let Some(transport) = parts.controller.transport_module() {
                match i % 29 {
                    2 => drop(
                        parts
                            .controller
                            .set_param_named(transport, "tempo", 90.0 + i as f32 % 50.0),
                    ),
                    6 => drop(
                        parts
                            .controller
                            .set_param_named(transport, "numerator", (1 + i % 7) as f32),
                    ),
                    9 => drop(parts.controller.set_param_named(transport, "reset", (i % 2) as f32)),
                    12 => {
                        let lfo = parts.controller.add_module("loom.mod.lfo").unwrap();
                        parts.controller.set_param_named(lfo, "shape", 4.0).unwrap();
                        parts.controller.set_param_named(lfo, "rate", 40.0).unwrap();
                        parts.controller.connect_named(lfo, "out", transport, "run").unwrap();
                        parts.controller.connect_named(lfo, "out", transport, "reset").unwrap();
                    }
                    20 => {
                        // The run cable goes, so blocks stop splitting.
                        let lfo = parts.controller.graph().cables().find(|(_, to)| to.module == transport);
                        if let Some((from, _)) = lfo {
                            parts.controller.remove_module(from.module).unwrap();
                        }
                    }
                    _ => {}
                }
            }
            // So are safety mode changes (ADR-0019), whose fade spans buffers.
            if i % 19 == 0 {
                let mode = [OutputSafety::Clean, OutputSafety::Off, OutputSafety::LowLatency][i / 19 % 3];
                parts.controller.set_output_safety(mode).unwrap();
            }
            // Panic (panic.md) runs on the audio thread, and its reset can
            // fall inside a buffer. It also runs from its MIDI trigger.
            if i % 23 == 0 {
                parts.controller.panic().unwrap();
            }
            // MIDI inputs come and go (device-changes.md): the engine
            // tracks their notes and releases them without allocating.
            let input = loom_core::MidiInputId(1 + (i / 10 % 3) as u16);
            if i % 10 == 0 {
                parts.controller.open_midi_input(input).unwrap();
            }
            if i % 10 == 3 {
                parts.midi.send(MidiEvent::note_on(0, 0, 64, 90).from_input(input));
            }
            if i % 10 == 7 {
                parts.controller.release_midi_input(input).unwrap();
            }
            if i == 0 {
                let trigger = loom_core::PanicTrigger::Note { channel: 0, note: 62 };
                parts.controller.set_panic_trigger(Some(trigger)).unwrap();
            }
            // Drain telemetry for a while, then let the queues fill up.
            if i < 200 {
                parts.telemetry.meters().for_each(drop);
                parts.telemetry.scope().for_each(drop);
            }
            let frames = [1, 32, 128, 129, 1024][i % 5];

            // Audio thread: no allocation allowed.
            assert_no_alloc(|| {
                parts.engine.process(&mut left[..frames], &mut right[..frames]);
            });
        }
        // The fade-out before a stop (rule 14) is applied on the audio
        // thread too, and runs until the engine is silent.
        parts.controller.fade_out().unwrap();
        for frames in [1, 128, 1024] {
            assert_no_alloc(|| {
                parts.engine.process(&mut left[..frames], &mut right[..frames]);
            });
        }
        assert!(parts.engine.is_silent());
        let stats = parts.controller.stats();
        assert!(stats.dropped_scope.load(std::sync::atomic::Ordering::Relaxed) > 0);
    }
}

/// Checks that `loom.time.delay` and `loom.time.allpass` never allocate on the
/// audio thread while their `time` and `gain` parameters and `time` input
/// cables change during playback.
#[test]
fn delay_and_allpass_never_allocate_while_sweeping_parameters_and_inputs() {
    let mut parts = load("sine.ron");
    let c = &mut parts.controller;
    let voice = c
        .graph()
        .nodes()
        .find(|(_, node)| node.descriptor.id == "loom.math.mul")
        .map(|(id, _)| id)
        .expect("sine.ron has a VCA");
    let out = c
        .graph()
        .nodes()
        .find(|(_, node)| node.descriptor.id == "loom.io.audio-out")
        .map(|(id, _)| id)
        .expect("sine.ron has an audio-out");
    let lfo_phase = c.add_module("loom.phase.generator").unwrap();
    let lfo = c.add_module("loom.osc.sine").unwrap();
    let lfo_scale = c.add_module("loom.math.mul").unwrap();
    let delay = c.add_module("loom.time.delay").unwrap();
    let allpass = c.add_module("loom.time.allpass").unwrap();
    c.set_param_named(lfo_scale, "gain", 0.005).unwrap();
    c.connect_named(lfo_phase, "phase", lfo, "phase").unwrap();
    c.connect_named(lfo, "out", lfo_scale, "a").unwrap();
    // Feed the sine voice through the delay line and all-pass in series, with
    // audio-rate time modulation on both.
    c.connect_named(voice, "out", delay, "in").unwrap();
    c.connect_named(lfo_scale, "out", delay, "time").unwrap();
    c.connect_named(delay, "out", allpass, "in").unwrap();
    c.connect_named(lfo_scale, "out", allpass, "time").unwrap();
    c.connect_named(allpass, "out", out, "in").unwrap();
    c.commit().unwrap();

    let mut left = vec![0.0; 1024];
    let mut right = vec![0.0; 1024];
    parts.midi.send(MidiEvent::note_on(0, 0, 60, 100));
    for i in 0..200usize {
        // Control thread: sweep delay time across the full 0.1 ms .. 2.0 s range
        // and all-pass gain across -0.95 .. 0.95.
        let delay_time = 0.0001 + (i as f32 / 199.0) * 1.9999;
        let ap_time = 0.0001 + ((199 - i) as f32 / 199.0) * 0.1999;
        let ap_gain = -0.95 + (i as f32 / 199.0) * 1.9;
        parts.controller.set_param_named(delay, "time", delay_time).unwrap();
        parts.controller.set_param_named(allpass, "time", ap_time).unwrap();
        parts.controller.set_param_named(allpass, "gain", ap_gain).unwrap();
        parts.controller.commit().unwrap();

        let frames = [1, 32, 77, 128, 1024][i % 5];
        assert_no_alloc(|| {
            parts.engine.process(&mut left[..frames], &mut right[..frames]);
        });
    }
}

/// Installing, reading, and removing an input ring never allocates or frees
/// on the audio thread, including an underrun and an overrun correction.
#[test]
fn audio_input_never_allocates_on_the_audio_thread() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let (mut writer, reader) = loom_core::audio_input::input_ring(128, 2);
    let c = &mut parts.controller;
    let input = c.add_module("loom.io.audio-in").unwrap();
    let output = c.output_ref(input, "out").unwrap();
    c.add_probe(output, ProbeKind::Meter).unwrap();
    c.set_audio_input(Some(reader)).unwrap();
    c.commit().unwrap();
    for _ in 0..300 {
        assert!(writer.push([0.25; 8]));
    }
    let mut left = [0.0; 128];
    let mut right = [0.0; 128];
    assert_no_alloc(|| parts.engine.process(&mut left, &mut right));
    assert_no_alloc(|| parts.engine.process(&mut left, &mut right));
    parts.controller.set_audio_input(None).unwrap();
    assert_no_alloc(|| parts.engine.process(&mut left, &mut right));
    parts.controller.collect_garbage();
}

/// Receiving a stream restart command never allocates on the audio thread.
#[test]
fn restarting_the_output_fade_never_allocates() {
    let mut parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
    let mut restart = parts.engine.restart_fade_sender();
    let mut left = [0.0; 128];
    let mut right = [0.0; 128];
    restart.request();
    assert_no_alloc(|| parts.engine.process(&mut left, &mut right));
}
