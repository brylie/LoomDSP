//! Bundle compilation and structural edits before the MIDI source exists.

use loom_core::{EngineConfig, EngineParts, MidiControl, OutputSafety, Registry, Width, create_engine};
use loom_modules::{register_all, test_modules};
use std::sync::Arc;

#[cfg(debug_assertions)]
#[global_allocator]
static ALLOCATOR: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

/// All built-ins and the bundle test source, with exact output values.
fn engine_with(config: EngineConfig) -> EngineParts {
    let mut registry = Registry::new();
    register_all(&mut registry).unwrap();
    test_modules::register(&mut registry).unwrap();
    create_engine(Arc::new(registry), config)
}

/// An ordinary test engine with output safety disabled.
fn engine() -> EngineParts {
    engine_with(EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::default()
    })
}

/// Runs past the fade-in and returns one steady buffer, checking audio allocation.
fn render(parts: &mut EngineParts) -> [f32; 512] {
    let mut left = [0.0; 512];
    let mut right = [0.0; 512];
    for _ in 0..3 {
        #[cfg(debug_assertions)]
        assert_no_alloc::assert_no_alloc(|| parts.engine.process(&mut left, &mut right));
        #[cfg(not(debug_assertions))]
        parts.engine.process(&mut left, &mut right);
    }
    assert_eq!(left, right);
    left
}

/// A bundle source's structural count directly changes output width and Audio Out sums.
#[test]
fn a_structural_edit_recompiles_and_audio_out_sums_voices() {
    let mut parts = engine();
    let source = parts.controller.add_module("test.voices").unwrap();
    let out = parts.controller.add_module("loom.io.audio-out").unwrap();
    parts.controller.connect_named(source, "out", out, "in").unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(render(&mut parts), [0.01; 512]);
    let revision = parts.controller.revision();
    parts.controller.set_param_named(source, "voices", 8.0).unwrap();
    assert_eq!(parts.controller.revision(), revision.wrapping_add(1));
    let port = parts.controller.output_ref(source, "out").unwrap();
    assert_eq!(
        parts.controller.last_report().width(port),
        Width { voices: 8, sides: 1 }
    );
    let sum = (0..8).map(|voice| 0.01 + voice as f32 * 0.01).sum::<f32>();
    assert_eq!(render(&mut parts), [sum; 512]);
    parts.controller.set_param_named(source, "voices", 1.0).unwrap();
    assert_eq!(render(&mut parts), [0.01; 512]);
}

/// Downmix control inputs copy per voice; a narrower bundle reads defaults beyond its end.
#[test]
fn ordinary_modules_copy_and_pad_narrower_bundles() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let four = c.add_module("test.voices").unwrap();
    let eight = c.add_module("test.voices").unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(four, "voices", 4.0).unwrap();
    c.set_param_named(eight, "voices", 8.0).unwrap();
    c.connect_named(four, "out", add, "a").unwrap();
    c.connect_named(eight, "out", add, "b").unwrap();
    c.connect_named(add, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(c.last_report().width(c.output_ref(add, "out").unwrap()).voices, 8);
    let sum = (0..8)
        .map(|v| (0.01 + v as f32 * 0.01) + if v < 4 { 0.01 + v as f32 * 0.01 } else { 0.0 })
        .sum::<f32>();
    assert_eq!(render(&mut parts), [sum; 512]);
}

/// A mono source fans out to every copy, without making the source polyphonic.
#[test]
fn mono_fans_out_to_every_voice() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let bundle = c.add_module("test.voices").unwrap();
    let mono = c.add_module("test.linear").unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(bundle, "voices", 8.0).unwrap();
    c.set_param_named(mono, "level", 0.02).unwrap();
    c.connect_named(bundle, "out", add, "a").unwrap();
    c.connect_named(mono, "out", add, "b").unwrap();
    c.connect_named(add, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(c.last_report().width(c.output_ref(mono, "out").unwrap()), Width::MONO);
    let sum = (0..8).map(|v| (0.01 + v as f32 * 0.01) + 0.02).sum::<f32>();
    assert_eq!(render(&mut parts), [sum; 512]);
}

/// The controller refuses audio-thread modulation and MIDI mappings on structure.
#[test]
fn structural_parameters_refuse_live_targets() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let error = c
        .add_assignment_named(source, "out", source, "voices", 0.5)
        .unwrap_err();
    assert!(error.to_string().contains("voices"));
    let error = c
        .map_midi_named(
            MidiControl {
                channel: 0,
                controller: 12,
            },
            source,
            "voices",
        )
        .unwrap_err();
    assert!(error.to_string().contains("changes the graph"));
}

