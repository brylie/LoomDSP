use super::*;
use loom_core::{EngineConfig, EngineParts, create_engine};
use std::path::PathBuf;

/// Loads `patch` into a fresh engine and reads its hierarchy.
fn load(patch: &Patch) -> (Controller, Hierarchy) {
    let EngineParts { mut controller, .. } = create_engine(loom_modules::registry(), EngineConfig::default());
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).expect("the patch loads");
    let hierarchy = Hierarchy::from_patch(patch, controller.graph(), ids).expect("the hierarchy reads");
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

/// Collapsing the phase generator, the saw, and the filter, which two
/// cables feed and one cable leaves, gives a tile with two inputs and one
/// output. The graph does not change, and the file the hierarchy writes
/// flattens back to the same cables.
#[test]
fn collapsing_three_modules_gives_two_inputs_and_one_output() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let before = cables(&controller);
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    assert_eq!(made.inputs.len(), 2, "{made:?}");
    assert_eq!(made.outputs.len(), 1, "{made:?}");
    assert_eq!(made.members.len(), 3);
    let (inputs, outputs) = hierarchy.instance_ports(made.instance);
    assert_eq!(
        inputs.iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>(),
        ["Pitch", "Cutoff"]
    );
    assert_eq!(outputs[0].1, "Lowpass");
    // The members kept their ids and moved inside.
    assert_eq!(hierarchy.level_of(ModuleId(3)), hierarchy.path_of(made.instance));
    assert_eq!(hierarchy.path_of(ModuleId(3)).last(), 3);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(cables(&controller), before, "collapsing changes no cable");
    // The file: both levels, and the fan-out from the filter stays
    // outside as two cables from the one output port.
    let saved = written(&hierarchy, &controller, &Metadata::default());
    assert_eq!(saved.subpatches.len(), 1);
    let definition = saved.subpatches.values().next().unwrap();
    assert_eq!(definition.metadata.name, DEFAULT_NAME);
    assert_eq!(definition.modules.len(), 6);
    assert_eq!(saved.cables.iter().filter(|c| c.from.0 == made.instance.0).count(), 2);
    let (again, again_h) = load(&saved);
    assert_eq!(
        cables_by_path(&again, &again_h),
        cables_by_path(&controller, &hierarchy)
    );
}

/// Dissolving the sub-patch gives back the same modules and cables as
/// the flat file (Collapsing, point 7, in reverse).
#[test]
fn dissolving_gives_back_the_same_modules_and_cables() {
    let file = Patch::from_ron(FLAT).unwrap();
    let (mut controller, mut hierarchy) = load(&file);
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let undone = hierarchy.dissolve(controller.graph(), made.instance).unwrap();
    assert_eq!(undone.name, DEFAULT_NAME);
    assert_eq!(undone.members, vec![ModuleId(2), ModuleId(3), ModuleId(4)]);
    assert_eq!(undone.removed.len(), 4);
    hierarchy.sync(&mut controller).unwrap();
    assert!(hierarchy.is_flat());
    assert_eq!(text(&written(&hierarchy, &controller, &file.metadata)), text(&file));
}

/// An assignment from outside onto a parameter inside becomes an exposed
/// parameter, and the file targets the exposed name (Collapsing, point 4).
#[test]
fn an_outside_assignment_becomes_an_exposed_parameter() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let lfo = OutputRef {
        module: ModuleId(5),
        port: 0,
    };
    let cutoff = controller
        .graph()
        .node(ModuleId(4))
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    controller.add_assignment(lfo, ModuleId(4), cutoff, 0.5).unwrap();
    controller.commit().unwrap();
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    let definition = hierarchy.definition_of(made.instance).unwrap();
    assert_eq!(definition.exposed.len(), 1);
    assert_eq!(definition.exposed[0].name, "Cutoff");
    assert_eq!(definition.exposed[0].module, 4);
    assert_eq!(definition.exposed[0].param, "cutoff");
    assert_eq!(definition.defaults[&(4, "cutoff".to_string())].value, Some(800.0));
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &Metadata::default());
    assert_eq!(saved.modulation.len(), 1);
    assert_eq!(saved.modulation[0].target, (made.instance.0, "Cutoff".to_string()));
    let instance = saved.modules.iter().find(|m| m.id == made.instance.0).unwrap();
    assert_eq!(instance.params.get("Cutoff"), Some(&800.0));
    // The file plays the same assignment.
    let (again, again_h) = load(&saved);
    let assignments: Vec<_> = again.graph().assignments().map(|(_, a)| *a).collect();
    assert_eq!(assignments.len(), 1);
    assert_eq!(
        again_h.path_of(assignments[0].target),
        ModulePath(vec![made.instance.0, 4])
    );
    assert_eq!(assignments[0].source, lfo);
}

