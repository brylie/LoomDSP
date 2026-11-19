use super::*;
use loom_core::{EngineConfig, EngineParts, create_engine};
use std::path::PathBuf;

/// Loads `patch` into a fresh engine and reads its hierarchy.
pub(super) fn load(patch: &Patch) -> (Controller, Hierarchy) {
    let EngineParts { mut controller, .. } = create_engine(loom_modules::registry(), EngineConfig::default());
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).expect("the patch loads");
    let hierarchy =
        Hierarchy::from_patch(patch, controller.graph(), ids, controller.registry()).expect("the hierarchy reads");
    (controller, hierarchy)
}

/// Loads patch text.
fn load_text(text: &str) -> (Controller, Hierarchy) {
    load(&Patch::from_ron(text).unwrap())
}

/// The patch as the hierarchy would save it, with the file's metadata and
/// no positions.
fn written(hierarchy: &Hierarchy, controller: &Controller, metadata: &Metadata) -> Patch {
    let mut patch = hierarchy.unflatten(controller.graph(), |_| Layout::default());
    patch.metadata = metadata.clone();
    patch
}

/// The canonical text of a patch.
fn text(patch: &Patch) -> String {
    patch.to_ron().unwrap()
}

/// The graph's cables as (output, input) in flat ids and names.
fn cables(controller: &Controller) -> BTreeSet<(End, End)> {
    graph_cables(controller.graph())
}

/// The graph's cables by path, so two engines that loaded the same
/// structure with different flat ids compare equal.
fn cables_by_path(
    controller: &Controller,
    hierarchy: &Hierarchy,
) -> BTreeSet<((ModulePath, String), (ModulePath, String))> {
    cables(controller)
        .into_iter()
        .map(|(from, to)| {
            (
                (hierarchy.path_of(ModuleId(from.0)), from.1),
                (hierarchy.path_of(ModuleId(to.0)), to.1),
            )
        })
        .collect()
}

/// The nested subtractive example, wherever it lives.
fn nested_example() -> Patch {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let path = ["patches", "dev"]
        .iter()
        .map(|folder| root.join(folder).join("nested-subtractive.ron"))
        .find(|path| path.exists())
        .expect("the nested example exists");
    Patch::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A flat patch: a note into a saw voice with a filter, an LFO on the
/// cutoff, and a scope on the filter.
const FLAT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Flat"),
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.saw@1"),
        PatchModule(id: 4, kind: "loom.filter.svf@1", params: {"cutoff": 800.0}),
        PatchModule(id: 5, kind: "loom.mod.lfo@1"),
        PatchModule(id: 9, kind: "loom.io.audio-out@1"),
        PatchModule(id: 10, kind: "loom.clock.transport@1"),
        PatchModule(id: 11, kind: "loom.display.scope@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "in")),
        PatchCable(from: (5, "out"), to: (4, "cutoff")),
        PatchCable(from: (4, "lowpass"), to: (9, "in")),
        PatchCable(from: (4, "lowpass"), to: (11, "in")),
    ],
)"#;

/// The nested example saves as the file it was loaded from: the stored
/// value inside Filter, the empty value on the inner instance, and the
/// live value on the top instance all come back (ADR-0039, invariant 6).
#[test]
fn the_nested_example_round_trips_through_unflatten() {
    let file = nested_example();
    let (controller, hierarchy) = load(&file);
    assert!(!hierarchy.is_flat());
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert_eq!(text(&saved), text(&file));
    // What the saved file loads is what plays.
    let (again, _) = load(&saved);
    assert_eq!(cables(&again), cables(&controller));
}

/// A flat patch saves exactly as `Patch::from_graph_with` saved it: the
/// hierarchy changes nothing for a patch with no sub-patch.
#[test]
fn every_flat_example_unflattens_as_from_graph_with_does() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    let mut checked = 0;
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "ron") {
            continue;
        }
        let file = Patch::from_ron(&std::fs::read_to_string(&path).unwrap()).unwrap();
        if !file.subpatches.is_empty() {
            continue;
        }
        let (controller, hierarchy) = load(&file);
        assert!(hierarchy.is_flat());
        let layout = |id: u32| {
            file.modules
                .iter()
                .find(|m| m.id == id)
                .map_or_else(Layout::default, |m| Layout {
                    position: m.position,
                    size: m.size,
                })
        };
        let mut expected = Patch::from_graph_with(file.metadata.name.clone(), controller.graph(), |id: ModuleId| {
            layout(id.0)
        });
        expected.metadata = file.metadata.clone();
        let mut saved = hierarchy.unflatten(controller.graph(), layout);
        saved.metadata = file.metadata.clone();
        assert_eq!(text(&saved), text(&expected), "{}", path.display());
        checked += 1;
    }
    assert!(checked > 5, "the examples folder has flat patches");
}

mod editing;
mod files;
