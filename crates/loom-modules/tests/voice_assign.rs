//! Voice assigner acceptance checks (#676). Spec: `docs/design/polyphony.md`,
//! Voice assigner. Every test runs at block sizes 1, 77, and 512.
use loom_modules::voice::ASSIGN;
use loom_test::{Harness, Render, Signal};

/// A gate high from `on` up to, not including, `off`.
fn gate(on: usize, off: usize) -> Signal {
    Signal::from_fn(move |f| if (on..off).contains(&f) { 1.0 } else { 0.0 })
}

/// The assigner at `voices` output voices in `mode`.
fn assigner(voices: f32, mode: f32) -> Harness {
    Harness::new(&ASSIGN)
        .frames(1024)
        .param("voices", voices)
        .param("mode", mode)
}

/// The output voices whose gate is high at `frame`.
fn high(render: &Render, voices: usize, frame: usize) -> Vec<usize> {
    (0..voices)
        .filter(|&v| render.voice_output("gate", v, 0)[frame] > 0.5)
        .collect()
}

/// The pitches of the output voices whose gate is high at `frame`, sorted.
fn sounding(render: &Render, voices: usize, frame: usize) -> Vec<f32> {
    let mut pitches: Vec<f32> = high(render, voices, frame)
        .into_iter()
        .map(|v| render.voice_output("pitch", v, 0)[frame])
        .collect();
    pitches.sort_by(f32::total_cmp);
    pitches
}

/// How many times an output gate rises over the whole render.
fn rises(render: &Render, voices: usize) -> usize {
    (0..voices)
        .map(|v| {
            let gate = render.voice_output("gate", v, 0);
            // Before the first frame the gate counts as low.
            (0..gate.len())
                .filter(|&f| gate[f] > 0.5 && (f == 0 || gate[f - 1] <= 0.5))
                .count()
        })
        .sum()
}

/// A mono pitch and gate into 8 voices: each edge starts one note on a new
/// voice, so release tails overlap, and a released voice holds its pitch.
#[test]
fn a_mono_sequence_rotates_over_eight_voices() {
    let render = assigner(8.0, 0.0)
        .input("pitch", Signal::from_fn(|f| (f / 100) as f32 * 0.01))
        .input(
            "gate",
            Signal::from_fn(|f| if f % 100 < 50 && f < 600 { 1.0 } else { 0.0 }),
        )
        .check_block_sizes();
    for step in 0..6 {
        assert_eq!(high(&render, 8, step * 100 + 10), vec![step], "step {step}");
    }
    // One output gate per input edge, never more.
    assert_eq!(rises(&render, 8), 6);
    for step in 0..6 {
        assert_eq!(render.voice_output("pitch", step, 0)[900], step as f32 * 0.01);
    }
}

/// Unison 2: a note takes two voices together and its note-off releases both;
/// pressure on the input voice reaches both output voices.
#[test]
fn unison_takes_and_releases_voices_together() {
    let render = assigner(8.0, 0.0)
        .param("unison", 2.0)
        .input("pitch", Signal::constant(0.1))
        .input("gate", gate(100, 400))
        .input("pressure", Signal::from_fn(|f| f as f32 / 1024.0))
        .check_block_sizes();
    let voices = high(&render, 8, 200);
    assert_eq!(voices.len(), 2);
    for &v in &voices {
        assert_eq!(render.voice_output("pressure", v, 0)[200], 200.0 / 1024.0);
        assert_eq!(render.voice_output("pressure", v, 0)[300], 300.0 / 1024.0);
    }
    assert!(high(&render, 8, 500).is_empty());
    assert_eq!(rises(&render, 8), 2);
}

/// A pitch that moves while the gate stays high glides on the same voice,
/// with no retrigger.
#[test]
fn a_held_gate_follows_its_pitch_without_a_retrigger() {
    let render = assigner(8.0, 0.0)
        .input("pitch", Signal::from_fn(|f| f as f32 * 0.0001))
        .input("gate", gate(100, 900))
        .check_block_sizes();
    let gate = render.voice_output("gate", 0, 0);
    assert!(gate[100..900].iter().all(|v| *v == 1.0));
    assert_eq!(render.voice_output("pitch", 0, 0)[500], 500.0 * 0.0001);
    assert_eq!(rises(&render, 8), 1);
}

/// Two input voices on one pitch are two notes on two output voices.
#[test]
fn two_input_voices_on_one_pitch_are_two_notes() {
    let render = assigner(8.0, 0.0)
        .bundle_input("pitch", vec![Signal::constant(0.1), Signal::constant(0.1)])
        .bundle_input("gate", vec![gate(100, 400), gate(100, 400)])
        .check_block_sizes();
    assert_eq!(high(&render, 8, 200).len(), 2);
}

/// 8 voices with unison 3: two notes sound, a third steals the oldest, and
/// the two voices left over stay free.
#[test]
fn unison_three_on_eight_voices_steals_whole_groups() {
    let render = assigner(8.0, 0.0)
        .param("unison", 3.0)
        .bundle_input(
            "pitch",
            vec![Signal::constant(0.1), Signal::constant(0.2), Signal::constant(0.3)],
        )
        .bundle_input("gate", vec![gate(100, 900), gate(200, 900), gate(300, 900)])
        .check_block_sizes();
    assert_eq!(sounding(&render, 8, 250), vec![0.1, 0.1, 0.1, 0.2, 0.2, 0.2]);
    assert_eq!(sounding(&render, 8, 350), vec![0.2, 0.2, 0.2, 0.3, 0.3, 0.3]);
}

