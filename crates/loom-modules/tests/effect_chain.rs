//! The built-in Effect chain (MT-STRUCT-04; sub-patches.md, Built-in
//! abstractions): audio passes through every slot in order, each slot's mix
//! works on the Linear law, an emptied slot passes its input through, and
//! at its defaults the chain keeps its input's level. The chain is the one
//! in `examples/patches/effect-chain.ron`, which a test in `loom-app` keeps
//! equal to the built-in file.

mod common;

use common::{Note, SAMPLE_RATE, patch_path, render};
use loom_core::{EngineConfig, OutputSafety, create_engine};
use loom_patch::Patch;

const FRAMES: usize = 24_000;
const NOTE: Note = Note {
    note: 60,
    on: 240,
    off: 12_000,
};

/// The example patch, with the chain's three mixes set.
fn chain(mixes: [f32; 3], edit: impl FnOnce(&mut Patch)) -> Patch {
    let mut patch = common::load_patch(&patch_path("effect-chain.ron"));
    let instance = patch.modules.iter_mut().find(|m| m.id == 5).expect("the chain tile");
    for (n, mix) in mixes.iter().enumerate() {
        instance.params.insert(format!("effect {} mix", n + 1), *mix);
    }
    edit(&mut patch);
    patch
}

/// The plucked saw of the example, through `effects` (kinds with their
/// values) in series, with no chain: what the chain should equal.
fn flat(effects: &[(&str, &str)]) -> Patch {
    let mut modules = String::from(
        r#"PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.osc.saw@1"),
        PatchModule(id: 3, kind: "loom.env.envelope@1", params: {"attack": 0.002, "decay": 0.15, "sustain": 0.0, "release": 0.1}),
        PatchModule(id: 4, kind: "loom.level.amp@1", params: {"gain": 0.5}),
        PatchModule(id: 6, kind: "loom.io.audio-out@1"),
        PatchModule(id: 7, kind: "loom.clock.transport@1"),"#,
    );
    let mut cables = String::from(
        r#"PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (1, "gate"), to: (3, "gate")),
        PatchCable(from: (2, "out"), to: (4, "in")),
        PatchCable(from: (3, "out"), to: (4, "level")),"#,
    );
    let mut from = (4, "out".to_string());
    for (n, (kind, params)) in effects.iter().enumerate() {
        let id = 10 + n;
        modules.push_str(&format!(
            r#"PatchModule(id: {id}, kind: "{kind}", params: {{{params}}}),"#
        ));
        cables.push_str(&format!(
            r#"PatchCable(from: ({}, "{}"), to: ({id}, "in")),"#,
            from.0, from.1
        ));
        from = (id, "out".to_string());
    }
    cables.push_str(&format!(
        r#"PatchCable(from: ({}, "{}"), to: (6, "in")),"#,
        from.0, from.1
    ));
    Patch::from_ron(&format!(
        "Patch(version: 1, metadata: Metadata(name: \"Flat\"), modules: [{modules}], cables: [{cables}])"
    ))
    .unwrap()
}

const EQ: (&str, &str) = ("loom.eq.eq@1", r#""high_gain": -3.0"#);
const DYNAMICS: (&str, &str) = (
    "loom.dyn.dynamics@1",
    r#""threshold": -12.0, "ratio": 2.0, "output": 3.0"#,
);
const DELAY: (&str, &str) = ("loom.time.delay@1", r#""time": 0.3"#);

/// The left channel of `patch` played with [`NOTE`] in buffers of `block`,
/// output safety off.
fn play(patch: &Patch, block: usize) -> Vec<f32> {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    patch.load_into(&mut parts.controller).expect("the patch loads");
    render(&mut parts, FRAMES, block, &[NOTE]).0
}

/// The RMS level of `samples`.
fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
}

/// With every mix at 0 the chain is its input, bit for bit; with one mix
/// at 1 it is that one effect; with all at 1 it is the three effects in
/// order, and the reverse order sounds different. The same at block sizes
/// 1, 77, and 512.
#[test]
fn the_chain_passes_audio_through_every_slot_in_order() {
    for block in [1, 77, 512] {
        let dry = play(&flat(&[]), block);
        assert!(dry.iter().any(|v| v.abs() > 0.1), "the pluck plays");
        assert_eq!(play(&chain([0.0; 3], |_| {}), block), dry, "block {block}");
        assert_eq!(play(&chain([1.0, 0.0, 0.0], |_| {}), block), play(&flat(&[EQ]), block));
        assert_eq!(
            play(&chain([0.0, 0.0, 1.0], |_| {}), block),
            play(&flat(&[DELAY]), block)
        );
        let all = play(&chain([1.0; 3], |_| {}), block);
        assert_eq!(all, play(&flat(&[EQ, DYNAMICS, DELAY]), block), "block {block}");
        assert_ne!(all, play(&flat(&[DELAY, DYNAMICS, EQ]), block), "the order matters");
    }
}

/// A mix between 0 and 1 is a straight blend: at 0.3 the chain gives
/// 0.7 × dry + 0.3 × wet, the Linear law (sub-patches.md, The Effect chain).
#[test]
fn a_middle_mix_is_a_linear_blend() {
    let dry = play(&chain([1.0, 1.0, 0.0], |_| {}), 512);
    let wet = play(&chain([1.0, 1.0, 1.0], |_| {}), 512);
    let mixed = play(&chain([1.0, 1.0, 0.3], |_| {}), 512);
    for i in 0..FRAMES {
        let want = 0.7 * dry[i] + 0.3 * wet[i];
        assert!((mixed[i] - want).abs() < 1e-5, "frame {i}: {} against {want}", mixed[i]);
    }
}

/// With Effect 2 emptied, the chain still plays: the empty slot passes its
/// input through (ADR-0043), so the chain is Effect 1 into Effect 3.
#[test]
fn an_emptied_slot_passes_its_input_through() {
    let emptied = chain([1.0; 3], |patch| {
        let definition = patch.subpatches.get_mut("effect chain").unwrap();
        definition.modules.retain(|m| m.id != 3);
        let slot = definition.modules.iter_mut().find(|m| m.id == 11).unwrap();
        slot.slot.as_mut().unwrap().part = None;
    });
    assert_eq!(play(&emptied, 512), play(&flat(&[EQ, DELAY]), 512));
}

/// At its defaults the chain keeps its input's level within 2 dB, so
/// inserting it does not sound like a level drop (sub-patches.md, The
/// Effect chain).
#[test]
fn the_chain_at_its_defaults_keeps_its_input_level() {
    let patch = common::load_patch(&patch_path("effect-chain.ron"));
    let through = play(&patch, 512);
    let dry = play(&flat(&[]), 512);
    let window = 240..NOTE.off;
    let db = 20.0 * (rms(&through[window.clone()]) / rms(&dry[window])).log10();
    assert!(db.abs() <= 2.0, "the chain changes the level by {db:.2} dB");
}