/// An assignment from inside to a parameter outside gets an output port
/// for its source, and the file's assignment starts at that port.
#[test]
fn an_inside_assignment_gets_an_output_port() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let lfo = OutputRef {
        module: ModuleId(5),
        port: 0,
    };
    let cutoff = controller
        .graph()
        .node(ModuleId(4))
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    controller.add_assignment(lfo, ModuleId(4), cutoff, 0.5).unwrap();
    controller.commit().unwrap();
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[5])
        .unwrap();
    assert_eq!(made.outputs.len(), 1);
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &Metadata::default());
    assert_eq!(saved.modulation[0].source, (made.instance.0, "Out".to_string()));
    let (again, again_h) = load(&saved);
    let assignments: Vec<_> = again.graph().assignments().map(|(_, a)| *a).collect();
    assert_eq!(assignments.len(), 1);
    assert_eq!(
        again_h.path_of(assignments[0].source.module),
        ModulePath(vec![made.instance.0, 5])
    );
    assert_eq!(assignments[0].target, ModuleId(4));
}

/// The Transport and a top-level Patch input count as outside: their
/// cables into the selection become ports, and selecting only them is
/// refused with the message (Collapsing, point 6).
#[test]
fn the_transport_and_top_level_ports_stay_outside() {
    let text = r#"Patch(
        version: 1,
        modules: [
            PatchModule(id: 1, kind: "loom.struct.input@1"),
            PatchModule(id: 2, kind: "loom.math.mul@1"),
            PatchModule(id: 3, kind: "loom.io.audio-out@1"),
            PatchModule(id: 4, kind: "loom.clock.transport@1"),
            PatchModule(id: 5, kind: "loom.osc.sine@1"),
        ],
        cables: [
            PatchCable(from: (1, "out"), to: (2, "a")),
            PatchCable(from: (4, "beat-phase"), to: (5, "phase")),
            PatchCable(from: (5, "out"), to: (2, "b")),
            PatchCable(from: (2, "out"), to: (3, "in")),
        ],
    )"#;
    let (mut controller, mut hierarchy) = load_text(text);
    assert_eq!(
        hierarchy
            .collapse(controller.graph(), &ModulePath::root(), &[4, 1])
            .unwrap_err(),
        NOTHING_TO_COLLAPSE
    );
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[1, 2, 4, 5])
        .unwrap();
    assert_eq!(made.members, vec![ModuleId(2), ModuleId(5)]);
    assert_eq!(made.inputs.len(), 2, "the Patch input's and the Transport's cables");
    assert_eq!(hierarchy.level_of(ModuleId(1)), ModulePath::root());
    assert_eq!(hierarchy.level_of(ModuleId(4)), ModulePath::root());
    let before = cables(&controller);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(cables(&controller), before);
}

/// A sub-patch can hold a sub-patch: collapsing an instance with a module
/// keeps the inner one, and the file flattens back to the same cables.
#[test]
fn a_nested_collapse_keeps_the_inner_instance() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let before = cables(&controller);
    let inner = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let outer = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[inner.instance.0, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(cables(&controller), before);
    assert_eq!(hierarchy.level_of(inner.instance), hierarchy.path_of(outer.instance));
    assert_eq!(
        hierarchy.path_names(&hierarchy.path_of(inner.instance)),
        vec!["Sub-patch 2".to_string(), "Sub-patch".to_string()]
    );
    let saved = written(&hierarchy, &controller, &Metadata::default());
    assert_eq!(saved.subpatches.len(), 2);
    let (again, again_h) = load(&saved);
    assert_eq!(
        cables_by_path(&again, &again_h),
        cables_by_path(&controller, &hierarchy)
    );
    // And back out, twice.
    hierarchy.dissolve(controller.graph(), outer.instance).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    hierarchy.dissolve(controller.graph(), inner.instance).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert!(hierarchy.is_flat());
    assert_eq!(cables(&controller), before);
}

