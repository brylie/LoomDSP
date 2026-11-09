//! Sub-patch modes in the engine (`docs/design/sub-patches.md`, Modes, and
//! Acceptance criteria, Roles, slots, and modes): a layer plays the sum of
//! its members, a select crossfades over 10 ms, equal power, a stack gives
//! each copy its spread value and sums at 1/√N, and stereo spread gives a
//! stereo output. Every render is the same at host buffers of 1, 77, and
//! 512 frames.

mod common;

use common::{SAMPLE_RATE, render, render_with};
use loom_core::{EngineConfig, EngineParts, ModuleId, ModuleNames, OutputSafety, create_engine};
use loom_dsp::blend;
use loom_patch::flatten::{ADDED_STEP, COPY_STEP, MEMBER_STEP};
use loom_patch::{FlatIds, ModulePath, Patch};

/// Frames a select fade takes at 48 kHz.
const FADE: usize = 480;

/// An engine with `text` loaded and output safety off, so the render holds
/// what the patch makes (ADR-0019), with the id table.
fn load(text: &str) -> (EngineParts, FlatIds) {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let mut ids = FlatIds::new();
    Patch::from_ron(text)
        .expect("the patch parses")
        .load_with_ids(&mut parts.controller, &mut ids)
        .expect("the patch loads");
    (parts, ids)
}

/// The flat id at `path`.
fn id(ids: &FlatIds, path: &[u32]) -> ModuleId {
    ids.get(&ModulePath(path.to_vec()))
        .unwrap_or_else(|| panic!("no module at {path:?}"))
}

/// A member definition `key` that outputs a sine `octave` octaves from
/// middle C, times `gain`.
fn sine_member(key: &str, name: &str, octave: f32, gain: f32) -> String {
    format!(
        r#""{key}": SubPatch(
            metadata: Metadata(name: "{name}"),
            modules: [
                PatchModule(id: 1, kind: "loom.phase.generator@1", params: {{"octave": {octave}}}),
                PatchModule(id: 2, kind: "loom.osc.sine@1"),
                PatchModule(id: 3, kind: "loom.math.mul@1", params: {{"gain": {gain}}}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [
                PatchCable(from: (1, "phase"), to: (2, "phase")),
                PatchCable(from: (2, "out"), to: (3, "a")),
                PatchCable(from: (3, "out"), to: (4, "in")),
            ],
        ),"#
    )
}

/// A patch whose instance 7 has the definition `outer` and the member
/// definitions `members`, played through the audio output.
fn patch(outer: &str, members: &str, instance: &str) -> String {
    format!(
        r#"Patch(
    version: 1,
    subpatches: {{
        {outer}
        {members}
    }},
    modules: [
        PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("outer"){instance}),
        PatchModule(id: 9, kind: "loom.io.audio-out@1"),
        PatchModule(id: 10, kind: "loom.clock.transport@1"),
    ],
    cables: [PatchCable(from: (7, "out"), to: (9, "in"))],
)"#
    )
}

/// A layer or select definition `outer` of members `a` and `b`.
fn two_members(mode: &str) -> String {
    format!(
        r#""outer": SubPatch(
            metadata: Metadata(name: "Layer"),
            mode: {mode},
            members: [1, 2],
            modules: [
                PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("a")),
                PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("b")),
            ],
        ),"#
    )
}

/// A single definition `outer` holding member `key`'s inside, for a render
/// of that member alone.
fn alone(key: &str, octave: f32, gain: f32) -> String {
    sine_member(key, "Alone", octave, gain).replacen(&format!("\"{key}\""), "\"outer\"", 1)
}

/// A layer plays the sum of its members: every frame is member a's plus
/// member b's, rendered alone, at every block size.
#[test]
fn a_layer_plays_the_sum_of_its_members() {
    let members = format!(
        "{}{}",
        sine_member("a", "Sine", 0.0, 0.3),
        sine_member("b", "Low", -1.0, 0.2)
    );
    let layer = patch(&two_members("Layer"), &members, "");
    let frames = 4_800;
    let (a, _) = render(&mut load(&patch(&alone("a", 0.0, 0.3), "", "")).0, frames, 64, &[]);
    let (b, _) = render(&mut load(&patch(&alone("b", -1.0, 0.2), "", "")).0, frames, 64, &[]);
    let sum: Vec<f32> = a.iter().zip(&b).map(|(a, b)| a + b).collect();
    for block in [1, 77, 512] {
        let (left, right) = render(&mut load(&layer).0, frames, block, &[]);
        assert!(left.iter().any(|&s| s != 0.0));
        // The output fades in over its first 10 ms even with output safety
        // off (ADR-0019), and the fade of a sum rounds apart from the sum of
        // two fades; from there on the frames are the same, bit for bit.
        assert_eq!(left[FADE..], sum[FADE..], "block {block}");
        assert!(left[..FADE].iter().zip(&sum).all(|(l, s)| (l - s).abs() < 1e-6));
        assert_eq!(left, right, "a mono cable plays on both sides");
    }
}

