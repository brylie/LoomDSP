//! Sub-patches in the engine (`docs/design/sub-patches.md`, Acceptance
//! criteria, Engine). Flattening (`loom-patch`) gives the engine a flat
//! graph, so a nested patch must play exactly as the same patch built flat
//! (FR-ENCAP-07), and a feedback loop across a boundary is found like any
//! other.

mod common;

use common::{Note, SAMPLE_RATE, load_patch, patch_path, render, render_with};
use loom_core::{EngineConfig, EngineParts, ModuleId, OutputSafety, create_engine};
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
