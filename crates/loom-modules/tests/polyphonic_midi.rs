//! Polyphonic MIDI Note input acceptance and expression routing.
use loom_core::{MidiEvent, MidiInputId};
use loom_modules::midi::NOTE_IN;
use loom_test::{Harness, Signal};

/// A source with distinct, input-tagged events at all supported block sizes.
fn source(voices: usize, events: impl IntoIterator<Item = MidiEvent>) -> Harness {
    Harness::new(&NOTE_IN)
        .frames(1024)
        .param("voices", voices as f32)
        .midi(events)
}

/// Announces a zone using RPN 6 on a physical input.
fn zone(input: u16, channel: u8, members: u8, frame: u32) -> [MidiEvent; 3] {
    [
        MidiEvent::control_change(frame, channel, 101, 0),
        MidiEvent::control_change(frame, channel, 100, 6),
        MidiEvent::control_change(frame, channel, 6, members),
    ]
    .map(|e| e.from_input(MidiInputId(input)))
}

/// Coincident pitches on separate inputs or channels retain separate lifetimes.
#[test]
fn identity_contains_input_channel_and_pitch() {
    let render = source(
        3,
        [
            MidiEvent::note_on(10, 0, 60, 100).from_input(MidiInputId(1)),
            MidiEvent::note_on(10, 0, 60, 80).from_input(MidiInputId(2)),
            MidiEvent::note_on(10, 1, 60, 60),
            MidiEvent::note_off(100, 0, 60).from_input(MidiInputId(1)),
            MidiEvent::note_off(200, 1, 60),
        ],
    )
    .check_block_sizes();
    assert_eq!(render.voice_output("gate", 0, 0)[100], 0.0);
    assert_eq!(render.voice_output("gate", 1, 0)[100], 1.0);
    assert_eq!(render.voice_output("gate", 2, 0)[199], 1.0);
    assert_eq!(render.voice_output("gate", 2, 0)[200], 0.0);
}

/// Quietest steals the lowest connected level, while Oldest steals the first note.
#[test]
fn stealing_reads_levels_and_pulses_the_gate() {
    let events = [
        MidiEvent::note_on(10, 0, 60, 100),
        MidiEvent::note_on(20, 0, 64, 100),
        MidiEvent::note_on(100, 0, 67, 100),
    ];
    for (mode, victim) in [(0.0, 0), (1.0, 1)] {
        let render = source(2, events)
            .param("mode", mode)
            .bundle_input("level", vec![Signal::constant(0.8), Signal::constant(0.1)])
            .check_block_sizes();
        let gate = render.voice_output("gate", victim, 0);
        assert_eq!(&gate[99..102], &[1.0, 0.0, 1.0]);
        assert_eq!(
            render.voice_output("pitch", victim, 0)[100],
            loom_core::signal::pitch::from_midi_note(67.0)
        );
    }
}

/// An invalid mode falls back according to output capacity without rewriting it.
#[test]
fn mode_fallback_and_mono_return_articulation() {
    let events = [
        MidiEvent::note_on(10, 0, 60, 100),
        MidiEvent::note_on(100, 0, 64, 60),
        MidiEvent::note_off(200, 0, 64),
    ];
    for mode in [0.0, 1.0, 2.0] {
        let render = source(1, events).param("mode", mode).check_block_sizes();
        assert!(render.output("gate")[10..1024].iter().all(|v| *v == 1.0));
        assert_eq!(render.output("pitch")[200], 0.0);
    }
    let render = source(1, events).param("mode", 3.0).check_block_sizes();
    assert_eq!(&render.output("gate")[99..102], &[1.0, 0.0, 1.0]);
    assert_eq!(
        render.output("gate")[200],
        1.0,
        "returning to a held note is not a new note-on"
    );
    for mode in [2.0, 3.0] {
        let render = source(2, events).param("mode", mode).check_block_sizes();
        assert_eq!(render.voice_output("gate", 0, 0)[100], 1.0);
        assert_eq!(render.voice_output("gate", 1, 0)[100], 1.0);
    }
}

/// Poly pressure addresses one key; channel expression addresses held keys only.
#[test]
fn ordinary_expression_and_release_values() {
    let render = source(
        2,
        [
            MidiEvent::note_on(10, 0, 60, 100),
            MidiEvent::note_on(20, 0, 64, 60),
            MidiEvent::poly_pressure(50, 0, 60, 127),
            MidiEvent::control_change(60, 0, 74, 100),
            MidiEvent::note_off(100, 0, 60),
            MidiEvent::channel_pressure(150, 0, 50),
            MidiEvent::pitch_bend(150, 0, 4096),
            MidiEvent::control_change(150, 0, 74, 20),
        ],
    )
    .check_block_sizes();
    assert_eq!(render.voice_output("pressure", 0, 0)[50], 1.0);
    assert_eq!(render.voice_output("pressure", 1, 0)[50], 0.0);
    assert_eq!(render.voice_output("pressure", 0, 0)[900], 1.0);
    assert_eq!(render.voice_output("timbre", 0, 0)[900], 100.0 / 127.0);
    assert_eq!(render.voice_output("pressure", 1, 0)[900], 50.0 / 127.0);
    assert_eq!(render.voice_output("pitch", 0, 0)[900], 0.0);
    assert_eq!(
        render.voice_output("pitch", 1, 0)[900],
        loom_core::signal::pitch::from_midi_note(64.0) + loom_core::signal::pitch::PER_SEMITONE
    );
}