/// A member that holds a constant `value`.
fn constant_member(key: &str, value: f32) -> String {
    format!(
        r#""{key}": SubPatch(
            metadata: Metadata(name: "{key}"),
            modules: [
                PatchModule(id: 1, kind: "loom.math.constant@1", params: {{"value": {value}}}),
                PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
        ),"#
    )
}

/// A select of a constant 0.5 and a constant -0.25, at Member 1.
fn select() -> String {
    let members = format!("{}{}", constant_member("a", 0.5), constant_member("b", -0.25));
    patch(&two_members("Select"), &members, r#", params: {"member": 1.0}"#)
}

/// Renders the select with Member set to 2 before frame `at`.
fn select_change(at: usize, frames: usize, block: usize) -> Vec<f32> {
    let (mut parts, ids) = load(&select());
    let member = id(&ids, &[7, ADDED_STEP + MEMBER_STEP]);
    let (left, _) = render_with(&mut parts, frames, block, &[], |position, parts| {
        if position == at {
            parts.controller.set_param(member, 0, 2.0).unwrap();
        }
    });
    left
}

/// A select change crossfades over 10 ms, equal power: the frame the change
/// lands on is member 1 alone, then the gains follow a quarter circle, and
/// after 480 frames it is member 2 exactly. The render is the same at block
/// sizes 1, 77, and 512.
#[test]
fn a_select_change_crossfades_over_10_ms() {
    // A frame where buffers of 1, 77, and 512 all start.
    let at = 77 * 512;
    let frames = at + 2 * FADE;
    let left = select_change(at, frames, 1);
    // Past the output's own 10 ms fade-in (ADR-0019).
    assert!(left[FADE..at].iter().all(|&s| s == 0.5), "member 1 before the change");
    for n in 0..FADE {
        let (from, to) = blend::equal_power(n as f32 / FADE as f32);
        let want = 0.5 * from - 0.25 * to;
        assert!(
            (left[at + n] - want).abs() < 1e-6,
            "frame {n}: {} vs {want}",
            left[at + n]
        );
    }
    assert!(left[at + FADE..].iter().all(|&s| s == -0.25), "member 2 after the fade");
    for block in [77, 512] {
        assert_eq!(select_change(at, frames, block), left, "block {block}");
    }
}

/// A stack definition `outer` of a constant at 0.2 exposed as `value`, with
/// `fields` added.
fn stack(fields: &str) -> String {
    format!(
        r#""outer": SubPatch(
            metadata: Metadata(name: "Unison"),
            mode: Stack,
            {fields}
            modules: [
                PatchModule(id: 1, kind: "loom.math.constant@1", params: {{"value": 0.2}}),
                PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
            exposed: [Exposed(name: "value", module: 1, param: "value")],
        ),"#
    )
}

/// The index of the `value` parameter of the constant `id`.
fn node_index(parts: &EngineParts, id: ModuleId) -> usize {
    parts
        .controller
        .graph()
        .node(id)
        .unwrap()
        .parameter_index("value")
        .unwrap()
}

/// The flat id of copy `k`'s constant in [`stack`].
fn copy_constant(ids: &FlatIds, k: u32) -> ModuleId {
    if k == 0 {
        id(ids, &[7, 1])
    } else {
        id(ids, &[7, COPY_STEP + k, 1])
    }
}

/// Stack mode with 4 copies and a spread of 0.3 on `value` gives each copy
/// its offset value, the base moved by 0.3 times -1, -1/3, 1/3, and 1 in the
/// control's range, and the output is their sum at 1/√4.
#[test]
fn a_stack_of_four_gives_each_copy_its_spread_value() {
    let text = patch(&stack(r#"copies: 4, spread: {"value": 0.3},"#), "", "");
    let (mut parts, ids) = load(&text);
    let frames = 2_400;
    let (left, _) = render(&mut parts, frames, 64, &[]);
    let node = parts.controller.graph().node(copy_constant(&ids, 0)).unwrap();
    let index = node.parameter_index("value").unwrap();
    let spec = node.spec(index).unwrap();
    let base = spec.normalize(0.2);
    let mut want = 0.0;
    for k in 0..4 {
        let offset = blend::spread_offset(k, 4);
        let position = (base + 0.3 * offset).clamp(0.0, 1.0);
        let live = parts.controller.modulated(copy_constant(&ids, k), index);
        assert!(
            live.is_some_and(|live| (live - position).abs() < 1e-5),
            "copy {k}: {live:?} vs {position}"
        );
        assert_eq!(
            parts.controller.graph().node(copy_constant(&ids, k)).unwrap().params[index],
            0.2
        );
        want += spec.denormalize(position);
    }
    let want = 0.5 * want;
    assert!((left[frames - 1] - want).abs() < 1e-4, "{} vs {want}", left[frames - 1]);
    for block in [1, 77, 512] {
        assert_eq!(render(&mut load(&text).0, frames, block, &[]).0, left, "block {block}");
    }
}

/// A stack of one copy with no spread plays as the single sub-patch, bit
/// for bit: a mode change alone never changes the sound.
#[test]
fn a_stack_of_one_plays_as_the_single_sub_patch() {
    let single = patch(&stack("").replace("mode: Stack,", ""), "", "");
    let one = patch(&stack(""), "", "");
    assert_eq!(
        render(&mut load(&single).0, 2_400, 64, &[]),
        render(&mut load(&one).0, 2_400, 64, &[])
    );
}

/// Stack mode with stereo spread gives a stereo output: copy k is panned by
/// its offset, equal power, at 1/√N, so four copies of 0.2 give the same
/// sum on each side and a different one with a spread on the value.
#[test]
fn stereo_spread_gives_a_stereo_output() {
    let text = patch(
        &stack(r#"copies: 4, spread: {"value": 0.3}, stereo_spread: true,"#),
        "",
        "",
    );
    let (mut parts, ids) = load(&text);
    let frames = 2_400;
    let (left, right) = render(&mut parts, frames, 64, &[]);
    let node = parts.controller.graph().node(copy_constant(&ids, 0)).unwrap();
    let spec = node.spec(node.parameter_index("value").unwrap()).unwrap();
    let (mut want_left, mut want_right) = (0.0, 0.0);
    for k in 0..4 {
        let offset = blend::spread_offset(k, 4);
        let value = spec.denormalize((spec.normalize(0.2) + 0.3 * offset).clamp(0.0, 1.0));
        let (l, r) = blend::pan(offset);
        want_left += 0.5 * l * value;
        want_right += 0.5 * r * value;
    }
    let last = frames - 1;
    assert!((left[last] - want_left).abs() < 1e-4, "{} vs {want_left}", left[last]);
    assert!(
        (right[last] - want_right).abs() < 1e-4,
        "{} vs {want_right}",
        right[last]
    );
    assert!((left[last] - right[last]).abs() > 1e-3, "the sides differ");
    for block in [1, 77, 512] {
        assert_eq!(
            render(&mut load(&text).0, frames, block, &[]),
            (left.clone(), right.clone()),
            "block {block}"
        );
    }
}

/// A MIDI mapping on a stack's exposed parameter moves every copy, though a
/// control has one mapping: the other copies follow it in the engine.
#[test]
fn a_mapping_on_a_stack_moves_every_copy() {
    let text = patch(&stack("copies: 3,"), "", "").replace(
        "    cables: [PatchCable(from: (7, \"out\"), to: (9, \"in\"))],",
        "    cables: [PatchCable(from: (7, \"out\"), to: (9, \"in\"))],\n    midi: [PatchMidiMapping(target: (7, \"value\"), channel: 0, controller: 20)],",
    );
    let (mut parts, ids) = load(&text);
    render(&mut parts, 512, 512, &[]);
    assert!(parts.midi.send(loom_core::MidiEvent::control_change(0, 0, 20, 80)));
    let (left, _) = render(&mut parts, 4_800, 512, &[]);
    let node = parts.controller.graph().node(copy_constant(&ids, 0)).unwrap();
    let value = node
        .spec(node.parameter_index("value").unwrap())
        .unwrap()
        .denormalize(80.0 / 127.0);
    // Copy 0 alone would give (value + 0.2 + 0.2)/√3.
    let want = blend::unison_gain(3) * 3.0 * value;
    assert!((left[4_799] - want).abs() < 1e-4, "{} vs {want}", left[4_799]);
    // The graph holds the value for every copy too, so a commit before the
    // copies sync again compiles what plays.
    parts.controller.sync_midi();
    let index = node_index(&parts, copy_constant(&ids, 0));
    for k in 0..3 {
        let copy = parts.controller.graph().node(copy_constant(&ids, k)).unwrap();
        assert!((copy.params[index] - value).abs() < 1e-6, "copy {k}");
    }
    // An unrelated module makes the commit compile the schedule again from
    // the graph's values; a stale copy would play its old value again.
    let unrelated = parts.controller.add_module("loom.math.constant").unwrap();
    let report = parts.controller.commit().unwrap();
    assert!(
        report.skipped.contains(&unrelated),
        "the commit compiled the graph again"
    );
    let (left, _) = render(&mut parts, 512, 512, &[]);
    assert!(
        (left[511] - want).abs() < 1e-4,
        "{} vs {want} after a commit",
        left[511]
    );
}