/// Inside a definition, a Patch input cabled straight to a Patch output is
/// a pass-through: syncing joins what feeds the instance to what it feeds.
#[test]
fn a_pass_through_made_by_hand_joins_the_outside_cables() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let inside = hierarchy.path_of(made.instance);
    let phase_in = hierarchy.path_of(made.inputs[0]).last();
    let out = hierarchy.path_of(made.outputs[0]).last();
    hierarchy.connect(&inside, (phase_in, "out".to_string()), (out, "in".to_string()));
    hierarchy.sync(&mut controller).unwrap();
    let now = cables(&controller);
    assert!(now.contains(&((2, "phase".to_string()), (9, "in".to_string()))));
    assert!(now.contains(&((2, "phase".to_string()), (11, "in".to_string()))));
    assert!(!now.contains(&((4, "lowpass".to_string()), (9, "in".to_string()))));
    assert!(hierarchy.is_implied(
        controller.graph(),
        OutputRef {
            module: ModuleId(2),
            port: 0
        },
        InputRef {
            module: ModuleId(9),
            port: 0
        }
    ));
    let saved = written(&hierarchy, &controller, &Metadata::default());
    let (again, again_h) = load(&saved);
    assert_eq!(
        cables_by_path(&again, &again_h),
        cables_by_path(&controller, &hierarchy)
    );
}

/// Outside, a new cable into an instance port replaces the one there, and
/// inside, taking the port's cable away leaves the input unfed.
#[test]
fn links_replace_and_remove_the_cables_they_imply() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let instance = hierarchy.path_of(made.instance).last();
    // The LFO now feeds the pitch port instead of the note.
    hierarchy.connect(
        &ModulePath::root(),
        (5, "out".to_string()),
        (instance, "Pitch".to_string()),
    );
    hierarchy.sync(&mut controller).unwrap();
    let now = cables(&controller);
    assert!(now.contains(&((5, "out".to_string()), (2, "pitch".to_string()))));
    assert!(!now.contains(&((1, "pitch".to_string()), (2, "pitch".to_string()))));
    // Inside, the cutoff port no longer feeds the filter.
    let inside = hierarchy.path_of(made.instance);
    assert!(hierarchy.disconnect(&inside, &(4, "cutoff".to_string())));
    hierarchy.sync(&mut controller).unwrap();
    let now = cables(&controller);
    assert!(!now.contains(&((5, "out".to_string()), (4, "cutoff".to_string()))));
    assert!(!hierarchy.disconnect(&inside, &(4, "cutoff".to_string())));
}

/// Removing an instance takes every module inside it, its ports, and the
/// cables through them.
#[test]
fn removing_an_instance_removes_everything_inside() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let removed = hierarchy.remove(controller.graph(), made.instance);
    assert_eq!(removed.modules, vec![ModuleId(2), ModuleId(3), ModuleId(4)]);
    assert_eq!(removed.tiles.len(), 7, "{removed:?}");
    for id in &removed.modules {
        controller.remove_module(*id).unwrap();
    }
    hierarchy.sync(&mut controller).unwrap();
    assert!(hierarchy.is_flat());
    assert!(
        cables(&controller)
            .iter()
            .all(|(from, to)| [1, 5, 9, 10, 11].contains(&from.0) && [1, 5, 9, 10, 11].contains(&to.0))
    );
    assert_eq!(hierarchy.tile(controller.graph(), made.instance), None);
}