/// A shortage of copy slots leaves the old values, report, and sound in place.
#[test]
fn a_failed_structural_edit_preserves_the_running_graph() {
    let mut parts = engine_with(EngineConfig {
        max_modules: 4,
        output_safety: OutputSafety::Off,
        ..EngineConfig::default()
    });
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(source, "out", add, "a").unwrap();
    c.connect_named(add, "out", out, "in").unwrap();
    c.commit().unwrap();
    let before = render(&mut parts);
    assert!(parts.controller.set_param_named(source, "voices", 8.0).is_err());
    let node = parts.controller.graph().node(source).unwrap();
    assert_eq!(node.params[node.descriptor.param_index("voices").unwrap()], 1.0);
    assert_eq!(
        parts
            .controller
            .last_report()
            .width(parts.controller.output_ref(source, "out").unwrap()),
        Width::MONO
    );
    assert_eq!(render(&mut parts), before);
}

/// A parameter assignment from a bundle does not widen its otherwise mono target.
#[test]
fn bundle_modulation_does_not_widen_its_target() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let target = c.add_module("test.linear").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(source, "voices", 8.0).unwrap();
    c.add_assignment_named(source, "out", target, "level", 0.5).unwrap();
    c.connect_named(target, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(c.last_report().width(c.output_ref(target, "out").unwrap()), Width::MONO);
    assert!(render(&mut parts).iter().all(|value| value.is_finite()));
}

/// The harness feeds every voice and reads every output at all required block sizes.
#[test]
fn bundle_harness_checks_each_voice_and_live_capacity() {
    let harness = loom_test::Harness::new(&test_modules::VOICES)
        .frames(1024)
        .param("voices", 8.0)
        .bundle_input(
            "in",
            (0..8)
                .map(|voice| loom_test::Signal::constant(voice as f32 * 0.02))
                .collect(),
        );
    let rendered = harness.check_block_sizes();
    for voice in 0..8 {
        let expected = 0.01 + voice as f32 * 0.01 + voice as f32 * 0.02;
        assert_eq!(rendered.voice_output("out", voice, 0), &[expected; 1024]);
    }
    harness.check_reset();
    assert!(harness.check_copy_state(400));
    let resized = loom_test::Harness::new(&test_modules::VOICES)
        .frames(1024)
        .param("voices", 8.0)
        .param_at(512.0 / 48_000.0, "voices", 4.0)
        .check_block_sizes();
    assert!(
        resized.voice_output("out", 7, 0)[..512]
            .iter()
            .all(|&value| value > 0.0)
    );
    assert_eq!(&resized.voice_output("out", 7, 0)[512..], &[0.0; 512]);
}

/// Feedback delays have separate state per voice and survive a width change.
#[test]
fn a_feedback_loop_carries_every_voice() {
    let mut parts = engine();
    // Finish the output fade before connecting the exact-value test graph.
    render(&mut parts);
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let delay = c.add_module("loom.time.delay1").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(source, "voices", 4.0).unwrap();
    c.connect_named(source, "out", add, "a").unwrap();
    c.connect_named(delay, "out", add, "b").unwrap();
    c.connect_named(add, "out", delay, "in").unwrap();
    c.connect_named(delay, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(c.last_report().width(c.output_ref(delay, "out").unwrap()).voices, 4);
    assert_eq!(c.last_report().loops.len(), 1);
    let mut state = [0.0; 8];
    let mut left = [0.0; 4];
    let mut right = [0.0; 4];
    #[cfg(debug_assertions)]
    assert_no_alloc::assert_no_alloc(|| parts.engine.process(&mut left, &mut right));
    #[cfg(not(debug_assertions))]
    parts.engine.process(&mut left, &mut right);
    for actual in left {
        assert_eq!(actual, state[..4].iter().sum::<f32>());
        for (voice, value) in state[..4].iter_mut().enumerate() {
            *value += 0.01 + voice as f32 * 0.01;
        }
    }
    assert_eq!(left, right);
    parts.controller.set_param_named(source, "voices", 8.0).unwrap();
    for _ in 0..2 {
        let mut left = [0.0];
        let mut right = [0.0];
        #[cfg(debug_assertions)]
        assert_no_alloc::assert_no_alloc(|| parts.engine.process(&mut left, &mut right));
        #[cfg(not(debug_assertions))]
        parts.engine.process(&mut left, &mut right);
        assert_eq!(left[0], state.iter().sum::<f32>());
        assert_eq!(left, right);
        for (voice, value) in state.iter_mut().enumerate() {
            *value += 0.01 + voice as f32 * 0.01;
        }
    }
}

/// Registry validation requires structural counts and still rejects Downmix outputs.
#[test]
fn voice_descriptors_require_structural_counts() {
    static MISSING: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
        params: &[],
        ..test_modules::VOICES
    };
    static DOWNMIX: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
        outputs: &[loom_core::PortSpec {
            channels: loom_core::Channels::Downmix,
            ..test_modules::VOICES.outputs[0]
        }],
        ..test_modules::VOICES
    };
    assert!(
        Registry::new()
            .register(&MISSING)
            .unwrap_err()
            .to_string()
            .contains("voices")
    );
    assert!(
        Registry::new()
            .register(&DOWNMIX)
            .unwrap_err()
            .to_string()
            .contains("Downmix")
    );
    Registry::new().register(&test_modules::VOICES).unwrap();
}

