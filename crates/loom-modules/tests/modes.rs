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

/// [`constant_member`] through a multiplier, so its output is audio.
fn audio_member(key: &str, value: f32) -> String {
    constant_member(key, value)
        .replace(
            r#"PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),"#,
            r#"PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")), PatchModule(id: 3, kind: "loom.math.mul@1"),"#,
        )
        .replace(
            r#"cables: [PatchCable(from: (1, "out"), to: (2, "in"))],"#,
            r#"cables: [PatchCable(from: (1, "out"), to: (3, "a")), PatchCable(from: (3, "out"), to: (2, "in"))],"#,
        )
}

/// A select of 0.5 and -0.25, at Member 1: audio (through a multiplier),
/// or the constants' own modulation outputs.
fn select(audio: bool) -> String {
    let member = if audio { audio_member } else { constant_member };
    let members = format!("{}{}", member("a", 0.5), member("b", -0.25));
    patch(&two_members("Select"), &members, r#", params: {"member": 1.0}"#)
}

/// Renders the select with Member set to 2 before frame `at`.
fn select_change(at: usize, frames: usize, block: usize) -> Vec<f32> {
    select_change_of(true, at, frames, block)
}

/// [`select_change`] for audio members or modulation members.
fn select_change_of(audio: bool, at: usize, frames: usize, block: usize) -> Vec<f32> {
    let (mut parts, ids) = load(&select(audio));
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

/// A select of modulation outputs fades linearly, so the gains sum to 1 and
/// a value both members share would stay put; equal power would swell it
/// by up to √2 in the middle.
#[test]
fn a_select_of_modulation_fades_linearly() {
    let at = 77 * 512;
    let left = select_change_of(false, at, at + 2 * FADE, 512);
    for n in 0..FADE {
        let (from, to) = blend::linear(n as f32 / FADE as f32);
        let want = 0.5 * from - 0.25 * to;
        assert!(
            (left[at + n] - want).abs() < 1e-6,
            "frame {n}: {} vs {want}",
            left[at + n]
        );
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

/// [`stack`] with the constant through a multiplier, so the output is
/// audio, which a stack sums at 1/√N and stereo spread pans.
fn audio_stack(fields: &str) -> String {
    stack(fields)
        .replace(
            r#"PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),"#,
            r#"PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")), PatchModule(id: 3, kind: "loom.math.mul@1"),"#,
        )
        .replace(
            r#"cables: [PatchCable(from: (1, "out"), to: (2, "in"))],"#,
            r#"cables: [PatchCable(from: (1, "out"), to: (3, "a")), PatchCable(from: (3, "out"), to: (2, "in"))],"#,
        )
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
/// control's range. A Constant's output is modulation, so the stack's output
/// is the copies' mean.
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
    let want = want / 4.0;
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
        &audio_stack(r#"copies: 4, spread: {"value": 0.3}, stereo_spread: true,"#),
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
    // A Constant's output is modulation, so the stack plays the copies'
    // mean; copy 0 alone would give (value + 0.2 + 0.2)/3.
    let want = value;
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

/// The patch `text` loaded as the editor loads it, with its hierarchy, so
/// a test can change modes as the editor does.
fn load_editor(text: &str) -> (EngineParts, loom_patch::Hierarchy) {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let patch = Patch::from_ron(text).expect("the patch parses");
    let mut ids = FlatIds::new();
    patch
        .load_with_ids(&mut parts.controller, &mut ids)
        .expect("the patch loads");
    let hierarchy =
        loom_patch::Hierarchy::from_patch(&patch, parts.controller.graph(), ids, parts.controller.registry())
            .expect("reads");
    (parts, hierarchy)
}

/// Renders `frames` frames in buffers of 64, making `edit` and committing
/// just before frame `at`. Returns the output and the edit's cable fades.
fn render_mode_edit(
    text: &str,
    frames: usize,
    at: usize,
    edit: impl FnOnce(&mut loom_patch::Hierarchy, &mut loom_core::Controller),
) -> (Vec<f32>, usize) {
    let (left, _, fades) = render_mode_edits(text, frames, at, |_, _| {}, edit);
    (left, fades)
}

/// [`render_mode_edit`] with a `setup` edit made and committed before the
/// first frame, and both sides returned.
fn render_mode_edits(
    text: &str,
    frames: usize,
    at: usize,
    setup: impl FnOnce(&mut loom_patch::Hierarchy, &mut loom_core::Controller),
    edit: impl FnOnce(&mut loom_patch::Hierarchy, &mut loom_core::Controller),
) -> (Vec<f32>, Vec<f32>, usize) {
    let (mut parts, mut hierarchy) = load_editor(text);
    setup(&mut hierarchy, &mut parts.controller);
    hierarchy.sync(&mut parts.controller).expect("syncs");
    parts.controller.commit().expect("commits");
    let mut edit = Some(edit);
    let mut fades = 0;
    let (left, right) = render_with(&mut parts, frames, 64, &[], |position, parts| {
        if position == at
            && let Some(edit) = edit.take()
        {
            edit(&mut hierarchy, &mut parts.controller);
            hierarchy.sync(&mut parts.controller).expect("syncs");
            fades = parts.controller.commit().expect("commits").cable_fades;
        }
    });
    (left, right, fades)
}

/// The largest change from one sample to the next.
fn largest_step(signal: &[f32]) -> f32 {
    signal.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max)
}

/// Single to Layer, and Single to Select, only rearrange: the cable to the
/// output moves onto a mixer that plays its old source alone, so it
/// switches, and the sound does not change by a single bit (live patching
/// spec, rule 9, The mixers the editor adds).
#[test]
fn single_to_layer_or_select_switches_with_no_fade() {
    let text = patch(&alone("a", 0.0, 0.5), "", "");
    let (plain, _) = render_mode_edit(&text, 2_048, 1_024, |_, _| {});
    for mode in [loom_patch::Mode::Layer, loom_patch::Mode::Select] {
        let (edited, fades) = render_mode_edit(&text, 2_048, 1_024, |hierarchy, controller| {
            hierarchy
                .set_mode(controller.graph(), ModuleId(7), mode, &|name| name.to_string())
                .expect("the mode changes");
        });
        assert_eq!(fades, 0, "{mode:?}");
        assert_eq!(edited, plain, "{mode:?} does not change the sound");
    }
}

/// A stack going from 1 copy to 2 gets a new Sum, which starts at its final
/// gain, so the output fades once to the whole stack instead of switching:
/// no step, and a stack of equal values never reads below the copy value
/// (rule 9, Every other change of mixer fades once).
#[test]
fn a_stack_going_from_one_copy_to_two_fades_once() {
    let at = 1_024;
    let frames = 2_048;
    let to_two = |hierarchy: &mut loom_patch::Hierarchy, _: &mut loom_core::Controller| {
        hierarchy.set_copies(ModuleId(7), 2).expect("copies change");
    };
    // Modulation: the copies' mean, 0.2 before and after.
    let (values, fades) = render_mode_edit(&patch(&stack(""), "", ""), frames, at, to_two);
    assert!(fades > 0, "the output's cable moved onto a new Sum");
    assert!(
        values[at..].iter().all(|&v| (v - 0.2).abs() < 1e-6),
        "the mean never dips"
    );
    // Audio: 0.2 alone, then two copies at 1/√2, reached in a straight line.
    let (audio, _) = render_mode_edit(&patch(&audio_stack(""), "", ""), frames, at, to_two);
    let before = audio[at - 1];
    let after = audio[frames - 1];
    assert!(after > before, "two copies are louder");
    let step = largest_step(&audio[at - 1..]);
    assert!(step <= (after - before) / 240.0 + 1e-6, "no step: {step}");
}

/// Layer of one member back to Single, and Select back to Single: the
/// mixer that leaves played its member alone, so the output switches and
/// the sound does not change by a single bit (rule 9, rule A).
#[test]
fn layer_or_select_of_one_back_to_single_switches_with_no_fade() {
    let text = patch(&alone("a", 0.0, 0.5), "", "");
    let (plain, _) = render_mode_edit(&text, 2_048, 1_024, |_, _| {});
    for mode in [loom_patch::Mode::Layer, loom_patch::Mode::Select] {
        let (edited, _, fades) = render_mode_edits(
            &text,
            2_048,
            1_024,
            |hierarchy, controller| {
                hierarchy
                    .set_mode(controller.graph(), ModuleId(7), mode, &|name| name.to_string())
                    .expect("to the mode");
            },
            |hierarchy, controller| {
                hierarchy
                    .set_mode(controller.graph(), ModuleId(7), loom_patch::Mode::Single, &|name| {
                        name.to_string()
                    })
                    .expect("back to single");
            },
        );
        assert_eq!(fades, 0, "{mode:?} back to single");
        assert_eq!(edited, plain, "{mode:?} back to single does not change the sound");
    }
}

/// A stack going from 2 copies to 1 loses its Sum, which played both, so
/// the output fades once to copy 0. Going from 2 to 3 plugs the new copy
/// into the Sum, which fades it in. Neither steps.
#[test]
fn a_stack_of_two_losing_or_gaining_a_copy_fades() {
    let at = 1_024;
    let frames = 2_048;
    let text = patch(&audio_stack("copies: 2,"), "", "");
    for copies in [1, 3] {
        let (audio, _, fades) = render_mode_edits(
            &text,
            frames,
            at,
            |_, _| {},
            |hierarchy, _| {
                hierarchy.set_copies(ModuleId(7), copies).expect("copies change");
            },
        );
        assert!(fades > 0, "{copies} copies");
        let before = audio[at - 1];
        let after = audio[frames - 1];
        assert_ne!(before, after, "{copies} copies changes the level");
        // Each copy plays 0.2. Going to 1, the output fades straight to
        // copy 0. Going to 3, copy 3 fades in over 5 ms at a gain of at
        // most 1/√2, while the Sum's gain glides to 1/√3 over 10 ms.
        let limit = if copies == 1 {
            (after - before).abs() / 240.0
        } else {
            0.2 * std::f32::consts::FRAC_1_SQRT_2 / 240.0
                + 0.4 * (std::f32::consts::FRAC_1_SQRT_2 - 1.0 / 3f32.sqrt()) / 480.0
        };
        let step = largest_step(&audio[at - 1..]);
        assert!(step <= limit + 1e-6, "{copies} copies: step {step} > {limit}");
    }
}

/// A stack with stereo spread going from 1 copy to 2. Its Stereo spread sum
/// is there already at one copy, so the new copy's cable into it fades in,
/// while copy 0 glides to its spread value and new place. Nothing steps on
/// either side: each side's largest step is within twice the slope of one
/// 5 ms move across that side's whole change, because two things move at
/// once. A click would be a jump of the whole change on one frame.
#[test]
fn a_spread_stack_going_from_one_copy_to_two_fades_both_sides() {
    let at = 1_024;
    let frames = 4_096;
    let text = patch(&audio_stack(r#"spread: {"value": 0.3}, stereo_spread: true,"#), "", "");
    let (left, right, fades) = render_mode_edits(
        &text,
        frames,
        at,
        |_, _| {},
        |hierarchy, _| {
            hierarchy.set_copies(ModuleId(7), 2).expect("copies change");
        },
    );
    assert!(fades > 0, "the new copy fades in");
    for (side, played) in [("left", &left), ("right", &right)] {
        assert!(played.iter().all(|v| v.is_finite()));
        let after = &played[at - 1..];
        let low = after.iter().copied().fold(f32::INFINITY, f32::min);
        let high = after.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let step = largest_step(after);
        assert!(
            step <= 2.0 * (high - low) / 240.0 + 1e-6,
            "{side}: step {step}, change {}",
            high - low
        );
    }
}