/// MPE managers and members combine bend, and take the maximum pressure/timbre.
#[test]
fn mpe_expression_follows_each_inputs_announced_zone() {
    let events = zone(1, 0, 4, 0).into_iter().chain([
        MidiEvent::note_on(10, 1, 60, 100).from_input(MidiInputId(1)),
        MidiEvent::note_on(10, 1, 60, 100).from_input(MidiInputId(2)),
        MidiEvent::pitch_bend(100, 1, 4096).from_input(MidiInputId(1)),
        MidiEvent::pitch_bend(100, 0, 4096).from_input(MidiInputId(1)),
        MidiEvent::channel_pressure(100, 0, 100).from_input(MidiInputId(1)),
        MidiEvent::channel_pressure(100, 1, 50).from_input(MidiInputId(1)),
        MidiEvent::control_change(100, 0, 74, 40).from_input(MidiInputId(1)),
        MidiEvent::control_change(100, 1, 74, 90).from_input(MidiInputId(1)),
    ]);
    let render = source(2, events).check_block_sizes();
    assert_eq!(render.voice_output("pitch", 0, 0)[99], 0.0);
    assert_eq!(
        render.voice_output("pitch", 0, 0)[100],
        25.0 * loom_core::signal::pitch::PER_SEMITONE
    );
    assert_eq!(render.voice_output("pitch", 1, 0)[100], 0.0);
    assert_eq!(render.voice_output("pressure", 0, 0)[100], 100.0 / 127.0);
    assert_eq!(render.voice_output("timbre", 0, 0)[100], 90.0 / 127.0);
}

/// A late announcement cannot change an earlier note's bend in the same buffer.
#[test]
fn mpe_configuration_is_sample_accurate_and_manager_notes_bend_once() {
    let events = [
        MidiEvent::note_on(10, 1, 60, 100).from_input(MidiInputId(1)),
        MidiEvent::pitch_bend(20, 1, 4096).from_input(MidiInputId(1)),
    ]
    .into_iter()
    .chain(zone(1, 0, 4, 100))
    .chain([
        MidiEvent::note_on(110, 1, 60, 100).from_input(MidiInputId(1)),
        MidiEvent::pitch_bend(120, 1, 4096).from_input(MidiInputId(1)),
        MidiEvent::note_off(140, 1, 60).from_input(MidiInputId(1)),
        MidiEvent::note_on(200, 0, 60, 100).from_input(MidiInputId(1)),
        MidiEvent::pitch_bend(210, 0, 4096).from_input(MidiInputId(1)),
    ]);
    let render = source(1, events).check_block_sizes();
    assert_eq!(render.output("pitch")[99], loom_core::signal::pitch::PER_SEMITONE);
    assert_eq!(
        render.output("gate")[100],
        0.0,
        "entering MPE releases an old ordinary note"
    );
    assert_eq!(
        render.output("pitch")[100],
        loom_core::signal::pitch::PER_SEMITONE,
        "release values hold"
    );
    assert_eq!(render.output("pitch")[110], 0.0, "the old bend was reset");
    assert_eq!(
        render.output("pitch")[120],
        24.0 * loom_core::signal::pitch::PER_SEMITONE
    );
    assert_eq!(render.output("pitch")[210], loom_core::signal::pitch::PER_SEMITONE);
}