/// Stereo-aware modules run once per voice and preserve both sides of each voice.
#[test]
fn stereo_sides_multiply_with_voices_without_downmixing_them() {
    let mut parts = engine();
    let c = &mut parts.controller;
    let left_source = c.add_module("test.voices").unwrap();
    let right_source = c.add_module("test.voices").unwrap();
    let join = c.add_module("loom.mix.join").unwrap();
    let split = c.add_module("loom.mix.split").unwrap();
    let rejoin = c.add_module("loom.mix.join").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(left_source, "voices", 8.0).unwrap();
    c.set_param_named(right_source, "voices", 4.0).unwrap();
    c.connect_named(left_source, "out", join, "left").unwrap();
    c.connect_named(right_source, "out", join, "right").unwrap();
    c.connect_named(join, "out", split, "in").unwrap();
    c.connect_named(split, "left", rejoin, "left").unwrap();
    c.connect_named(split, "right", rejoin, "right").unwrap();
    c.connect_named(rejoin, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(
        c.last_report().width(c.output_ref(join, "out").unwrap()),
        Width { voices: 8, sides: 2 }
    );
    assert_eq!(
        c.last_report().width(c.output_ref(split, "left").unwrap()),
        Width { voices: 8, sides: 1 }
    );
    let mut left = [0.0; 512];
    let mut right = [0.0; 512];
    for _ in 0..3 {
        parts.engine.process(&mut left, &mut right);
    }
    let sum = |count| (0..count).map(|voice| 0.01 + voice as f32 * 0.01).sum::<f32>();
    assert_eq!(left, [sum(8); 512]);
    assert_eq!(right, [sum(4); 512]);
}

/// Selects one input voice so an engine test can inspect every voice independently.
#[derive(Debug)]
struct SelectVoice {
    voice: usize,
}

impl SelectVoice {
    const SIGNAL_IN: usize = SELECT_VOICE.input_index("in").unwrap();
    const SIGNAL_OUT: usize = SELECT_VOICE.output_index("out").unwrap();
    const SELECT: usize = SELECT_VOICE.param_index("select").unwrap();
}

/// Bundle-aware sink for inspecting a copied module's individual outputs.
static SELECT_VOICE: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
    id: "test.select-voice",
    name: "Select test voice",
    outputs: &[loom_core::PortSpec {
        channels: loom_core::Channels::PerChannel,
        ..test_modules::VOICES.outputs[0]
    }],
    params: &[loom_core::ParamSpec {
        name: "select",
        label: "Select",
        description: "Voice to inspect.",
        min: 1.0,
        max: 16.0,
        default: 1.0,
        unit: "",
        options: loom_core::ParamOptions::DEFAULT,
    }
    .integer()],
    panel: loom_core::Panel::EMPTY,
    role: loom_core::ModuleRole::Dsp(|_| Box::new(SelectVoice { voice: 0 })),
    ..test_modules::VOICES
};

impl loom_core::Module for SelectVoice {
    fn process(&mut self, _: &loom_core::ProcessContext<'_>, io: &mut loom_core::PortIo<'_>) {
        let input = io.voice_input(Self::SIGNAL_IN, self.voice, 0);
        io.output(Self::SIGNAL_OUT).copy_from_slice(input);
    }
    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::SELECT {
            self.voice = value.round() as usize - 1;
        }
    }
}

/// Eight seeded copies differ and stay repeatable when the engine supplies voice identity.
#[test]
fn eight_seeded_voices_differ_and_repeat() {
    let play = || {
        let mut registry = Registry::new();
        register_all(&mut registry).unwrap();
        test_modules::register(&mut registry).unwrap();
        registry.register(&SELECT_VOICE).unwrap();
        let mut parts = create_engine(
            Arc::new(registry),
            EngineConfig {
                output_safety: OutputSafety::Off,
                ..EngineConfig::default()
            },
        );
        let c = &mut parts.controller;
        let source = c.add_module("test.voices").unwrap();
        let random = c.add_module("loom.rand.random").unwrap();
        let select = c.add_module("test.select-voice").unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.set_param_named(source, "voices", 8.0).unwrap();
        c.set_param_named(source, "value", 0.7).unwrap();
        c.connect_named(source, "out", random, "trigger").unwrap();
        c.connect_named(random, "out", select, "in").unwrap();
        c.connect_named(select, "out", out, "in").unwrap();
        c.commit().unwrap();
        let mut values = Vec::new();
        for voice in 1..=8 {
            parts
                .controller
                .set_param_named(select, "select", voice as f32)
                .unwrap();
            values.push(render(&mut parts)[511]);
        }
        values
    };
    let first = play();
    assert_eq!(play(), first);
    for (i, value) in first.iter().enumerate() {
        assert!(!first[..i].contains(value));
    }
}

