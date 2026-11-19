use super::*;
use crate::slot::tests::SLOT_FILE;
use crate::{FlatIds, PartKey, Patch};
use loom_core::{EngineConfig, EngineParts, MidiControl, create_engine, roles};

/// Loads `patch` into a fresh engine and reads its hierarchy.
fn load(patch: &Patch) -> (Controller, Hierarchy) {
    let EngineParts { mut controller, .. } = create_engine(loom_modules::registry(), EngineConfig::default());
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).expect("the patch loads");
    let hierarchy =
        Hierarchy::from_patch(patch, controller.graph(), ids, controller.registry()).expect("the hierarchy reads");
    (controller, hierarchy)
}

/// The patch as the hierarchy would save it, with no positions.
fn written(hierarchy: &Hierarchy, controller: &Controller) -> Patch {
    let mut patch = hierarchy.unflatten(controller.graph(), |_| Layout::default());
    patch.metadata = Metadata {
        name: "Slots".into(),
        ..Metadata::default()
    };
    patch
}

/// The graph's cables as (output, input) in flat ids and names.
fn cables(controller: &Controller) -> BTreeSet<(End, End)> {
    graph_cables(controller.graph())
}

/// The flat id of the tile with level id `lid` inside the Voice tile.
fn inside(hierarchy: &Hierarchy, lid: u32) -> ModuleId {
    hierarchy.id_at(&ModulePath::top(2), lid).expect("a tile inside Voice")
}

/// The one module of `kind` in the graph.
fn module_of(controller: &Controller, kind: &str) -> ModuleId {
    controller
        .graph()
        .nodes()
        .find(|(_, n)| n.descriptor.id == kind)
        .map(|(id, _)| id)
        .unwrap_or_else(|| panic!("a {kind}"))
}

/// A sine into an empty Filter slot into the output, and an empty
/// Envelope slot whose output drives nothing yet.
const EMPTY_SLOTS: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Empty slots"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1"))),
        PatchModule(id: 10, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.envelope@1"))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (3, "in")),
    ],
)"#;

mod editing;
mod moves;
mod stack;
mod thru;
