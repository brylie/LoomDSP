//! Slot swaps in the engine (`docs/design/sub-patches.md`, Slots and "How
//! a swap crossfades"; FR-ENCAP-08, FR-GRAPH-05). A swap is ordinary graph
//! edits: the new part and one swap fade per role output join the running
//! graph at a buffer boundary, both parts run for 10 ms, and the leaving
//! part and the fade go once the fade is done. The engine learns nothing
//! new, so what the render shows is the parts' own output through the
//! fade's equal-power law.

mod common;

use common::SAMPLE_RATE;
use loom_core::{EngineConfig, EngineParts, ModuleId, OutputSafety, create_engine};
use loom_patch::{FlatIds, Hierarchy, Layout, Patch};

/// Noise into a Filter slot holding an SVF, into the output, at the top
/// level. The slot's cutoff is low, so the SVF and the Ladder sound
/// clearly different.
const NOISE_SLOT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Noise slot"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 2, kind: "loom.filter.svf@1", params: {"cutoff": 300.0, "resonance": 0.6}),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 4, kind: "loom.clock.transport@1"),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(2)))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (3, "in")),
    ],
)
"#;

/// The frame the swap lands on, and the fade's length at the test rate.
const SWAP_AT: usize = 2_000;
const FADE: usize = 480;
const FRAMES: usize = 6_000;

/// An engine with `text` loaded, output safety off, and its hierarchy.
fn load(text: &str) -> (EngineParts, Hierarchy) {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let patch = Patch::from_ron(text).unwrap();
    let mut ids = FlatIds::new();
    patch
        .load_with_ids(&mut parts.controller, &mut ids)
        .expect("the patch loads");
    let hierarchy =
        Hierarchy::from_patch(&patch, parts.controller.graph(), ids, parts.controller.registry()).expect("reads");
    (parts, hierarchy)
}

/// Renders `frames` frames in buffers of `block`, with a shorter buffer
/// before `at`, so the edit `edit` lands on frame `at` at every block
/// size, and `between` runs before each buffer after it.
fn render_edit_at(
    parts: &mut EngineParts,
    frames: usize,
    block: usize,
    at: usize,
    edit: impl FnOnce(&mut EngineParts),
    mut between: impl FnMut(usize, &mut EngineParts),
) -> Vec<f32> {
    let mut left = vec![0.0; frames];
    let mut right = vec![0.0; frames];
    let mut position = 0;
    let mut edit = Some(edit);
    while position < frames {
        if position == at
            && let Some(edit) = edit.take()
        {
            edit(parts);
        }
        if position > at {
            between(position, parts);
        }
        let end = if position < at {
            at.min(position + block)
        } else {
            frames.min(position + block)
        };
        parts
            .engine
            .process(&mut left[position..end], &mut right[position..end]);
        position = end;
    }
    left
}

/// Renders the patch at `block` frames per buffer, swapping the slot to
/// `kind` at `SWAP_AT` and finishing the fade when every fade reports
/// done, if `finish`. Returns the left channel and the frame the finish
/// landed on.
fn render_swap(block: usize, kind: &str, finish: bool) -> (Vec<f32>, Option<usize>) {
    let (mut parts, hierarchy) = load(NOISE_SLOT);
    // Both closures edit the hierarchy, one after the other.
    let hierarchy = std::cell::RefCell::new(hierarchy);
    let slot = ModuleId(9);
    let layout = |_: u32| Layout::default();
    let mut finished = None;
    let left = render_edit_at(
        &mut parts,
        FRAMES,
        block,
        SWAP_AT,
        |parts| {
            let mut hierarchy = hierarchy.borrow_mut();
            hierarchy
                .swap_to_module(&mut parts.controller, &layout, slot, kind)
                .expect("the part fills Filter");
            hierarchy.sync(&mut parts.controller).expect("syncs");
            parts.controller.commit().expect("commits");
        },
        |position, parts| {
            if finish && finished.is_none() {
                let mut hierarchy = hierarchy.borrow_mut();
                let fading = hierarchy.fading_slots();
                let done = !fading.is_empty()
                    && fading
                        .iter()
                        .all(|(_, fades)| fades.iter().all(|f| parts.controller.display(*f, 0) == Some(1.0)));
                if done {
                    hierarchy.finish_all_fades(&mut parts.controller);
                    hierarchy.sync(&mut parts.controller).expect("syncs");
                    parts.controller.commit().expect("commits");
                    finished = Some(position);
                }
            }
        },
    );
    (left, finished)
}