/// Queue pressure rejects a structural edit before changing graph values or schedule.
#[test]
fn a_full_command_queue_preserves_structural_values() {
    let mut parts = engine_with(EngineConfig {
        command_capacity: 4,
        output_safety: OutputSafety::Off,
        ..EngineConfig::default()
    });
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(source, "out", add, "a").unwrap();
    c.connect_named(add, "out", out, "in").unwrap();
    c.commit().unwrap();
    let before = render(&mut parts);
    assert!(matches!(
        parts.controller.set_param_named(source, "voices", 8.0),
        Err(loom_core::LoomError::QueueFull)
    ));
    assert_eq!(parts.controller.graph().node(source).unwrap().params[0], 1.0);
    assert_eq!(render(&mut parts), before);
}

/// Invalid saved mappings are refused with their controller and parameter named.
#[test]
fn a_patch_mapping_a_structural_parameter_is_refused() {
    let patch = loom_patch::Patch::from_ron(
        r#"Patch(version: 1,
        modules: [(id: 1, kind: "test.voices@1")],
        midi: [(target: (1, "voices"), channel: 0, controller: 12)])"#,
    )
    .unwrap();
    let mut parts = engine();
    let message = patch.load_into(&mut parts.controller).unwrap_err().to_string();
    assert!(message.contains("CC 12") && message.contains("voices"), "{message}");
}

/// Stores a structural integer so its callback delivery can be observed in audio.
#[derive(Debug)]
struct StructuralMirror {
    enabled: bool,
}

/// A structural parameter need not be named voices to reach an existing instance.
static STRUCTURAL_MIRROR: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
    id: "test.structural-mirror",
    name: "Structural test mirror",
    inputs: &[],
    outputs: &[loom_core::PortSpec {
        channels: loom_core::Channels::PerChannel,
        ..test_modules::VOICES.outputs[0]
    }],
    params: &[loom_core::ParamSpec {
        name: "enabled",
        label: "Enabled",
        description: "A structural integer value.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: loom_core::ParamOptions::DEFAULT,
    }
    .integer()
    .shapes_graph()],
    panel: loom_core::Panel::EMPTY,
    role: loom_core::ModuleRole::Dsp(|_| Box::new(StructuralMirror { enabled: false })),
    ..test_modules::VOICES
};

impl StructuralMirror {
    const ENABLED: usize = STRUCTURAL_MIRROR.param_index("enabled").unwrap();
    const SIGNAL_OUT: usize = STRUCTURAL_MIRROR.output_index("out").unwrap();
}

impl loom_core::Module for StructuralMirror {
    fn process(&mut self, _: &loom_core::ProcessContext<'_>, io: &mut loom_core::PortIo<'_>) {
        io.output(Self::SIGNAL_OUT).fill(if self.enabled { 1.0 } else { 0.0 });
    }
    fn set_param(&mut self, index: usize, value: f32) {
        if index == Self::ENABLED {
            self.enabled = value >= 0.5;
        }
    }
}

/// The schedule swap delivers a structural value to an already-running module.
#[test]
fn structural_values_reach_existing_instances_at_the_swap() {
    let mut registry = Registry::new();
    register_all(&mut registry).unwrap();
    registry.register(&STRUCTURAL_MIRROR).unwrap();
    let mut parts = create_engine(
        Arc::new(registry),
        EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::default()
        },
    );
    let source = parts.controller.add_module("test.structural-mirror").unwrap();
    let out = parts.controller.add_module("loom.io.audio-out").unwrap();
    parts.controller.connect_named(source, "out", out, "in").unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(render(&mut parts), [0.0; 512]);
    parts.controller.set_param_named(source, "enabled", 1.0).unwrap();
    assert_eq!(render(&mut parts), [1.0; 512]);
}