/// Builds the acceptance synth. The source precedes its envelope to delay level.
fn synth(voices: usize, channel: u8) -> loom_core::EngineParts {
    let mut parts = loom_core::create_engine(
        loom_modules::registry(),
        loom_core::EngineConfig {
            output_safety: loom_core::OutputSafety::Off,
            ..loom_core::EngineConfig::default()
        },
    );
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    c.set_param_named(note, "voices", voices as f32).unwrap();
    c.set_param_named(note, "channel", f32::from(channel)).unwrap();
    let saw = c.add_module("loom.osc.saw").unwrap();
    let filter = c.add_module("loom.filter.svf").unwrap();
    let envelope = c.add_module("loom.env.envelope").unwrap();
    let amp = c.add_module("loom.level.amp").unwrap();
    let merge = c.add_module("loom.voice.merge").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.set_param_named(amp, "gain", 0.03).unwrap();
    for (from, port, to, input) in [
        (note, "pitch", saw, "pitch"),
        (saw, "out", filter, "in"),
        (filter, "lowpass", amp, "in"),
        (note, "gate", envelope, "gate"),
        (envelope, "out", amp, "level"),
        (envelope, "out", note, "level"),
        (amp, "out", merge, "in"),
        (merge, "out", out, "in"),
    ] {
        c.connect_named(from, port, to, input).unwrap();
    }
    c.commit().unwrap();
    assert_eq!(c.last_report().loops.len(), 1);
    assert_eq!(
        c.last_report().loops[0].inserted_delays,
        vec![(
            c.output_ref(envelope, "out").unwrap(),
            c.input_ref(note, "level").unwrap()
        )]
    );
    parts
}

/// Renders with genuine buffer timestamps, including releases and quiet tails.
fn play_synth(parts: &mut loom_core::EngineParts, block: usize) -> Vec<f32> {
    let mut events = Vec::new();
    for voice in 0..8u8 {
        events.push(MidiEvent::note_on(
            1200 + u32::from(voice) * 17,
            voice,
            48 + voice * 3,
            100,
        ));
        events.push(MidiEvent::note_off(2800 + u32::from(voice) * 31, voice, 48 + voice * 3));
    }
    events.sort_by_key(|event| event.frame);
    let mut result = Vec::new();
    for start in (0..4800).step_by(block) {
        let len = block.min(4800 - start);
        for event in events
            .iter()
            .filter(|event| (start..start + len).contains(&(event.frame as usize)))
        {
            assert!(parts.midi.send(MidiEvent {
                frame: event.frame - start as u32,
                ..*event
            }));
        }
        let mut left = vec![0.0; len];
        let mut right = vec![0.0; len];
        parts.engine.process(&mut left, &mut right);
        assert_eq!(left, right);
        result.extend(left);
    }
    result
}

/// Eight automatic copies are bit-identical to eight independent mono chains.
#[test]
fn acceptance_synth_matches_eight_mono_chains() {
    let mut expected = vec![0.0f32; 4800];
    for channel in 1..=8 {
        let mono = play_synth(&mut synth(1, channel), 77);
        for (sum, sample) in expected.iter_mut().zip(mono) {
            *sum += sample;
        }
    }
    assert!(expected.iter().any(|sample| sample.abs() > 0.01), "the synth must play");
    for block in [1, 77, 512] {
        let actual = play_synth(&mut synth(8, 0), block);
        assert_eq!(
            actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            expected.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            "block {block}"
        );
    }
}

/// Eight voices steal only the oldest, and a repeated note retriggers only itself.
#[test]
fn ninth_note_and_repeated_identity_preserve_other_gates() {
    let events = (0..8)
        .map(|voice| MidiEvent::note_on(10 + voice, 0, 60 + voice as u8, 100))
        .chain([MidiEvent::note_on(100, 0, 64, 100), MidiEvent::note_on(200, 0, 80, 100)]);
    let render = source(8, events).check_block_sizes();
    assert_eq!(&render.voice_output("gate", 4, 0)[99..102], &[1.0, 0.0, 1.0]);
    assert_eq!(&render.voice_output("gate", 0, 0)[199..202], &[1.0, 0.0, 1.0]);
    for voice in 1..8 {
        assert_eq!(render.voice_output("gate", voice, 0)[200], 1.0);
    }
}

/// A schedule width decrease forgets removed voices and a later increase starts silent.
#[test]
fn output_capacity_controls_allocation_across_live_edits() {
    let render = source(4, (0..4).map(|voice| MidiEvent::note_on(10, 0, 60 + voice, 100)))
        .param_at(100.0 / 48000.0, "voices", 1.0)
        .param_at(200.0 / 48000.0, "voices", 4.0)
        .check_block_sizes();
    assert_eq!(render.voice_output("gate", 0, 0)[300], 1.0);
    for voice in 1..4 {
        assert_eq!(render.voice_output("gate", voice, 0)[99], 1.0);
        assert_eq!(render.voice_output("gate", voice, 0)[300], 0.0);
    }
}

/// Manager channel filters expand their own zone, while member filters stay exact.
#[test]
fn upper_and_lower_zones_follow_the_channel_filter() {
    for (manager, member) in [(0, 2), (15, 13)] {
        let events = zone(1, manager, 4, 0).into_iter().chain([
            MidiEvent::note_on(10, member, 60, 100).from_input(MidiInputId(1)),
            MidiEvent::pitch_bend(20, member, 4096).from_input(MidiInputId(1)),
        ]);
        let render = source(1, events)
            .param("channel", f32::from(manager + 1))
            .check_block_sizes();
        assert_eq!(render.output("gate")[10], 1.0);
        assert_eq!(
            render.output("pitch")[20],
            24.0 * loom_core::signal::pitch::PER_SEMITONE
        );
    }
    let events = zone(1, 0, 4, 0)
        .into_iter()
        .chain([MidiEvent::note_on(10, 2, 60, 100).from_input(MidiInputId(1))]);
    assert_eq!(
        source(1, events)
            .param("channel", 2.0)
            .check_block_sizes()
            .output("gate")[10],
        0.0
    );
}

