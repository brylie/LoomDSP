//! Sub-patches in the engine (`docs/design/sub-patches.md`, Acceptance
//! criteria, Engine). Flattening (`loom-patch`) gives the engine a flat
//! graph, so a nested patch must play exactly as the same patch built flat
//! (FR-ENCAP-07), and a feedback loop across a boundary is found like any
//! other.

mod common;

use common::{Note, SAMPLE_RATE, load_patch, patch_path, render, render_with};
use loom_core::{EngineConfig, EngineParts, ModuleId, ModuleNames, OutputSafety, create_engine};
use loom_patch::Patch;

/// Length of each render: 0.2 s, as the golden patch tests use.
const FRAMES: usize = 9_600;

/// The note every render plays.
const NOTE: Note = Note {
    note: 60,
    on: 240,
    off: 7_200,
};

/// An engine with `patch` loaded and output safety off, so the render
/// holds what the patch makes (ADR-0019).
fn load(patch: &Patch) -> EngineParts {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(SAMPLE_RATE)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    patch.load_into(&mut parts.controller).expect("the patch loads");
    parts
}

/// The subtractive example built as three levels renders bit-identical to
/// the flat one at host buffers of 1, 77, and 512 frames. The nested
/// file's inner cutoff is 1000 Hz and only the top instance sets 300 Hz,
/// so this also proves the exposed value reaches the inner parameter.
#[test]
fn three_levels_render_bit_identical_to_the_flat_patch() {
    let nested = load_patch(&patch_path("nested-subtractive.ron"));
    let flat = load_patch(&patch_path("subtractive.ron"));
    assert_eq!(nested.subpatches.len(), 2);
    for block in [1, 77, 512] {
        let (left, right) = render(&mut load(&nested), FRAMES, block, &[NOTE]);
        let (flat_left, flat_right) = render(&mut load(&flat), FRAMES, block, &[NOTE]);
        assert!(left.iter().any(|&s| s != 0.0), "the nested patch makes sound");
        assert!(left == flat_left && right == flat_right, "block {block} differs");
    }
}

/// A feedback loop that crosses a sub-patch boundary gets a unit delay
/// where a flat loop would, and none when the definition holds a
/// `loom.time.delay1` of its own.
#[test]
fn a_loop_across_a_boundary_gets_a_unit_delay() {
    let text = |inside: &str| {
        format!(
            r#"Patch(
                version: 1,
                subpatches: {{
                    "fb": SubPatch(
                        metadata: Metadata(name: "Feedback"),
                        modules: [
                            PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                            PatchModule(id: 2, kind: "loom.math.mul@1", params: {{"gain": 0.3}}),
                            {inside}
                            PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                        ],
                        cables: [
                            PatchCable(from: (1, "out"), to: (2, "a")),
                            PatchCable(from: (2, "out"), to: (3, "in")),
                            PatchCable(from: (3, "out"), to: (4, "in")),
                        ],
                    ),
                }},
                modules: [
                    PatchModule(id: 1, kind: "loom.phase.generator@1"),
                    PatchModule(id: 2, kind: "loom.osc.sine@1"),
                    PatchModule(id: 3, kind: "loom.struct.subpatch@1", subpatch: Some("fb")),
                    PatchModule(id: 4, kind: "loom.io.audio-out@1"),
                    PatchModule(id: 5, kind: "loom.clock.transport@1"),
                ],
                cables: [
                    PatchCable(from: (1, "phase"), to: (2, "phase")),
                    PatchCable(from: (2, "out"), to: (3, "in")),
                    PatchCable(from: (3, "out"), to: (1, "pitch")),
                    PatchCable(from: (2, "out"), to: (4, "in")),
                ],
            )"#
        )
    };
    let through = r#"PatchModule(id: 3, kind: "loom.level.clamp@1"),"#;
    let parts = load(&Patch::from_ron(&text(through)).unwrap());
    let report = parts.controller.last_report();
    assert_eq!(report.loops.len(), 1, "{report:?}");
    let feedback = &report.loops[0];
    assert_eq!(feedback.modules.len(), 4);
    assert!(feedback.modules.contains(&ModuleId(1)) && feedback.modules.contains(&ModuleId(2)));
    assert_eq!(feedback.inserted_delays.len(), 1);

    let delayed = r#"PatchModule(id: 3, kind: "loom.time.delay1@1"),"#;
    let parts = load(&Patch::from_ron(&text(delayed)).unwrap());
    let report = parts.controller.last_report();
    assert_eq!(report.loops.len(), 1);
    assert!(report.loops[0].inserted_delays.is_empty(), "{report:?}");
}