/// A missing phase voice uses OwnPhase while a real zero phase cable holds zero.
#[test]
fn narrower_phase_bundles_pad_with_own_phase() {
    let mut registry = Registry::new();
    register_all(&mut registry).unwrap();
    test_modules::register(&mut registry).unwrap();
    registry.register(&SELECT_VOICE).unwrap();
    let mut parts = create_engine(
        Arc::new(registry),
        EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::default()
        },
    );
    let c = &mut parts.controller;
    let phases = c.add_module("test.voices").unwrap();
    let pitches = c.add_module("test.voices").unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    let select = c.add_module("test.select-voice").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(phases, "voices", 4.0).unwrap();
    c.set_param_named(phases, "value", 0.0).unwrap();
    c.set_param_named(pitches, "voices", 8.0).unwrap();
    c.connect_named(phases, "out", sine, "phase").unwrap();
    c.connect_named(pitches, "out", sine, "pitch").unwrap();
    c.connect_named(sine, "out", select, "in").unwrap();
    c.connect_named(select, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(render(&mut parts), [0.0; 512], "voice 1 has a real zero phase cable");
    parts.controller.set_param_named(select, "select", 5.0).unwrap();
    let own = render(&mut parts);
    assert!(own.iter().any(|value| value.abs() > 0.5), "voice 5 runs its own phase");
}

/// A phase chain whose individual copies can be read without summing them.
fn phase_engine() -> (EngineParts, loom_core::ModuleId, loom_core::ModuleId) {
    let mut registry = Registry::new();
    register_all(&mut registry).unwrap();
    test_modules::register(&mut registry).unwrap();
    registry.register(&SELECT_VOICE).unwrap();
    let mut parts = create_engine(
        Arc::new(registry),
        EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::default()
        },
    );
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let phase = c.add_module("loom.phase.generator").unwrap();
    let select = c.add_module("test.select-voice").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(source, "voices", 4.0).unwrap();
    c.connect_named(source, "out", phase, "pitch").unwrap();
    c.connect_named(phase, "phase", select, "in").unwrap();
    c.connect_named(select, "out", out, "in").unwrap();
    c.commit().unwrap();
    (parts, source, select)
}

/// Increasing width resets added voices while the original voice keeps its running phase.
#[test]
fn added_voices_start_reset_and_surviving_voices_keep_state() {
    let (mut playing, source, select) = phase_engine();
    let (mut reference, _, _) = phase_engine();
    assert_eq!(render(&mut playing), render(&mut reference));
    playing.controller.set_param_named(source, "voices", 8.0).unwrap();
    playing.controller.set_param_named(select, "select", 5.0).unwrap();
    let (mut left, mut right) = ([0.0; 512], [0.0; 512]);
    #[cfg(debug_assertions)]
    assert_no_alloc::assert_no_alloc(|| playing.engine.process(&mut left, &mut right));
    #[cfg(not(debug_assertions))]
    playing.engine.process(&mut left, &mut right);
    assert_eq!(left[0], 0.0, "new voice 5 begins at reset phase");
    #[cfg(debug_assertions)]
    assert_no_alloc::assert_no_alloc(|| reference.engine.process(&mut left, &mut right));
    #[cfg(not(debug_assertions))]
    reference.engine.process(&mut left, &mut right);
    playing.controller.set_param_named(select, "select", 1.0).unwrap();
    let (mut actual, mut actual_right) = ([0.0; 512], [0.0; 512]);
    #[cfg(debug_assertions)]
    assert_no_alloc::assert_no_alloc(|| playing.engine.process(&mut actual, &mut actual_right));
    #[cfg(not(debug_assertions))]
    playing.engine.process(&mut actual, &mut actual_right);
    #[cfg(debug_assertions)]
    assert_no_alloc::assert_no_alloc(|| reference.engine.process(&mut left, &mut right));
    #[cfg(not(debug_assertions))]
    reference.engine.process(&mut left, &mut right);
    assert_eq!(actual, left, "voice 1 keeps the phase it had before the swap");
}

/// Shrinking returns every removed copy for reuse without freeing on the audio thread.
#[test]
fn removed_voice_copies_return_to_the_control_thread() {
    let mut parts = engine_with(EngineConfig {
        max_modules: 9,
        output_safety: OutputSafety::Off,
        ..EngineConfig::default()
    });
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let add = c.add_module("loom.math.add").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(source, "out", add, "a").unwrap();
    c.connect_named(add, "out", out, "in").unwrap();
    c.commit().unwrap();
    render(&mut parts);
    for count in [8.0, 4.0, 8.0, 1.0, 8.0] {
        parts.controller.set_param_named(source, "voices", count).unwrap();
        let sum = (0..count as usize).map(|voice| 0.01 + voice as f32 * 0.01).sum::<f32>();
        assert_eq!(render(&mut parts), [sum; 512]);
        parts.controller.collect_garbage();
    }
}

