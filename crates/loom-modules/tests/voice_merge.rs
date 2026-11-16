//! Voice merge acceptance checks, including its declared gate normal.
use loom_modules::voice::MERGE;
use loom_test::{Harness, Signal};

/// An unpatched gate makes all voices active; sum and average ignore gates.
#[test]
fn merge_modes_and_single_voice() {
    for (mode, expected) in [(0, 9.0), (1, 3.0), (2, 2.0), (3, 2.0), (4, 4.0)] {
        let h = Harness::new(&MERGE)
            .frames(1024)
            .param("mode", mode as f32)
            .bundle_input(
                "in",
                vec![Signal::constant(2.0), Signal::constant(3.0), Signal::constant(4.0)],
            );
        assert!(h.check_block_sizes().output("out").iter().all(|v| *v == expected));
        h.check_copy_state(389);
        let single = Harness::new(&MERGE)
            .frames(1024)
            .param("mode", mode as f32)
            .input("in", Signal::from_fn(|f| f as f32 / 1024.0))
            .check_block_sizes();
        assert_eq!(single.output("out")[777], 777.0 / 1024.0);
    }
    for (mode, expected) in [(0.0, 36.0), (1.0, 4.5)] {
        let render = Harness::new(&MERGE)
            .frames(1024)
            .param("mode", mode)
            .bundle_input("in", (1..=8).map(|v| Signal::constant(v as f32)).collect())
            .bundle_input("gate", (0..8).map(|_| Signal::constant(0.0)).collect())
            .check_block_sizes();
        assert!(render.output("out").iter().all(|v| *v == expected));
    }
}

/// Selection follows gate edges and holds when all voices stop, including ties.
#[test]
fn selection_holds_between_edges_and_after_release() {
    for mode in [2.0, 3.0, 4.0] {
        let rendered = Harness::new(&MERGE)
            .frames(1024)
            .param("mode", mode)
            .bundle_input(
                "in",
                vec![
                    Signal::from_fn(|f| if f < 200 { 1.0 } else { 5.0 }),
                    Signal::constant(2.0),
                ],
            )
            .bundle_input(
                "gate",
                vec![
                    Signal::from_fn(|f| if (100..400).contains(&f) { 1.0 } else { 0.0 }),
                    Signal::from_fn(|f| if (300..500).contains(&f) { 1.0 } else { 0.0 }),
                ],
            )
            .check_block_sizes();
        let out = rendered.output("out");
        assert_eq!(out[0], 1.0);
        assert_eq!(out[100], 1.0);
        assert_eq!(out[250], 5.0);
        assert_eq!(out[300], if mode == 4.0 { 5.0 } else { 2.0 });
        assert_eq!(out[450], 2.0);
        assert_eq!(out[900], 2.0);
    }
    let ties = Harness::new(&MERGE)
        .frames(1024)
        .param("mode", 2.0)
        .bundle_input("in", vec![Signal::constant(2.0), Signal::constant(3.0)])
        .check_block_sizes();
    assert_eq!(ties.output("out")[0], 2.0);
}

/// Missing gate voices read 1, while a mono gate fans out to every voice.
#[test]
fn narrower_gate_bundle_uses_the_declared_normal() {
    let h = Harness::new(&MERGE).frames(1024).param("mode", 4.0).bundle_input(
        "in",
        vec![Signal::constant(1.0), Signal::constant(2.0), Signal::constant(3.0)],
    );
    let padded = h
        .bundle_input("gate", vec![Signal::constant(0.0), Signal::constant(0.0)])
        .check_block_sizes();
    assert!(padded.output("out").iter().all(|v| *v == 3.0));
    let mono = Harness::new(&MERGE)
        .frames(1024)
        .param("mode", 4.0)
        .bundle_input("in", vec![Signal::constant(1.0), Signal::constant(3.0)])
        .input("gate", Signal::from_fn(|f| if f >= 100 { 1.0 } else { 0.0 }))
        .check_block_sizes();
    assert_eq!(mono.output("out")[99], 1.0);
    assert_eq!(mono.output("out")[100], 3.0);
}