/// A restored snapshot keeps the ids handed out since, so no id is ever
/// given out twice (undo.md, rule 5).
#[test]
fn restoring_a_snapshot_keeps_the_ids_handed_out_since() {
    let (controller, mut hierarchy) = load_text(FLAT);
    let snapshot = hierarchy.clone();
    let (_, added) = hierarchy.new_module(&ModulePath::root()).unwrap();
    assert_eq!(added, ModuleId(12));
    hierarchy.restore(&snapshot);
    assert_eq!(hierarchy.ids().next_free(), 13);
    assert_eq!(hierarchy.new_module(&ModulePath::root()).unwrap().1, ModuleId(13));
    assert_eq!(hierarchy.level_of(ModuleId(13)), ModulePath::root());
    assert_eq!(
        hierarchy.tile(controller.graph(), ModuleId(13)),
        None,
        "not in the graph yet"
    );
}

/// At the top level a new module's level id is its flat id; inside, the
/// level id is the definition's next and the flat id the table's.
#[test]
fn new_modules_take_ids_from_the_table() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let inside = hierarchy.path_of(made.instance);
    let (path, id) = hierarchy.new_module(&inside).unwrap();
    assert_eq!(path.parent(), Some(inside.clone()));
    assert_eq!(path.last(), 8, "after the members 2, 3, 4 and the ports 5, 6, 7");
    assert!(id.0 > 11, "{id:?}");
    assert_ne!(path.last(), id.0);
    let (top_path, top_id) = hierarchy.new_module(&ModulePath::root()).unwrap();
    assert_eq!(top_path, ModulePath::top(top_id.0));
    // A port added inside gets a unique name.
    let port = hierarchy.add_port(&inside, Direction::Input, "Pitch").unwrap();
    assert_eq!(
        hierarchy.tile(controller.graph(), port),
        Some(Tile::Port(Direction::Input, "Pitch 2".to_string()))
    );
    assert!(hierarchy.add_port(&ModulePath::root(), Direction::Input, "in").is_err());
}

/// The names along a path, for the breadcrumb, and what each tile is.
#[test]
fn the_breadcrumb_names_and_tiles_follow_the_path() {
    let (controller, hierarchy) = load(&nested_example());
    let filter = hierarchy.path_of(hierarchy.id_at(&ModulePath::top(2), 5).unwrap());
    assert_eq!(filter, ModulePath(vec![2, 5]));
    assert_eq!(
        hierarchy.path_names(&filter),
        vec!["Voice".to_string(), "Filter".to_string()]
    );
    assert_eq!(hierarchy.path_names(&ModulePath::root()), Vec::<String>::new());
    assert_eq!(
        hierarchy.tile(controller.graph(), ModuleId(2)),
        Some(Tile::Instance("voice".to_string()))
    );
    let tiles = hierarchy.structural_tiles(&filter);
    assert_eq!(tiles.len(), 3);
    assert_eq!(tiles[0].1, Tile::Port(Direction::Input, "in".to_string()));
    assert_eq!(tiles[2].1, Tile::Port(Direction::Output, "out".to_string()));
    // The cables the Filter level shows: five, two of them links from the
    // ports and one into the output port.
    let shown = hierarchy.level_cables(controller.graph(), &filter);
    assert_eq!(shown.len(), 5);
    assert_eq!(shown.iter().filter(|c| c.link).count(), 3);
    // Behind the Voice's pitch port stands the phase generator's input;
    // behind its out port, the VCA.
    let (inputs, outputs) = hierarchy.instance_ports(ModuleId(2));
    assert_eq!(inputs.len(), 2);
    assert_eq!(outputs.len(), 2);
    let voice = ModulePath::top(2);
    let behind = hierarchy.inputs_behind(controller.graph(), &ModulePath::root(), &(2, "pitch".to_string()));
    assert_eq!(behind, vec![(hierarchy.id_at(&voice, 3).unwrap(), "pitch".to_string())]);
    let behind = hierarchy.source_behind(controller.graph(), &ModulePath::root(), &(2, "out".to_string()));
    assert_eq!(behind, Some((hierarchy.id_at(&voice, 7).unwrap(), "out".to_string())));
}