/// Rounded unchanged counts send ordinary edits without compiling or swapping.
#[test]
fn unchanged_structural_values_do_not_swap_schedules() {
    use std::sync::atomic::Ordering;
    let mut parts = engine();
    let source = parts.controller.add_module("test.voices").unwrap();
    let out = parts.controller.add_module("loom.io.audio-out").unwrap();
    parts.controller.connect_named(source, "out", out, "in").unwrap();
    parts.controller.commit().unwrap();
    render(&mut parts);
    let revision = parts.controller.revision();
    let swaps = parts.controller.stats().schedule_swaps.load(Ordering::Relaxed);
    parts
        .controller
        .set_params(
            source,
            &[
                (test_modules::VOICES.param_index("voices").unwrap(), 1.4),
                (test_modules::VOICES.param_index("value").unwrap(), 0.02),
            ],
        )
        .unwrap();
    assert_eq!(render(&mut parts), [0.02; 512]);
    assert_eq!(parts.controller.revision(), revision);
    assert_eq!(parts.controller.stats().schedule_swaps.load(Ordering::Relaxed), swaps);
}

/// A Voices output owns its capacity even when an ordinary input carries more voices.
#[test]
fn a_bundle_output_module_is_never_copied() {
    static SOURCE: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
        id: "test.bundle-source-with-input",
        inputs: &[loom_core::PortSpec {
            channels: loom_core::Channels::Downmix,
            ..test_modules::VOICES.inputs[0]
        }],
        ..test_modules::VOICES
    };
    let mut registry = Registry::new();
    register_all(&mut registry).unwrap();
    test_modules::register(&mut registry).unwrap();
    registry.register(&SOURCE).unwrap();
    let mut parts = create_engine(
        Arc::new(registry),
        EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::default()
        },
    );
    let c = &mut parts.controller;
    let four = c.add_module("test.voices").unwrap();
    let two = c.add_module(SOURCE.id).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(four, "voices", 4.0).unwrap();
    c.set_param_named(two, "voices", 2.0).unwrap();
    // Downmix owns one input voice; the source fans that value out to both outputs.
    c.connect_named(four, "out", two, "in").unwrap();
    c.connect_named(two, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(c.last_report().width(c.output_ref(two, "out").unwrap()).voices, 2);
    assert_eq!(render(&mut parts), [(0.01f32 + 0.01) + (0.01 + 0.01 + 0.01); 512]);
}

/// Initial structural values leave siblings fresh, preserving their first reset.
#[test]
fn fresh_structural_edits_do_not_publish_an_incomplete_graph() {
    use std::sync::atomic::Ordering;
    let mut parts = engine_with(EngineConfig {
        command_capacity: 3,
        output_safety: OutputSafety::Off,
        ..EngineConfig::default()
    });
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let smooth = c.add_module("test.smooth").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_params(
        source,
        &[
            (test_modules::VOICES.param_index("voices").unwrap(), 8.0),
            (test_modules::VOICES.param_index("value").unwrap(), 0.02),
        ],
    )
    .unwrap();
    c.set_param_named(smooth, "level", 0.25).unwrap();
    c.connect_named(smooth, "out", out, "in").unwrap();
    assert!(c.has_uncommitted_changes());
    assert_eq!(c.stats().schedule_swaps.load(Ordering::Relaxed), 0);
    c.commit().unwrap();
    let mut left = [0.0; 512];
    let mut right = [0.0; 512];
    parts.engine.process(&mut left, &mut right);
    // After the fade, the very first buffer is already at the initial target.
    assert_eq!(left[511], 0.25);
    assert_eq!(parts.controller.stats().schedule_swaps.load(Ordering::Relaxed), 1);
}

/// Dirty live graphs keep count edits staged until their owner's explicit commit.
#[test]
fn dirty_live_graphs_do_not_publish_structural_edits_early() {
    use std::sync::atomic::Ordering;
    let mut parts = engine();
    let source = parts.controller.add_module("test.voices").unwrap();
    let out = parts.controller.add_module("loom.io.audio-out").unwrap();
    parts.controller.connect_named(source, "out", out, "in").unwrap();
    parts.controller.commit().unwrap();
    render(&mut parts);
    let swaps = parts.controller.stats().schedule_swaps.load(Ordering::Relaxed);
    let smooth = parts.controller.add_module("test.smooth").unwrap();
    parts.controller.set_param_named(source, "voices", 8.0).unwrap();
    parts.controller.set_param_named(smooth, "level", 0.25).unwrap();
    assert_eq!(render(&mut parts), [0.01; 512]);
    assert_eq!(parts.controller.stats().schedule_swaps.load(Ordering::Relaxed), swaps);
    parts.controller.commit().unwrap();
    assert!(render(&mut parts)[511] > 0.3);
}