/// Collapsing three modules of the flat subtractive patch into a sub-patch
/// renders bit-identical to the flat patch at host buffers of 1, 77, and
/// 512 frames: the file the editor writes after a collapse plays the same
/// (sub-patches.md, rule 2).
#[test]
fn a_collapse_renders_bit_identical_to_the_flat_patch() {
    use loom_patch::{FlatIds, Hierarchy, Layout, ModulePath};
    let flat = load_patch(&patch_path("subtractive.ron"));
    // The editor's steps: load, collapse the phase generator, the saw, and
    // the filter, sync, and write the file.
    let mut parts = load(&flat);
    let mut ids = FlatIds::new();
    let mut edited = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(SAMPLE_RATE));
    flat.load_with_ids(&mut edited.controller, &mut ids).unwrap();
    let mut hierarchy = Hierarchy::from_patch(&flat, edited.controller.graph(), ids).unwrap();
    let oscillator = flat
        .modules
        .iter()
        .filter(|m| ["loom.phase.generator@1", "loom.osc.saw@1", "loom.filter.svf@1"].contains(&m.kind.as_str()))
        .map(|m| m.id)
        .collect::<Vec<_>>();
    assert_eq!(oscillator.len(), 3, "the subtractive example has these three");
    let made = hierarchy
        .collapse(edited.controller.graph(), &ModulePath::root(), &oscillator)
        .unwrap();
    assert!(!made.inputs.is_empty() && !made.outputs.is_empty());
    hierarchy.sync(&mut edited.controller).unwrap();
    let mut collapsed = hierarchy.unflatten(edited.controller.graph(), |_| Layout::default());
    collapsed.metadata = flat.metadata.clone();
    assert_eq!(collapsed.subpatches.len(), 1);
    for block in [1, 77, 512] {
        let (left, right) = render(&mut parts, FRAMES, block, &[NOTE]);
        let (c_left, c_right) = render(&mut load(&collapsed), FRAMES, block, &[NOTE]);
        assert!(left.iter().any(|&s| s != 0.0));
        assert!(left == c_left && right == c_right, "block {block} differs");
        parts = load(&flat);
    }
}

/// An edit inside a sub-patch keeps the state of the other inner modules:
/// a module added inside while the patch plays leaves the oscillators'
/// phases alone, so the render equals one with no edit.
#[test]
fn an_edit_inside_keeps_the_state_of_the_other_inner_modules() {
    let nested = load_patch(&patch_path("nested-subtractive.ron"));
    let (left, right) = render(&mut load(&nested), FRAMES, 64, &[NOTE]);
    let mut parts = load(&nested);
    let mut added = false;
    let (e_left, e_right) = render_with(&mut parts, FRAMES, 64, &[NOTE], |position, parts| {
        if position >= 2_048 && !added {
            // As the app does for a module added inside the Voice: a new
            // flat module, unconnected, and one commit.
            parts.controller.add_module("loom.osc.sine").unwrap();
            parts.controller.commit().unwrap();
            added = true;
        }
    });
    assert!(added);
    assert!(left.iter().any(|&s| s != 0.0));
    assert!(left == e_left && right == e_right, "the edit changed the sound");
}

/// A flat patch whose filter cutoff three sources can modulate: an LFO and
/// an envelope outside the part that collapses, and the saw inside it.
const THREE_SOURCES: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Three sources"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.filter.svf@1", params: {"cutoff": 800.0, "resonance": 0.5}),
        PatchModule(id: 5, kind: "loom.mod.lfo@1", params: {"rate": 3.0}),
        PatchModule(id: 6, kind: "loom.env.envelope@1", params: {"attack": 0.01, "decay": 0.1, "sustain": 0.3}),
        PatchModule(id: 9, kind: "loom.io.audio-out@1"),
        PatchModule(id: 10, kind: "loom.clock.transport@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (1, "gate"), to: (6, "gate")),
        PatchCable(from: (4, "lowpass"), to: (9, "in")),
    ],
)"#;

/// An engine with output safety off, as [`load`] makes, and nothing in it.
fn empty_engine() -> EngineParts {
    create_engine(
        loom_modules::registry(),
        EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(SAMPLE_RATE)
        },
    )
}