/// Renders the patch with no swap.
fn render_plain(block: usize) -> Vec<f32> {
    let (mut parts, _) = load(NOISE_SLOT);
    render_edit_at(&mut parts, FRAMES, block, SWAP_AT, |_| {}, |_, _| {})
}

/// The first frame after the swap is the leaving part alone, the output
/// never goes silent or infinite through the fade, and after the fade the
/// output is the new part's: it differs from the SVF's render (sub-patches.md,
/// How a swap crossfades, point 2).
#[test]
fn a_swap_starts_on_the_leaving_part_and_ends_on_the_new_one() {
    let plain = render_plain(512);
    let (swapped, finished) = render_swap(512, "loom.filter.ladder@1", true);
    assert_eq!(
        swapped[..=SWAP_AT],
        plain[..=SWAP_AT],
        "up to and including the swap frame"
    );
    assert!(swapped.iter().all(|v| v.is_finite()));
    let after = &swapped[SWAP_AT + FADE..];
    assert!(after.iter().any(|v| v.abs() > 1e-4), "the ladder plays");
    assert_ne!(after, &plain[SWAP_AT + FADE..], "the ladder is not the SVF");
    let finished = finished.expect("the fade finished during the render");
    assert!(finished > SWAP_AT + FADE && finished < FRAMES, "{finished}");
}

/// Finishing the fade (removing the leaving part and the fade) changes
/// nothing the player hears: the render with the finish is bit-identical
/// to the one that leaves the fade in place, because a finished fade
/// outputs the new part exactly (point 3).
#[test]
fn finishing_the_fade_is_bit_identical_to_leaving_it() {
    let (with_finish, finished) = render_swap(77, "loom.filter.ladder@1", true);
    let (without, _) = render_swap(77, "loom.filter.ladder@1", false);
    assert!(finished.is_some());
    assert_eq!(with_finish, without);
}

/// The swap lands on the same frame at every block size, so the renders
/// are bit-identical (AGENTS.md, rule 2).
#[test]
fn a_swap_renders_the_same_at_every_block_size() {
    let (at_1, _) = render_swap(1, "loom.filter.ladder@1", false);
    let (at_77, _) = render_swap(77, "loom.filter.ladder@1", false);
    let (at_512, _) = render_swap(512, "loom.filter.ladder@1", false);
    assert_eq!(at_1, at_77);
    assert_eq!(at_1, at_512);
}

/// The fade's gains follow the quarter circle. The leaving SVF plays as it
/// would have, so its sample is the plain render's; solving the fade law
/// for the ladder's sample at the middle of the fade gives a sensible
/// value, the first fade frame is the SVF alone, and the frame after the
/// fade is the ladder alone.
#[test]
fn the_fade_mixes_the_two_parts_equal_power() {
    let plain = render_plain(1);
    let (swapped, _) = render_swap(1, "loom.filter.ladder@1", false);
    // out = a·cos θ + b·sin θ, with a known from the plain render.
    let i = SWAP_AT + FADE / 2;
    let theta = std::f32::consts::FRAC_PI_4;
    let a = plain[i];
    let b = (swapped[i] - a * theta.cos()) / theta.sin();
    assert!(b.is_finite() && b.abs() < 4.0, "a sensible ladder sample: {b}");
    assert_eq!(swapped[SWAP_AT], plain[SWAP_AT]);
    let after = SWAP_AT + FADE + 1;
    assert_ne!(swapped[after], plain[after]);
}