/// Several non-default source counts load with one swap and a bounded command queue.
#[test]
fn loading_polyphonic_sources_commits_once() {
    use std::sync::atomic::Ordering;
    let patch = loom_patch::Patch::from_ron(
        r#"Patch(version: 1, modules: [
        (id: 1, kind: "test.voices@1", params: {"voices": 8.0, "value": 0.02}),
        (id: 2, kind: "test.voices@1", params: {"voices": 8.0, "value": 0.03}),
        (id: 3, kind: "test.voices@1", params: {"voices": 8.0, "value": 0.04})])"#,
    )
    .unwrap();
    let mut parts = engine_with(EngineConfig {
        command_capacity: 5,
        ..EngineConfig::default()
    });
    patch.load_into(&mut parts.controller).unwrap();
    for id in 1..=3 {
        let node = parts
            .controller
            .graph()
            .node(loom_core::ModuleId(id))
            .expect("loaded source");
        assert_eq!(node.params[node.descriptor.param_index("voices").unwrap()], 8.0);
        assert_eq!(
            parts
                .controller
                .last_report()
                .width(parts.controller.output_ref(loom_core::ModuleId(id), "out").unwrap())
                .voices,
            8
        );
    }
    render(&mut parts);
    assert_eq!(parts.controller.stats().schedule_swaps.load(Ordering::Relaxed), 1);
}

/// Flat insertion stages modules and links without publishing a schedule.
#[test]
fn adding_a_flat_polyphonic_patch_does_not_commit() {
    use std::sync::atomic::Ordering;
    let patch = loom_patch::Patch::from_ron(
        r#"Patch(version: 1, modules: [
        (id: 1, kind: "test.voices@1", params: {"voices": 8.0})])"#,
    )
    .unwrap();
    let mut parts = engine();
    let flat = patch
        .flatten(parts.controller.registry(), &mut loom_patch::flatten::FlatIds::new())
        .unwrap();
    flat.add_to(&mut parts.controller, &|id| loom_core::ModuleId(id.0 + 100))
        .unwrap();
    render(&mut parts);
    assert!(parts.controller.has_uncommitted_changes());
    assert_eq!(parts.controller.stats().schedule_swaps.load(Ordering::Relaxed), 0);
    parts.controller.commit().unwrap();
    render(&mut parts);
    assert_eq!(parts.controller.stats().schedule_swaps.load(Ordering::Relaxed), 1);
}

/// Unsynced MIDI cannot replace a batch's initial value on newly added voice copies.
#[test]
fn structural_batches_compile_the_screen_value_after_midi_feedback() {
    static MIRROR: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
        id: "test.poly-smooth",
        inputs: &[loom_core::PortSpec {
            channels: loom_core::Channels::Downmix,
            ..test_modules::VOICES.inputs[0]
        }],
        ..test_modules::SMOOTH
    };
    let mut registry = Registry::new();
    register_all(&mut registry).unwrap();
    test_modules::register(&mut registry).unwrap();
    registry.register(&SELECT_VOICE).unwrap();
    registry.register(&MIRROR).unwrap();
    let mut parts = create_engine(
        Arc::new(registry),
        EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::default()
        },
    );
    let c = &mut parts.controller;
    let source = c.add_module("test.voices").unwrap();
    let mirror = c.add_module(MIRROR.id).unwrap();
    let select = c.add_module(SELECT_VOICE.id).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(source, "out", mirror, "in").unwrap();
    c.connect_named(mirror, "out", select, "in").unwrap();
    c.connect_named(select, "out", out, "in").unwrap();
    c.map_midi_named(
        MidiControl {
            channel: 0,
            controller: 12,
        },
        mirror,
        "level",
    )
    .unwrap();
    c.commit().unwrap();
    render(&mut parts);
    assert!(parts.midi.send(loom_core::MidiEvent::control_change(0, 0, 12, 127)));
    render(&mut parts);
    parts
        .controller
        .set_params_across(&[
            (source, test_modules::VOICES.param_index("voices").unwrap(), 2.0),
            (mirror, MIRROR.param_index("level").unwrap(), 0.25),
        ])
        .unwrap();
    parts.controller.set_param_named(select, "select", 2.0).unwrap();
    let (mut left, mut right) = ([0.0; 512], [0.0; 512]);
    parts.engine.process(&mut left, &mut right);
    assert_eq!(left, [0.25; 512]);
    assert_eq!(
        parts.controller.graph().node(mirror).unwrap().params[MIRROR.param_index("level").unwrap()],
        0.25
    );
}