/// Five held notes into 4 voices, the fifth at frame 300, with voice 3 (not
/// the oldest) the quietest on `level`.
fn quietest(level: bool) -> Render {
    let mut h = assigner(4.0, 1.0)
        .bundle_input("pitch", (0..5).map(|v| Signal::constant(v as f32 * 0.1)).collect())
        .bundle_input("gate", (0..5).map(|v| gate(100 + v * 50, 900)).collect());
    if level {
        h = h.bundle_input(
            "level",
            (0..4)
                .map(|v| Signal::constant(if v == 2 { 0.1 } else { 0.9 }))
                .collect(),
        );
    }
    h.check_block_sizes()
}

/// Quietest steals the voice with the lowest `level`; with `level`
/// unpatched it acts as Oldest.
#[test]
fn quietest_steals_by_level_and_falls_back_to_oldest() {
    let quiet = quietest(true);
    assert_eq!(sounding(&quiet, 4, 400), vec![0.0, 0.1, 0.3, 0.4]);
    let oldest = quietest(false);
    assert_eq!(sounding(&oldest, 4, 400), vec![0.1, 0.2, 0.3, 0.4]);
}

/// A 4-voice pitch bundle with a mono gate is a 4-note chord on one edge.
#[test]
fn a_pitch_bundle_with_a_mono_gate_plays_a_chord() {
    let render = assigner(8.0, 0.0)
        .bundle_input("pitch", (0..4).map(|v| Signal::constant(v as f32 * 0.1)).collect())
        .input("gate", gate(100, 300))
        .check_block_sizes();
    assert_eq!(sounding(&render, 8, 200), vec![0.0, 0.1, 0.2, 0.3]);
    assert_eq!(rises(&render, 8), 4);
}

/// With no cable on `gate`, nothing plays.
#[test]
fn nothing_plays_without_a_gate_cable() {
    let render = assigner(8.0, 0.0)
        .bundle_input("pitch", (0..4).map(|v| Signal::constant(v as f32 * 0.1)).collect())
        .check_block_sizes();
    assert_eq!(rises(&render, 8), 0);
    assert!(high(&render, 8, 500).is_empty());
}

/// 16 input voices into 4 output voices steal by the mode: the last four
/// notes sound.
#[test]
fn sixteen_input_voices_steal_into_four() {
    let render = assigner(4.0, 0.0)
        .bundle_input("pitch", (0..16).map(|v| Signal::constant(v as f32 * 0.01)).collect())
        .bundle_input("gate", (0..16).map(|v| gate(100 + v * 20, 900)).collect())
        .check_block_sizes();
    let last_four: Vec<f32> = (12..16).map(|v| v as f32 * 0.01).collect();
    assert_eq!(sounding(&render, 4, 600), last_four);
}

/// A note stolen while its input gate stays high stays silent, even when a
/// voice frees, until its input gate rises again.
#[test]
fn a_stolen_note_stays_silent_until_its_gate_rises_again() {
    let render = assigner(2.0, 0.0)
        .bundle_input(
            "pitch",
            vec![Signal::constant(0.1), Signal::constant(0.2), Signal::constant(0.3)],
        )
        .bundle_input(
            "gate",
            vec![
                Signal::from_fn(|f| if (100..500).contains(&f) || f >= 600 { 1.0 } else { 0.0 }),
                gate(200, 400),
                gate(300, 900),
            ],
        )
        .check_block_sizes();
    // Input voice 1's note was stolen at 300.
    assert_eq!(sounding(&render, 2, 350), vec![0.2, 0.3]);
    // Input voice 2's note ended at 400; input voice 1 does not come back.
    assert_eq!(sounding(&render, 2, 450), vec![0.3]);
    // Its gate rose again at 600.
    assert_eq!(sounding(&render, 2, 650), vec![0.1, 0.3]);
}

/// On one frame a lower input voice rises while a higher one, not the
/// oldest, falls: the new note takes the ending note's voice, and the oldest
/// keeps sounding. Read by rising index in one pass, the rise would steal the
/// oldest.
#[test]
fn note_offs_come_before_note_ons_on_one_frame() {
    let render = assigner(2.0, 0.0)
        .bundle_input(
            "pitch",
            vec![Signal::constant(0.1), Signal::constant(0.2), Signal::constant(0.3)],
        )
        .bundle_input("gate", vec![gate(300, 900), gate(100, 900), gate(200, 300)])
        .check_block_sizes();
    assert_eq!(sounding(&render, 2, 250), vec![0.2, 0.3]);
    assert_eq!(sounding(&render, 2, 350), vec![0.1, 0.2]);
}

/// NaN on `pitch` up to and including the last high gate frame does not
/// stick: the released voice holds the last finite pitch.
#[test]
fn a_non_finite_pitch_does_not_stick_after_the_note_off() {
    let render = assigner(8.0, 0.0)
        .input(
            "pitch",
            Signal::from_fn(|f| match f {
                ..200 => 0.2,
                200..300 => f32::NAN,
                _ => 0.5,
            }),
        )
        .input("gate", gate(100, 300))
        .check_block_sizes();
    assert_eq!(render.voice_output("pitch", 0, 0)[250], 0.2);
    assert_eq!(render.voice_output("pitch", 0, 0)[400], 0.2);
}

/// The module recovers from non-finite inputs, and a copy takes over its
/// running state.
#[test]
fn the_harness_checks_recovery_and_copied_state() {
    let h = assigner(8.0, 0.0)
        .frames(4096)
        .param("unison", 2.0)
        .input("pitch", Signal::from_fn(|f| (f / 100) as f32 * 0.01))
        .input("gate", Signal::from_fn(|f| if f % 100 < 50 { 1.0 } else { 0.0 }));
    h.check_non_finite_recovery();
    h.check_copy_state(389);
}