/// A sine through an amplifier whose gain a 30 Hz LFO in a Modulator slot
/// drives through an assignment, so the slot's output is a modulation
/// source and not a cable. One LFO cycle fits the steady window before the
/// swap, so that window's largest step is the signal's own.
const LFO_SLOT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "LFO slot"),
    modules: [
        PatchModule(id: 1, kind: "loom.phase.generator@1"),
        PatchModule(id: 2, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.level.amp@1", params: {"gain": 0.5}),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
        PatchModule(id: 5, kind: "loom.clock.transport@1"),
        PatchModule(id: 6, kind: "loom.mod.lfo@1", params: {"rate": 30.0}),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.modulator@1", part: Some(6)))),
    ],
    cables: [
        PatchCable(from: (1, "phase"), to: (2, "phase")),
        PatchCable(from: (2, "out"), to: (3, "in")),
        PatchCable(from: (3, "out"), to: (4, "in")),
    ],
    modulation: [PatchModulation(source: (9, "out"), target: (3, "gain"), depth: 0.4)],
)
"#;

/// Swapping a Modulator slot that drives a parameter through an
/// assignment fades too: the gain moves from the LFO's value to the
/// random's through the fade, so no step in the swap window is larger
/// than the steady audio on either side allows plus the fade's own gain
/// change (sub-patches.md, How a swap crossfades, point 2).
#[test]
fn a_modulation_source_slot_swaps_through_the_fade() {
    let (mut parts, hierarchy) = load(LFO_SLOT);
    let hierarchy = std::cell::RefCell::new(hierarchy);
    let layout = |_: u32| Layout::default();
    let mut finished = None;
    let left = render_edit_at(
        &mut parts,
        FRAMES,
        1,
        SWAP_AT,
        |parts| {
            let mut hierarchy = hierarchy.borrow_mut();
            hierarchy
                .swap_to_module(&mut parts.controller, &layout, ModuleId(9), "loom.rand.random@1")
                .expect("the random fills Modulator");
            hierarchy.sync(&mut parts.controller).expect("syncs");
            parts.controller.commit().expect("commits");
        },
        |position, parts| {
            if finished.is_none() {
                let mut hierarchy = hierarchy.borrow_mut();
                let fading = hierarchy.fading_slots();
                let done = !fading.is_empty()
                    && fading
                        .iter()
                        .all(|(_, fades)| fades.iter().all(|f| parts.controller.display(*f, 0) == Some(1.0)));
                if done {
                    hierarchy.finish_all_fades(&mut parts.controller);
                    hierarchy.sync(&mut parts.controller).expect("syncs");
                    parts.controller.commit().expect("commits");
                    finished = Some(position);
                }
            }
        },
    );
    let finished = finished.expect("the fade finished");
    let max_step = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let limit = |steady: &[f32]| max_step(steady) + peak(steady) * (std::f32::consts::FRAC_PI_2 / FADE as f32) + 1e-6;
    let before = &left[SWAP_AT - 2_000..SWAP_AT];
    let after = &left[finished + 200..finished + 2_200];
    assert!(peak(after) > 1e-3, "the sine plays");
    let bound = limit(before).max(limit(after));
    let across = max_step(&left[SWAP_AT - 1..finished + 200]);
    assert!(across <= bound, "swap step {across} > {bound}");
    // The assignment started at the fade and ends at the random.
    let graph = parts.controller.graph();
    let random = graph
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.rand.random")
        .map(|(id, _)| id)
        .expect("the random");
    assert!(graph.assignments().all(|(_, a)| a.source.module == random));
}

/// [`NOISE_SLOT`] with an LFO on the Filter role's cutoff.
const MODULATED_SLOT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Modulated slot"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 2, kind: "loom.filter.svf@1", params: {"cutoff": 300.0, "resonance": 0.6}),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 4, kind: "loom.clock.transport@1"),
        PatchModule(id: 5, kind: "loom.mod.lfo@1", params: {"rate": 3.0}),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(2)))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (3, "in")),
    ],
    modulation: [PatchModulation(source: (5, "out"), target: (9, "cutoff"), depth: 0.3)],
)
"#;