/// Selection compares stereo means and carries both sides of the selected voice.
#[test]
fn stereo_selection_and_gate_width() {
    let rendered = Harness::new(&MERGE)
        .frames(1024)
        .param("mode", 4.0)
        .stereo_bundle_input(
            "in",
            vec![
                [Signal::constant(8.0), Signal::constant(-6.0)],
                [Signal::constant(2.0), Signal::constant(4.0)],
            ],
        )
        .check_block_sizes();
    assert_eq!(rendered.voice_output("out", 0, 0)[0], 2.0);
    assert_eq!(rendered.voice_output("out", 0, 1)[0], 4.0);
    let mono = Harness::new(&MERGE)
        .frames(1024)
        .bundle_input("in", vec![Signal::constant(1.0), Signal::constant(2.0)])
        .stereo_bundle_input("gate", vec![[Signal::constant(1.0), Signal::constant(1.0)]])
        .check_block_sizes();
    // A mono `in` gives a mono `out` and a mono `gate`, whatever the gate's width.
    assert_eq!(mono.outputs().len(), 2);
}

/// A right-side gate is still the gate of that voice, using stereo gate maximum.
#[test]
fn either_stereo_gate_side_can_select_a_voice() {
    let render = Harness::new(&MERGE)
        .frames(1024)
        .param("mode", 4.0)
        .bundle_input("in", vec![Signal::constant(1.0), Signal::constant(2.0)])
        .stereo_bundle_input(
            "gate",
            vec![
                [Signal::constant(0.0), Signal::constant(0.0)],
                [Signal::constant(0.0), Signal::constant(1.0)],
            ],
        )
        .check_block_sizes();
    assert_eq!(render.output("out")[0], 2.0);
}

/// Switching into a selection mode chooses from the currently active chord.
#[test]
fn selection_modes_reselect_immediately_when_the_effective_mode_changes() {
    let render = Harness::new(&MERGE)
        .frames(1024)
        .bundle_input(
            "in",
            vec![
                Signal::constant(0.0),
                Signal::constant(3.0),
                Signal::constant(1.0),
                Signal::constant(2.0),
            ],
        )
        .bundle_input(
            "gate",
            vec![
                Signal::constant(0.0),
                Signal::from_fn(|f| if f >= 100 { 1.0 } else { 0.0 }),
                Signal::from_fn(|f| if f >= 150 { 1.0 } else { 0.0 }),
                Signal::from_fn(|f| if f >= 200 { 1.0 } else { 0.0 }),
            ],
        )
        .param_at(300.0 / 48000.0, "mode", 2.0)
        .param_at(400.0 / 48000.0, "mode", 3.0)
        .param_at(500.0 / 48000.0, "mode", 4.0)
        .check_block_sizes();
    assert_eq!(render.output("out")[299], 6.0);
    assert_eq!(render.output("out")[300], 2.0);
    assert_eq!(render.output("out")[400], 1.0);
    assert_eq!(render.output("out")[500], 3.0);
}

/// A four-voice bundle with every gate high and `first` on voice 1, the
/// others 2, 3, and 4 (or 4, 3, 2 descending), in the given mode.
fn ranked(mode: f32, first: f32) -> Harness {
    Harness::new(&MERGE).frames(1024).param("mode", mode).bundle_input(
        "in",
        vec![
            Signal::constant(first),
            Signal::constant(3.0),
            Signal::constant(2.0),
            Signal::constant(4.0),
        ],
    )
}

/// A non-finite voice never wins Lowest or Highest over a finite one (#794),
/// wherever it sits in the bundle.
#[test]
fn a_non_finite_voice_never_wins_lowest_or_highest() {
    for (mode, first, expected) in [
        (3.0, f32::NAN, 2.0),
        (4.0, f32::NAN, 4.0),
        (3.0, f32::NEG_INFINITY, 2.0),
        (4.0, f32::INFINITY, 4.0),
        // The other sign already loses, and must still lose.
        (3.0, f32::INFINITY, 2.0),
        (4.0, f32::NEG_INFINITY, 4.0),
    ] {
        let out = ranked(mode, first).check_block_sizes();
        assert!(
            out.output("out").iter().all(|v| *v == expected),
            "mode {mode}, voice 1 {first}"
        );
    }
}

/// With every playing voice NaN, the tie rule picks voice 1 and its NaN passes
/// through unchanged, at every block size.
#[test]
fn all_non_finite_voices_keep_the_lowest_index() {
    for mode in [3.0, 4.0] {
        let h = Harness::new(&MERGE)
            .frames(1024)
            .param("mode", mode)
            .bundle_input("in", (0..4).map(|_| Signal::constant(f32::NAN)).collect());
        let renders: Vec<_> = loom_test::BLOCK_SIZES.iter().map(|&b| h.render_with_block(b)).collect();
        for render in &renders {
            assert!(render.output("out").iter().all(|v| v.is_nan()));
            assert_eq!(
                render.output("out").iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                renders[0].output("out").iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            );
        }
    }
}