/// The [`THREE_SOURCES`] patch loaded as the editor loads it, with its
/// hierarchy, and an output on module `module`.
fn three_sources() -> (EngineParts, loom_patch::Hierarchy, Patch) {
    use loom_patch::{FlatIds, Hierarchy};
    let flat = Patch::from_ron(THREE_SOURCES).unwrap();
    let mut parts = empty_engine();
    let mut ids = FlatIds::new();
    flat.load_with_ids(&mut parts.controller, &mut ids).unwrap();
    let hierarchy = Hierarchy::from_patch(&flat, parts.controller.graph(), ids).unwrap();
    (parts, hierarchy, flat)
}

/// An output reference on the first output of `module`.
fn out(module: u32) -> loom_core::OutputRef {
    loom_core::OutputRef {
        module: ModuleId(module),
        port: 0,
    }
}

/// Three assignments on one cutoff, two from outside the sub-patch and one
/// from inside, made outer first. The engine sums them in id order, and
/// with three terms the order changes the last bits, so the live sum
/// differs from what the saved file loads unless the sync reorders them.
/// After the sync the render is bit-identical to the reloaded file's
/// (sub-patches.md, Editor model, Assignment order is kept canonical).
#[test]
fn assignments_from_both_sides_sum_as_the_file_loads_them() {
    use loom_patch::{Layout, ModulePath};
    // The editor's steps, with and without the sync that follows an
    // assignment, each in a fresh engine so the renders start cold.
    let build = |sync: bool| {
        let (mut parts, mut hierarchy, _) = three_sources();
        let made = hierarchy
            .collapse(parts.controller.graph(), &ModulePath::root(), &[2, 3, 4])
            .unwrap();
        hierarchy.sync(&mut parts.controller).unwrap();
        let inside = ModulePath::top(made.instance.0);
        hierarchy
            .expose(parts.controller.graph(), &inside, 4, "cutoff")
            .unwrap();
        hierarchy.sync(&mut parts.controller).unwrap();
        let svf = hierarchy.id_at(&inside, 4).unwrap();
        let saw = hierarchy.id_at(&inside, 3).unwrap();
        let cutoff = parts
            .controller
            .graph()
            .node(svf)
            .unwrap()
            .parameter_index("cutoff")
            .unwrap();
        parts.controller.add_assignment(out(5), svf, cutoff, 0.13).unwrap();
        parts.controller.add_assignment(out(6), svf, cutoff, 0.31).unwrap();
        parts.controller.add_assignment(out(saw.0), svf, cutoff, 0.07).unwrap();
        if sync {
            hierarchy.sync(&mut parts.controller).unwrap();
        }
        parts.controller.commit().unwrap();
        (parts, hierarchy)
    };
    let (mut unsynced, _) = build(false);
    let (mut synced, hierarchy) = build(true);
    let order: Vec<ModulePath> = synced
        .controller
        .graph()
        .assignments()
        .map(|(_, a)| hierarchy.path_of(a.source.module))
        .collect();
    assert_eq!(order[0].0.len(), 2, "the inner assignment sums first");
    let mut saved = hierarchy.unflatten(synced.controller.graph(), |_| Layout::default());
    saved.metadata.name = "saved".to_string();
    let (left, right) = render(&mut synced, FRAMES, 64, &[NOTE]);
    let (r_left, r_right) = render(&mut load(&saved), FRAMES, 64, &[NOTE]);
    assert!(left.iter().any(|&s| s != 0.0));
    assert!(
        left == r_left && right == r_right,
        "the reloaded file plays what was heard"
    );
    let (u_left, _) = render(&mut unsynced, FRAMES, 64, &[NOTE]);
    assert!(u_left != left, "without the sync the sum order differs from the file's");
}