/// Quietest can steal a recent key while the oldest key keeps receiving expression.
#[test]
fn expression_slots_follow_sounding_notes_after_quietest_steals() {
    let render = source(
        16,
        (0..16)
            .map(|voice| MidiEvent::note_on(10 + voice, 0, 60 + voice as u8, 100))
            .chain([
                MidiEvent::note_on(100, 0, 90, 100),
                MidiEvent::poly_pressure(200, 0, 60, 127),
            ]),
    )
    .param("mode", 1.0)
    .bundle_input(
        "level",
        (0..16)
            .map(|voice| Signal::constant(if voice == 15 { 0.0 } else { 1.0 }))
            .collect(),
    )
    .check_block_sizes();
    assert_eq!(render.voice_output("pressure", 0, 0)[200], 1.0);
    assert_eq!(
        render.voice_output("pitch", 15, 0)[101],
        loom_core::signal::pitch::from_midi_note(90.0)
    );
}

/// Stereo envelope levels use their mean, as modulation downmixes do.
#[test]
fn quietest_compares_both_sides_of_a_stereo_level_voice() {
    let render = source(
        2,
        [
            MidiEvent::note_on(10, 0, 60, 100),
            MidiEvent::note_on(20, 0, 64, 100),
            MidiEvent::note_on(100, 0, 67, 100),
        ],
    )
    .param("mode", 1.0)
    .stereo_bundle_input(
        "level",
        vec![
            [Signal::constant(0.0), Signal::constant(1.0)],
            [Signal::constant(0.25), Signal::constant(0.25)],
        ],
    )
    .check_block_sizes();
    assert_eq!(render.voice_output("gate", 0, 0)[100], 1.0);
    assert_eq!(render.voice_output("gate", 1, 0)[100], 0.0);
}

/// The retained voice keeps its audible velocity across either mono/poly crossing.
#[test]
fn count_crossings_preserve_the_surviving_velocity() {
    let poly = source(
        2,
        [MidiEvent::note_on(10, 0, 60, 100), MidiEvent::note_on(20, 0, 64, 40)],
    )
    .param_at(100.0 / 48000.0, "voices", 1.0)
    .check_block_sizes();
    assert_eq!(poly.voice_output("velocity", 0, 0)[99], 100.0 / 127.0);
    assert_eq!(poly.voice_output("velocity", 0, 0)[100], 100.0 / 127.0);
    let mono = source(
        1,
        [
            MidiEvent::note_on(10, 0, 60, 100),
            MidiEvent::note_on(20, 0, 64, 40),
            MidiEvent::note_off(30, 0, 64),
        ],
    )
    .param_at(100.0 / 48000.0, "voices", 2.0)
    .check_block_sizes();
    assert_eq!(mono.voice_output("velocity", 0, 0)[99], 40.0 / 127.0);
    assert_eq!(mono.voice_output("velocity", 0, 0)[100], 40.0 / 127.0);
}

/// Zone activation/deactivation leaves ordinary channels outside it untouched.
#[test]
fn zone_changes_release_only_affected_channels() {
    let events = [
        MidiEvent::note_on(10, 1, 60, 100),
        MidiEvent::note_on(10, 10, 67, 100),
        MidiEvent::control_change(20, 10, 74, 90),
    ]
    .into_iter()
    .map(|e| e.from_input(MidiInputId(1)))
    .chain(zone(1, 0, 4, 100))
    .chain([
        MidiEvent::note_on(150, 1, 60, 100).from_input(MidiInputId(1)),
        MidiEvent::pitch_bend(160, 1, 4096).from_input(MidiInputId(1)),
    ])
    .chain(zone(1, 0, 0, 200));
    let render = source(2, events).check_block_sizes();
    assert_eq!(render.voice_output("gate", 0, 0)[100], 0.0);
    assert_eq!(render.voice_output("gate", 0, 0)[150], 1.0);
    assert_eq!(render.voice_output("gate", 0, 0)[200], 0.0);
    assert_eq!(
        render.voice_output("pitch", 0, 0)[200],
        24.0 * loom_core::signal::pitch::PER_SEMITONE
    );
    assert!(render.voice_output("gate", 1, 0)[10..].iter().all(|v| *v == 1.0));
    assert_eq!(render.voice_output("timbre", 1, 0)[900], 90.0 / 127.0);
}