/// Two voices: voice 1 plays frames 100 to 400 at the lower value, voice 2
/// frames 300 to 600 at the higher value.
fn handed_off(mode: f32) -> loom_test::Render {
    Harness::new(&MERGE)
        .frames(1024)
        .param("mode", mode)
        .bundle_input("in", vec![Signal::constant(1.0), Signal::constant(2.0)])
        .bundle_input(
            "gate",
            vec![
                Signal::from_fn(|f| if (100..400).contains(&f) { 1.0 } else { 0.0 }),
                Signal::from_fn(|f| if (300..600).contains(&f) { 1.0 } else { 0.0 }),
            ],
        )
        .check_block_sizes()
}

/// The `gate` output follows the chosen voice in the selection modes and any
/// voice in Sum and Average, and a hand-off between held voices has no low
/// sample (#793).
#[test]
fn the_gate_output_follows_the_voice_out_follows() {
    for mode in [0.0, 1.0, 2.0, 3.0, 4.0] {
        let render = handed_off(mode);
        let gate = render.output("gate");
        // Low before any gate and after every gate falls.
        assert_eq!(gate[50], 0.0, "mode {mode}");
        assert_eq!(gate[700], 0.0, "mode {mode}");
        // High on every frame from the first rise to the last fall: Lowest
        // hands off from voice 1 to voice 2 on frame 400 with no low sample.
        assert!(gate[100..600].iter().all(|v| *v == 1.0), "mode {mode}");
    }
    // Lowest follows voice 1 and then hands off to voice 2.
    let lowest = handed_off(3.0);
    assert_eq!(lowest.output("out")[350], 1.0);
    assert_eq!(lowest.output("out")[400], 2.0);
}

/// Every gate low gives a low output in every mode, and an unpatched `gate`
/// input gives 1.
#[test]
fn the_gate_output_is_low_when_all_are_low_and_one_when_unpatched() {
    for mode in [0.0, 1.0, 2.0, 3.0, 4.0] {
        let low = Harness::new(&MERGE)
            .frames(512)
            .param("mode", mode)
            .bundle_input("in", vec![Signal::constant(1.0), Signal::constant(2.0)])
            .bundle_input("gate", vec![Signal::constant(0.0), Signal::constant(0.0)])
            .check_block_sizes();
        assert!(low.output("gate").iter().all(|v| *v == 0.0), "mode {mode}");
        let unpatched = Harness::new(&MERGE)
            .frames(512)
            .param("mode", mode)
            .bundle_input("in", vec![Signal::constant(1.0), Signal::constant(2.0)])
            .check_block_sizes();
        assert!(unpatched.output("gate").iter().all(|v| *v == 1.0), "mode {mode}");
    }
}

/// A one-voice `in` passes its gate through, value and all.
#[test]
fn a_single_voice_passes_its_gate_through() {
    for mode in [0.0, 1.0, 2.0, 3.0, 4.0] {
        let render = Harness::new(&MERGE)
            .frames(512)
            .param("mode", mode)
            .input("in", Signal::constant(0.5))
            .input("gate", Signal::from_fn(|f| if f % 100 < 50 { 0.8 } else { 0.0 }))
            .check_block_sizes();
        let gate = render.output("gate");
        assert_eq!(gate[10], 0.8, "mode {mode}");
        assert_eq!(gate[60], 0.0, "mode {mode}");
    }
}

/// A stereo `in` makes `gate` stereo, with two equal sides.
#[test]
fn a_stereo_input_gives_equal_gate_sides() {
    let render = Harness::new(&MERGE)
        .frames(512)
        .param("mode", 3.0)
        .stereo_bundle_input(
            "in",
            vec![
                [Signal::constant(1.0), Signal::constant(1.5)],
                [Signal::constant(2.0), Signal::constant(2.5)],
            ],
        )
        .bundle_input(
            "gate",
            vec![
                Signal::from_fn(|f| if f < 200 { 1.0 } else { 0.0 }),
                Signal::constant(0.0),
            ],
        )
        .check_block_sizes();
    assert_eq!(render.voice_output("gate", 0, 0), render.voice_output("gate", 0, 1));
    assert_eq!(render.voice_output("gate", 0, 0)[100], 1.0);
    assert_eq!(render.voice_output("gate", 0, 0)[300], 0.0);
}