/// Bundle-aware modules cannot declare inputs whose implicit copying cannot happen.
#[test]
fn bundle_descriptors_refuse_per_channel_and_stereo_inputs() {
    for channels in [loom_core::Channels::PerChannel, loom_core::Channels::Stereo] {
        let descriptor = loom_core::ModuleDescriptor {
            inputs: Box::leak(Box::new([loom_core::PortSpec {
                channels,
                ..test_modules::VOICES.inputs[0]
            }])),
            ..test_modules::VOICES
        };
        let message = Registry::new()
            .register(Box::leak(Box::new(descriptor)))
            .unwrap_err()
            .to_string();
        assert!(message.contains("Voices or Downmix"), "{message}");
    }
}

/// A hand-edited structural modulation error names the patch target before remapping.
#[test]
fn structural_modulation_errors_name_the_patch_target() {
    let patch = loom_patch::Patch::from_ron(
        r#"Patch(version: 1,
        modules: [(id: 1, kind: "test.linear@1"), (id: 7, kind: "test.voices@1")],
        modulation: [(source: (1, "out"), target: (7, "voices"), depth: 0.5)])"#,
    )
    .unwrap();
    let mut parts = engine();
    let flat = patch
        .flatten(parts.controller.registry(), &mut loom_patch::flatten::FlatIds::new())
        .unwrap();
    let error = flat
        .add_to(&mut parts.controller, &|id| loom_core::ModuleId(id.0 + 100))
        .unwrap_err();
    assert!(
        matches!(error, loom_patch::PatchError::StructuralModulationTarget {
        module: 7, ref param,
    } if param == "voices"),
        "{error}"
    );
}

/// A padded sidechain uses the local signal, while existing sidechain voices still key externally.
#[test]
fn padded_dynamics_sidechains_use_the_voices_own_signal() {
    let mut registry = Registry::new();
    register_all(&mut registry).unwrap();
    test_modules::register(&mut registry).unwrap();
    registry.register(&SELECT_VOICE).unwrap();
    let mut parts = create_engine(
        Arc::new(registry),
        EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::default()
        },
    );
    let c = &mut parts.controller;
    let audio = c.add_module("test.voices").unwrap();
    let side = c.add_module("test.voices").unwrap();
    let dynamics = c.add_module("loom.dyn.dynamics").unwrap();
    let select = c.add_module(SELECT_VOICE.id).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(audio, "voices", 8.0).unwrap();
    c.set_param_named(audio, "value", 0.7).unwrap();
    c.set_param_named(side, "voices", 4.0).unwrap();
    c.set_param_named(side, "value", 0.0).unwrap();
    c.connect_named(audio, "out", dynamics, "in").unwrap();
    c.connect_named(side, "out", dynamics, "sidechain").unwrap();
    c.connect_named(dynamics, "gain", select, "in").unwrap();
    c.connect_named(select, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(
        render(&mut parts),
        [1.0; 512],
        "voice 1 keys from silent external input"
    );
    parts.controller.set_param_named(select, "select", 5.0).unwrap();
    assert!(
        render(&mut parts).iter().all(|&gain| gain < 0.8),
        "voice 5 keys from its loud signal"
    );
}

/// A padded Chance lane follows its predecessor, including cascaded missing lanes.
#[test]
fn padded_chance_lanes_follow_the_lane_above() {
    let mut registry = Registry::new();
    register_all(&mut registry).unwrap();
    test_modules::register(&mut registry).unwrap();
    registry.register(&SELECT_VOICE).unwrap();
    let mut parts = create_engine(
        Arc::new(registry),
        EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::default()
        },
    );
    let c = &mut parts.controller;
    let eight = c.add_module("test.voices").unwrap();
    let four = c.add_module("test.voices").unwrap();
    let chance = c.add_module("loom.rand.chance").unwrap();
    let select = c.add_module(SELECT_VOICE.id).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(eight, "voices", 8.0).unwrap();
    c.set_param_named(eight, "value", 0.7).unwrap();
    c.set_param_named(four, "voices", 4.0).unwrap();
    c.set_param_named(four, "value", 0.8).unwrap();
    c.set_param_named(chance, "chance2", 1.0).unwrap();
    c.set_param_named(chance, "chance3", 1.0).unwrap();
    c.connect_named(eight, "out", chance, "trigger1").unwrap();
    c.connect_named(four, "out", chance, "trigger2").unwrap();
    c.connect_named(chance, "a2", select, "in").unwrap();
    c.connect_named(select, "out", out, "in").unwrap();
    c.commit().unwrap();
    assert_eq!(render(&mut parts), [0.8; 512]);
    parts.controller.set_param_named(select, "select", 5.0).unwrap();
    assert_eq!(render(&mut parts), [0.7f32 + 4.0 * 0.01; 512]);
    parts.controller.connect_named(chance, "a3", select, "in").unwrap();
    parts.controller.commit().unwrap();
    assert_eq!(render(&mut parts), [0.7f32 + 4.0 * 0.01; 512]);
}
