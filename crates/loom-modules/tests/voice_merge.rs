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
    assert_eq!(mono.outputs().len(), 1);
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
