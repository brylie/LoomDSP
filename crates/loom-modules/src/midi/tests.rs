use super::*;
use loom_core::testing::run;
use loom_core::{MidiEvent, ModuleConfig};

/// The `gate` display value is 1 while a note is held and 0 after its
/// note-off (#106).
#[test]
fn display_shows_a_held_note() {
    let ModuleRole::Dsp(create) = NOTE_IN.role else {
        unreachable!()
    };
    let mut note_in = create(&ModuleConfig {
        sample_rate: 1_000.0,
        max_block: 64,
    });
    for (i, p) in NOTE_IN.params.iter().enumerate() {
        note_in.set_param(i, p.default);
    }
    let mut display = [f32::NAN];
    let (mut pitch, mut gate, mut velocity) = ([0.0; 64], [0.0; 64], [0.0; 64]);

    note_in.displays(&mut display);
    assert_eq!(display[0], 0.0, "no note yet");

    let on = [MidiEvent::note_on(10, 0, 60, 100)];
    run(
        note_in.as_mut(),
        1_000.0,
        &[],
        &[],
        &mut [&mut pitch, &mut gate, &mut velocity],
        &on,
    );
    note_in.displays(&mut display);
    assert_eq!(gate[63], 1.0);
    assert_eq!(display[0], 1.0, "held");

    let off = [MidiEvent::note_off(10, 0, 60)];
    run(
        note_in.as_mut(),
        1_000.0,
        &[],
        &[],
        &mut [&mut pitch, &mut gate, &mut velocity],
        &off,
    );
    note_in.displays(&mut display);
    assert_eq!(gate[63], 0.0);
    assert_eq!(display[0], 0.0, "released");
}

/// A controller module at 1 kHz with its defaults, then `params`.
fn controller(params: &[(usize, f32)]) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = CONTROLLER_IN.role else {
        unreachable!()
    };
    let mut module = create(&ModuleConfig {
        sample_rate: 1_000.0,
        max_block: 512,
    });
    for (i, p) in CONTROLLER_IN.params.iter().enumerate() {
        module.set_param(i, p.default);
    }
    for &(i, value) in params {
        module.set_param(i, value);
    }
    module.reset();
    module
}

/// Plays `events` through `module` over `frames` frames, `block`
/// frames at a time, and returns the output.
fn play(module: &mut dyn Module, frames: usize, block: usize, events: &[MidiEvent]) -> Vec<f32> {
    let mut out = Vec::new();
    for start in (0..frames).step_by(block) {
        let len = block.min(frames - start);
        let in_block: Vec<MidiEvent> = events
            .iter()
            .filter(|e| (start..start + len).contains(&(e.frame as usize)))
            .map(|e| MidiEvent {
                frame: e.frame - start as u32,
                ..*e
            })
            .collect();
        let mut chunk = vec![f32::NAN; len];
        run(module, 1_000.0, &[], &[], &mut [&mut chunk], &in_block);
        out.extend(chunk);
    }
    out
}

/// With no smoothing, a controller change, a bend, and aftertouch land
/// on the sample of their offset at block sizes 1, 77, and 512, with
/// the ranges of the spec (p1-modules.md, Acceptance criteria).
#[test]
fn controller_events_land_on_their_sample() {
    /// A source, its events, and (frame, expected value) checks.
    type Case = (f32, Vec<MidiEvent>, Vec<(usize, f32)>);
    let cases: [Case; 3] = [
        (
            0.0,
            vec![
                MidiEvent::control_change(100, 0, 1, 127),
                MidiEvent::control_change(300, 0, 2, 50),
                MidiEvent::control_change(450, 0, 1, 0),
                MidiEvent::control_change(600, 3, 1, 64),
            ],
            vec![
                (0, 0.0),
                (99, 0.0),
                (100, 1.0),
                (300, 1.0),
                (449, 1.0),
                (450, 0.0),
                (600, 64.0 / 127.0),
            ],
        ),
        (
            1.0,
            vec![
                MidiEvent::pitch_bend(77, 0, 8191),
                MidiEvent::pitch_bend(200, 5, -8192),
                MidiEvent::pitch_bend(513, 0, 0),
                MidiEvent::pitch_bend(700, 0, -4096),
            ],
            vec![
                (0, 0.0),
                (76, 0.0),
                (77, 1.0),
                (199, 1.0),
                (200, -1.0),
                (513, 0.0),
                (700, -0.5),
            ],
        ),
        (
            2.0,
            vec![
                MidiEvent::channel_pressure(1, 0, 127),
                MidiEvent::channel_pressure(511, 0, 0),
                MidiEvent::control_change(600, 0, 1, 127),
            ],
            vec![(0, 0.0), (1, 1.0), (510, 1.0), (511, 0.0), (700, 0.0)],
        ),
    ];
    for (source, events, checks) in cases {
        for block in [1, 77, 512] {
            let mut module = controller(&[(0, source), (3, 0.0)]);
            let out = play(module.as_mut(), 1_000, block, &events);
            for &(frame, value) in &checks {
                assert!(
                    (out[frame] - value).abs() < 1e-6,
                    "source {source}, block {block}, frame {frame}: {} not {value}",
                    out[frame]
                );
            }
        }
    }
}

/// Smoothing starts on the event's sample and gives the same output at
/// every block size.
#[test]
fn controller_smoothing_is_block_size_invariant() {
    let events = [
        MidiEvent::control_change(100, 0, 1, 127),
        MidiEvent::control_change(140, 0, 1, 20),
    ];
    let reference = play(controller(&[(3, 0.05)]).as_mut(), 400, 400, &events);
    assert_eq!(reference[99], 0.0, "nothing before the event");
    assert!(reference[100] > 0.0 && reference[100] < 0.2, "{}", reference[100]);
    // About 99% of the way in 50 ms.
    assert!((reference[149 + 50] - 20.0 / 127.0).abs() < 0.01);
    for block in [1, 77, 512] {
        let out = play(controller(&[(3, 0.05)]).as_mut(), 400, block, &events);
        assert_eq!(out, reference, "block {block}");
    }
}

/// `channel` 0 listens to every channel, 1 to 16 to one. Changing the
/// `controller` knob shows that controller's last value.
#[test]
fn controller_channel_and_number() {
    let events = [
        MidiEvent::control_change(0, 0, 7, 127),
        MidiEvent::control_change(1, 4, 1, 127),
    ];
    let out = play(controller(&[(2, 1.0), (3, 0.0)]).as_mut(), 10, 10, &events);
    assert_eq!(out[9], 0.0, "channel 1 ignores channel 5");
    let out = play(controller(&[(2, 5.0), (3, 0.0)]).as_mut(), 10, 10, &events);
    assert_eq!(out[9], 1.0, "channel 5");

    let mut module = controller(&[(3, 0.0)]);
    play(module.as_mut(), 10, 10, &events);
    module.set_param(1, 7.0);
    assert_eq!(play(module.as_mut(), 1, 1, &[])[0], 1.0, "controller 7 was set");
    module.reset();
    assert_eq!(play(module.as_mut(), 1, 1, &[])[0], 0.0, "reset forgets messages");
}