/// Collapsing with three assignments on one parameter, one of whose
/// sources goes inside, puts the inner one first in the live order (rule
/// 2's exception), and the saved file plays exactly what plays after the
/// collapse.
#[test]
fn a_collapse_with_three_sources_reloads_as_it_plays() {
    use loom_patch::{Layout, ModulePath};
    let (mut parts, mut hierarchy, _) = three_sources();
    let cutoff = parts
        .controller
        .graph()
        .node(ModuleId(4))
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    // The saw's assignment sits between the two outer ones.
    parts
        .controller
        .add_assignment(out(5), ModuleId(4), cutoff, 0.13)
        .unwrap();
    parts
        .controller
        .add_assignment(out(3), ModuleId(4), cutoff, 0.07)
        .unwrap();
    parts
        .controller
        .add_assignment(out(6), ModuleId(4), cutoff, 0.31)
        .unwrap();
    parts.controller.commit().unwrap();
    let made = hierarchy
        .collapse(parts.controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut parts.controller).unwrap();
    parts.controller.commit().unwrap();
    let inside = ModulePath::top(made.instance.0);
    let order: Vec<ModulePath> = parts
        .controller
        .graph()
        .assignments()
        .map(|(_, a)| hierarchy.path_of(a.source.module))
        .collect();
    assert_eq!(order, [inside.child(3), ModulePath::top(5), ModulePath::top(6)]);
    let mut saved = hierarchy.unflatten(parts.controller.graph(), |_| Layout::default());
    saved.metadata.name = "saved".to_string();
    let (left, right) = render(&mut parts, FRAMES, 64, &[NOTE]);
    let (r_left, r_right) = render(&mut load(&saved), FRAMES, 64, &[NOTE]);
    assert!(left.iter().any(|&s| s != 0.0));
    assert!(
        left == r_left && right == r_right,
        "the reloaded file plays what plays after the collapse"
    );
}

/// A sub-patch written as a library file and inserted into a fresh patch
/// renders bit-identical to the patch it came from, at block sizes 1, 77,
/// and 512: Save to library and Insert keep what plays (sub-patches.md,
/// Library).
#[test]
fn a_sub_patch_saved_to_the_library_plays_the_same_inserted_again() {
    use loom_patch::{FlatIds, Hierarchy, Layout, ModulePath};
    let nested = load_patch(&patch_path("nested-subtractive.ron"));
    // The Voice tile as a library file, from the nested example.
    let file = {
        let mut ids = FlatIds::new();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(SAMPLE_RATE));
        nested.load_with_ids(&mut parts.controller, &mut ids).expect("loads");
        let hierarchy = Hierarchy::from_patch(&nested, parts.controller.graph(), ids).expect("reads");
        hierarchy
            .definition_file(parts.controller.graph(), |_| Layout::default(), ModuleId(2))
            .expect("the Voice tile has a file")
    };
    assert_eq!(file.metadata.name, "Voice");
    assert_eq!(file.subpatches.len(), 1, "the Filter comes along");
    // The example without its Voice, with the file inserted in its place.
    let mut host = nested.clone();
    host.subpatches.clear();
    host.modules.retain(|m| m.id != 2);
    host.cables.retain(|c| c.from.0 != 2 && c.to.0 != 2);
    for block in [1, 77, 512] {
        let (left, right) = render(&mut load(&nested), FRAMES, block, &[NOTE]);
        let config = EngineConfig {
            output_safety: OutputSafety::Off,
            ..EngineConfig::with_sample_rate(SAMPLE_RATE)
        };
        let mut parts = create_engine(loom_modules::registry(), config);
        let mut ids = FlatIds::new();
        host.load_with_ids(&mut parts.controller, &mut ids)
            .expect("the host loads");
        let mut hierarchy = Hierarchy::from_patch(&host, parts.controller.graph(), ids).expect("reads");
        let inserted = hierarchy
            .insert(&mut parts.controller, &ModulePath::root(), &file, None)
            .expect("the file inserts");
        let lid = hierarchy.path_of(inserted.instance).last();
        let root = ModulePath::root();
        hierarchy.connect(&root, (1, "pitch".to_string()), (lid, "pitch".to_string()));
        hierarchy.connect(&root, (1, "gate".to_string()), (lid, "gate".to_string()));
        hierarchy.connect(&root, (lid, "out".to_string()), (9, "in".to_string()));
        hierarchy.connect(&root, (lid, "filter".to_string()), (11, "in".to_string()));
        hierarchy.sync(&mut parts.controller).expect("syncs");
        // The example's instance sets the cutoff; the file holds the
        // definition's own value.
        let cutoff = hierarchy.exposed_target(inserted.instance, 0).expect("the cutoff");
        parts
            .controller
            .set_param_named(cutoff.module, &cutoff.param, 300.0)
            .unwrap();
        parts.controller.commit().unwrap();
        let (again_left, again_right) = render(&mut parts, FRAMES, block, &[NOTE]);
        assert!(left.iter().any(|&s| s != 0.0), "the patch makes sound");
        assert!(left == again_left && right == again_right, "block {block} differs");
    }
}
