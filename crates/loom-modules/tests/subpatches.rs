//! Sub-patches in the engine (`docs/design/sub-patches.md`, Acceptance
//! criteria, Engine). Flattening (`loom-patch`) gives the engine a flat
//! graph, so a nested patch must play exactly as the same patch built flat
//! (FR-ENCAP-07), and a feedback loop across a boundary is found like any
//! other.

mod common;

use common::{Note, SAMPLE_RATE, load_patch, patch_path, render};
use loom_core::{EngineConfig, EngineParts, ModuleId, OutputSafety, create_engine};
use loom_patch::Patch;
use std::path::PathBuf;

/// Length of each render: 0.2 s, as the golden patch tests use.
const FRAMES: usize = 9_600;

/// The note every render plays.
const NOTE: Note = Note {
    note: 60,
    on: 240,
    off: 7_200,
};

/// The dev example `name`, which the app does not list.
fn dev_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/dev")
        .join(name)
}

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
    let nested = load_patch(&dev_path("nested-subtractive.ron"));
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