/// A slot swap and its finish only rearrange: every cable onto and off the
/// Swap fade switches, so no input fades (live patching spec, rule 9). A
/// modulated role parameter ramps out on the leaving part only; the new
/// part has it from its first frame.
#[test]
fn a_swap_and_its_finish_fade_no_cable() {
    for (text, leaving) in [(NOISE_SLOT, 0), (MODULATED_SLOT, 1)] {
        let (mut parts, hierarchy) = load(text);
        let hierarchy = std::cell::RefCell::new(hierarchy);
        let layout = |_: u32| Layout::default();
        let mut start = None;
        let mut finish = None;
        render_edit_at(
            &mut parts,
            FRAMES,
            64,
            SWAP_AT,
            |parts| {
                let mut hierarchy = hierarchy.borrow_mut();
                hierarchy
                    .swap_to_module(&mut parts.controller, &layout, ModuleId(9), "loom.filter.ladder@1")
                    .expect("the ladder fills Filter");
                hierarchy.sync(&mut parts.controller).expect("syncs");
                start = Some(parts.controller.commit().expect("commits").cable_fades);
            },
            |_, parts| {
                if finish.is_none() {
                    let mut hierarchy = hierarchy.borrow_mut();
                    let fading = hierarchy.fading_slots();
                    let done = !fading.is_empty()
                        && fading
                            .iter()
                            .all(|(_, fades)| fades.iter().all(|f| parts.controller.display(*f, 0) == Some(1.0)));
                    if done {
                        hierarchy.finish_all_fades(&mut parts.controller);
                        hierarchy.sync(&mut parts.controller).expect("syncs");
                        finish = Some(parts.controller.commit().expect("commits").cable_fades);
                    }
                }
            },
        );
        assert_eq!(start, Some(leaving), "the swap: only a leaving contribution ramps out");
        assert_eq!(finish, Some(0), "the finish switches");
    }
}

/// A swap during a fade, as the app makes it: the first fade is finished
/// in its own commit, then the swap commits. The output moves off the
/// unfinished fade onto the part it was fading to, so it fades once from
/// the mix, then switches onto the new fade, with no step larger than the
/// steady sound allows (rule 9, sub-patches.md point 5).
#[test]
fn a_swap_during_a_fade_fades_the_step_it_leaves() {
    let (mut parts, hierarchy) = load(NOISE_SLOT);
    let hierarchy = std::cell::RefCell::new(hierarchy);
    let layout = |_: u32| Layout::default();
    let mut second = None;
    let left = render_edit_at(
        &mut parts,
        FRAMES,
        1,
        SWAP_AT,
        |parts| {
            let mut hierarchy = hierarchy.borrow_mut();
            hierarchy
                .swap_to_module(&mut parts.controller, &layout, ModuleId(9), "loom.filter.ladder@1")
                .expect("the ladder fills Filter");
            hierarchy.sync(&mut parts.controller).expect("syncs");
            parts.controller.commit().expect("commits");
        },
        |position, parts| {
            if position == SWAP_AT + FADE / 2 {
                let mut hierarchy = hierarchy.borrow_mut();
                hierarchy.finish_all_fades(&mut parts.controller);
                hierarchy.sync(&mut parts.controller).expect("syncs");
                let finish = parts.controller.commit().expect("commits").cable_fades;
                hierarchy
                    .swap_to_module(&mut parts.controller, &layout, ModuleId(9), "loom.filter.onepole@1")
                    .expect("the one-pole fills Filter");
                hierarchy.sync(&mut parts.controller).expect("syncs");
                let swap = parts.controller.commit().expect("commits").cable_fades;
                second = Some((finish, swap));
            }
        },
    );
    let (finish, swap) = second.expect("the second swap ran");
    assert!(finish > 0, "moving off the unfinished fade fades");
    assert_eq!(swap, 0, "moving onto the new fade switches");
    assert!(left.iter().all(|v| v.is_finite()));
    let max_step = |s: &[f32]| s.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    let peak = |s: &[f32]| s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // The steady sound on either side, plus the cable fade's slope (held
    // and signal at most two peaks apart) and the swap fade's.
    let limit = |steady: &[f32]| {
        max_step(steady)
            + 2.0 * peak(steady) / 240.0
            + peak(steady) * (std::f32::consts::FRAC_PI_2 / FADE as f32)
            + 1e-6
    };
    let second_at = SWAP_AT + FADE / 2;
    let bound = limit(&left[..SWAP_AT]).max(limit(&left[second_at + 2 * FADE..]));
    let across = max_step(&left[second_at - 1..second_at + 2 * FADE]);
    assert!(across <= bound, "{across} > {bound}");
}
