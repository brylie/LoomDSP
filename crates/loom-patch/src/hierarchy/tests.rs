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

/// The live cutoff of the state variable filter in the nested example,
/// wherever it now sits, in a freshly loaded engine.
fn reloaded_cutoff(saved: &Patch) -> f32 {
    let (again, _) = load(saved);
    let graph = again.graph();
    let (id, node) = graph
        .nodes()
        .find(|(_, node)| node.descriptor.id == "loom.filter.svf")
        .expect("the filter is still there");
    let index = node.parameter_index("cutoff").unwrap();
    let _ = id;
    node.params[index]
}

/// Dissolving the Filter inside the Voice, whose definition exposes the
/// Filter's cutoff, keeps that exposure: it now names the filter module
/// itself, the saved file loads, and the top instance's 300 Hz still
/// reaches the filter.
#[test]
fn dissolving_inside_a_sub_patch_keeps_the_outer_exposure() {
    let file = nested_example();
    let (mut controller, mut hierarchy) = load(&file);
    let before = cables_by_path(&controller, &hierarchy);
    let voice = ModulePath::top(2);
    let filter = hierarchy.id_at(&voice, 5).unwrap();
    hierarchy.dissolve(controller.graph(), filter).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(cables(&controller).len(), before.len());
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert_eq!(saved.subpatches.len(), 1);
    let definition = &saved.subpatches["voice"];
    let svf = definition
        .modules
        .iter()
        .find(|m| m.kind == "loom.filter.svf@1")
        .expect("the filter is in the Voice now");
    assert_eq!(definition.exposed.len(), 1);
    assert_eq!(definition.exposed[0].module, svf.id);
    assert_eq!(definition.exposed[0].param, "cutoff");
    assert_eq!(
        definition.exposed[0].range,
        Some((20.0, 20000.0)),
        "the inner range carried up"
    );
    assert_eq!(
        svf.params.get("cutoff"),
        Some(&1000.0),
        "the stored value stays the definition's"
    );
    assert_eq!(reloaded_cutoff(&saved), 300.0);
}

/// Collapsing the Filter and the saw inside the Voice carries the Voice's
/// exposure of the cutoff through the new sub-patch, and the file loads
/// with the same cutoff.
#[test]
fn collapsing_inside_a_sub_patch_keeps_the_outer_exposure() {
    let file = nested_example();
    let (mut controller, mut hierarchy) = load(&file);
    let voice = ModulePath::top(2);
    let filter = hierarchy.id_at(&voice, 5).unwrap();
    let saw = hierarchy.id_at(&voice, 4).unwrap();
    let made = hierarchy
        .collapse(controller.graph(), &voice, &[filter.0, saw.0])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert_eq!(saved.subpatches.len(), 3);
    let voice_def = &saved.subpatches["voice"];
    assert_eq!(voice_def.exposed.len(), 1);
    assert_eq!(voice_def.exposed[0].module, hierarchy.path_of(made.instance).last());
    assert_eq!(voice_def.exposed[0].param, "cutoff");
    let new_def = &saved.subpatches["sub-patch"];
    assert_eq!(new_def.exposed.len(), 1);
    assert_eq!(new_def.exposed[0].name, "cutoff");
    assert_eq!(
        (new_def.exposed[0].module, new_def.exposed[0].param.as_str()),
        (5, "cutoff")
    );
    assert_eq!(reloaded_cutoff(&saved), 300.0);
    // The instance between stores no value of its own.
    let between = voice_def.modules.iter().find(|m| m.is_instance()).unwrap();
    assert!(between.params.is_empty());
}

/// Removing a module whose parameter is exposed takes the exposure with
/// it, and every re-exposure above: the saved file loads again.
#[test]
fn removing_an_exposed_module_drops_its_exposure_at_every_level() {
    let file = nested_example();
    let (mut controller, mut hierarchy) = load(&file);
    let svf = hierarchy.id_at(&ModulePath(vec![2, 5]), 3).unwrap();
    let removed = hierarchy.remove(controller.graph(), svf);
    assert_eq!(removed.modules, vec![svf]);
    hierarchy.sync_without(&mut controller, &removed.modules).unwrap();
    controller.remove_module(svf).unwrap();
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert!(saved.subpatches["filter"].exposed.is_empty());
    assert!(saved.subpatches["voice"].exposed.is_empty());
    let top = saved.modules.iter().find(|m| m.id == 2).unwrap();
    assert!(top.params.is_empty(), "no value for a parameter that is gone");
    let (again, _) = load(&saved);
    assert!(again.graph().nodes().all(|(_, n)| n.descriptor.id != "loom.filter.svf"));
    // Later edits still work.
    hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[1])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
}

/// Removing a connected port takes the cables through it on both sides,
/// and the saved file loads again.
#[test]
fn removing_a_connected_port_drops_the_cables_through_it() {
    let file = nested_example();
    let (mut controller, mut hierarchy) = load(&file);
    let voice = ModulePath::top(2);
    let pitch = hierarchy.id_at(&voice, 1).unwrap();
    let removed = hierarchy.remove(controller.graph(), pitch);
    assert!(removed.modules.is_empty());
    hierarchy.sync(&mut controller).unwrap();
    let phase = hierarchy.id_at(&voice, 3).unwrap();
    assert!(
        !cables(&controller)
            .iter()
            .any(|(from, to)| from.0 == 1 && to.0 == phase.0),
        "the note no longer reaches the phase generator"
    );
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert!(!saved.cables.iter().any(|c| c.to == (2, "pitch".to_string())));
    let (inputs, _) = hierarchy.instance_ports(ModuleId(2));
    assert_eq!(inputs.len(), 1);
    load(&saved);
}

/// Two assignments onto one inner parameter expose it once, and both
/// saved assignments target that one name.
#[test]
fn two_assignments_to_one_parameter_expose_it_once() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let cutoff = controller
        .graph()
        .node(ModuleId(4))
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    for source in [1, 5] {
        controller
            .add_assignment(
                OutputRef {
                    module: ModuleId(source),
                    port: 0,
                },
                ModuleId(4),
                cutoff,
                0.5,
            )
            .unwrap();
    }
    controller.commit().unwrap();
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let definition = hierarchy.definition_of(made.instance).unwrap();
    assert_eq!(definition.exposed.len(), 1);
    let saved = written(&hierarchy, &controller, &Metadata::default());
    assert_eq!(saved.modulation.len(), 2);
    assert!(
        saved
            .modulation
            .iter()
            .all(|a| a.target == (made.instance.0, "Cutoff".to_string()))
    );
    let (again, _) = load(&saved);
    assert_eq!(again.graph().assignments().count(), 2);
}

/// Expose, as the inspector does it: the envelope's attack inside the
/// Voice gets a surface name from its label, its own value is stored for
/// the file, the instance carries the live value, and the saved file
/// loads with the new surface entry. Unexpose takes it off again and the
/// file is back to what was loaded.
#[test]
fn exposing_and_unexposing_a_parameter_round_trip_through_the_file() {
    let file = nested_example();
    let (mut controller, mut hierarchy) = load(&file);
    let voice = ModulePath::top(2);
    assert_eq!(
        hierarchy.expose(controller.graph(), &ModulePath::root(), 2, "cutoff"),
        Err(EXPOSE_INSIDE_ONLY.to_string())
    );
    assert!(hierarchy.expose(controller.graph(), &voice, 6, "nothing").is_err());
    assert!(
        hierarchy.expose(controller.graph(), &voice, 1, "out").is_err(),
        "a port"
    );
    let name = hierarchy.expose(controller.graph(), &voice, 6, "attack").unwrap();
    assert_eq!(name, "Attack");
    // Exposing it again changes nothing.
    assert_eq!(
        hierarchy.expose(controller.graph(), &voice, 6, "attack").unwrap(),
        "Attack"
    );
    let definition = hierarchy.definition_at(&voice).unwrap();
    assert_eq!(definition.exposed.len(), 2);
    assert_eq!(definition.defaults[&(6, "attack".to_string())].value, Some(0.005));
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &file.metadata);
    let instance = saved.modules.iter().find(|m| m.id == 2).unwrap();
    assert_eq!(instance.params.get("Attack"), Some(&0.005));
    let envelope = saved.subpatches["voice"].modules.iter().find(|m| m.id == 6).unwrap();
    assert!(envelope.ranges.is_empty(), "a default control range is not written");
    assert!(
        saved.subpatches["voice"]
            .exposed
            .iter()
            .any(|e| e.name == "Attack" && e.module == 6 && e.param == "attack")
    );
    let (_, again) = load(&saved);
    assert_eq!(
        again.exposed_target(ModuleId(2), 1).map(|t| t.param),
        Some("attack".to_string())
    );

    assert!(hierarchy.unexpose(&voice, 6, "attack"));
    assert!(!hierarchy.unexpose(&voice, 6, "attack"));
    let definition = hierarchy.definition_at(&voice).unwrap();
    assert_eq!(definition.exposed.len(), 1);
    assert!(!definition.defaults.contains_key(&(6, "attack".to_string())));
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(text(&written(&hierarchy, &controller, &file.metadata)), text(&file));
}

/// Unexposing on the Voice takes the filter's cutoff off the Voice's
/// surface and leaves the Filter's own exposure; the inner instance then
/// carries the live value, so the sound stays. Unexposing inside the
/// Filter takes both off, since the Voice's entry had nothing left to
/// stand for, and the filter module keeps the live value.
#[test]
fn unexposing_at_one_level_leaves_the_exposures_below() {
    let file = nested_example();
    let voice = ModulePath::top(2);
    let filter = voice.child(5);

    let (mut controller, mut hierarchy) = load(&file);
    assert!(
        !hierarchy.unexpose(&ModulePath::root(), 2, "cutoff"),
        "the top has no surface"
    );
    assert!(hierarchy.unexpose(&voice, 5, "cutoff"));
    assert!(hierarchy.definition_at(&voice).unwrap().exposed.is_empty());
    assert_eq!(hierarchy.definition_at(&filter).unwrap().exposed.len(), 1);
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert!(saved.modules.iter().find(|m| m.id == 2).unwrap().params.is_empty());
    let inner_instance = saved.subpatches["voice"].modules.iter().find(|m| m.id == 5).unwrap();
    assert_eq!(inner_instance.params.get("cutoff"), Some(&300.0));
    assert_eq!(reloaded_cutoff(&saved), 300.0);

    let (mut controller, mut hierarchy) = load(&file);
    assert!(hierarchy.unexpose(&filter, 3, "cutoff"));
    assert!(hierarchy.definition_at(&voice).unwrap().exposed.is_empty());
    assert!(hierarchy.definition_at(&filter).unwrap().exposed.is_empty());
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &file.metadata);
    let svf = saved.subpatches["filter"].modules.iter().find(|m| m.id == 3).unwrap();
    assert_eq!(svf.params.get("cutoff"), Some(&300.0));
    assert_eq!(reloaded_cutoff(&saved), 300.0);
}

/// What a surface control stands for, and the way back: the Voice's one
/// surface entry is the filter's cutoff two levels down, with the range
/// the Filter gives it; the filter's cutoff is exposed on the Filter tile,
/// and that on the Voice tile; and from the top level the filter's cutoff
/// is reached through the Voice tile's first control.
#[test]
fn surface_lookups_resolve_through_levels() {
    let (_, hierarchy) = load(&nested_example());
    let voice = ModulePath::top(2);
    let filter_tile = hierarchy.id_at(&voice, 5).unwrap();
    let svf = hierarchy.id_at(&voice.child(5), 3).unwrap();
    let target = hierarchy.exposed_target(ModuleId(2), 0).unwrap();
    assert_eq!(
        (target.module, target.param.as_str(), target.range),
        (svf, "cutoff", Some((20.0, 20000.0)))
    );
    assert!(hierarchy.exposed_target(ModuleId(2), 1).is_none());
    assert!(hierarchy.exposed_target(svf, 0).is_none(), "a module has no surface");
    assert_eq!(
        hierarchy.exposure_of(svf, "cutoff"),
        Some((filter_tile, "cutoff".to_string()))
    );
    assert_eq!(
        hierarchy.exposure_of(filter_tile, "cutoff"),
        Some((ModuleId(2), "cutoff".to_string()))
    );
    assert_eq!(hierarchy.exposure_of(svf, "resonance"), None);
    assert_eq!(
        hierarchy.exposure_of(ModuleId(2), "cutoff"),
        None,
        "the top has no surface"
    );
    let cutoff = (svf, "cutoff".to_string());
    assert_eq!(
        hierarchy.surface_for(&ModulePath::root(), &cutoff),
        Some((ModuleId(2), 0))
    );
    assert_eq!(hierarchy.surface_for(&voice, &cutoff), Some((filter_tile, 0)));
    assert_eq!(hierarchy.surface_for(&voice.child(5), &cutoff), None, "its own level");
    assert_eq!(
        hierarchy.surface_for(&ModulePath::root(), &(svf, "resonance".to_string())),
        None
    );
}

/// Ranges through levels: the surface range is the overlap of each
/// level's range, and the value is clamped to each in turn, which gives
/// the same number. Ranges that do not overlap give the innermost one.
#[test]
fn exposed_ranges_overlap_outer_then_inner() {
    let mut file = nested_example();
    file.subpatches.get_mut("voice").unwrap().exposed[0].range = Some((100.0, 500.0));
    file.modules
        .iter_mut()
        .find(|m| m.id == 2)
        .unwrap()
        .params
        .insert("cutoff".into(), 5000.0);
    let (controller, hierarchy) = load(&file);
    let voice = ModulePath::top(2);
    // The Voice's entry: its own range, then the Filter's behind it.
    let exposed = hierarchy.definition_at(&voice).unwrap().exposed[0].clone();
    assert_eq!(hierarchy.exposed_range(&voice, &exposed), Some((100.0, 500.0)));
    assert_eq!(
        hierarchy.exposed_value(controller.graph(), &voice, &exposed),
        Some(500.0)
    );
    assert_eq!(
        hierarchy.exposed_target(ModuleId(2), 0).unwrap().range,
        Some((100.0, 500.0))
    );
    // The Filter's own entry is only its own range.
    let inner = hierarchy.definition_at(&voice.child(5)).unwrap().exposed[0].clone();
    assert_eq!(hierarchy.exposed_range(&voice.child(5), &inner), Some((20.0, 20000.0)));

    file.subpatches.get_mut("voice").unwrap().exposed[0].range = Some((1.0, 10.0));
    file.modules
        .iter_mut()
        .find(|m| m.id == 2)
        .unwrap()
        .params
        .insert("cutoff".into(), 5.0);
    let (_, hierarchy) = load(&file);
    let exposed = hierarchy.definition_at(&voice).unwrap().exposed[0].clone();
    assert_eq!(
        hierarchy.exposed_range(&voice, &exposed),
        Some((20.0, 20000.0)),
        "disjoint ranges: the inner clamp is applied last"
    );
    assert_eq!(
        hierarchy.exposed_target(ModuleId(2), 0).unwrap().range,
        Some((20.0, 20000.0))
    );
}

/// Renaming a sub-patch changes what the tile and the file show, and keeps
/// the definition's key, which the player never sees.
#[test]
fn renaming_a_sub_patch_changes_its_name_and_keeps_its_key() {
    let (controller, mut hierarchy) = load(&nested_example());
    hierarchy.rename(ModuleId(2), " Lead voice ").unwrap();
    assert_eq!(hierarchy.tile_name(ModuleId(2)), Some("Lead voice".to_string()));
    assert!(hierarchy.rename(ModuleId(2), "  ").is_err());
    assert!(
        hierarchy.rename(ModuleId(1), "Note").is_err(),
        "a module is not a sub-patch"
    );
    let saved = written(&hierarchy, &controller, &Metadata::default());
    assert_eq!(saved.subpatches["voice"].metadata.name, "Lead voice");
    assert!(saved.modules.iter().any(|m| m.subpatch.as_deref() == Some("voice")));
}

/// Assignments on one parameter from both sides of a boundary sum in the
/// order the file loads them, inner first, whatever order they were made
/// in: the sync moves the outer ones behind the inner one, giving new ids
/// only to those, keeps their options, moves nothing on a second pass, and
/// the saved file reloads in the same order (Editor model, Assignment
/// order is kept canonical).
#[test]
fn sync_puts_assignments_in_file_order_and_renumbers_only_the_movers() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let inside = ModulePath::top(made.instance.0);
    let svf = hierarchy.id_at(&inside, 4).unwrap();
    let saw = hierarchy.id_at(&inside, 3).unwrap();
    hierarchy.expose(controller.graph(), &inside, 4, "cutoff").unwrap();
    let cutoff = controller.graph().node(svf).unwrap().parameter_index("cutoff").unwrap();
    let out = |module: ModuleId| OutputRef { module, port: 0 };
    // Outside first: the LFO, then the note. Then inside: the saw.
    let lfo = controller.add_assignment(out(ModuleId(5)), svf, cutoff, 0.1).unwrap();
    let note = controller.add_assignment(out(ModuleId(1)), svf, cutoff, 0.3).unwrap();
    controller.set_assignment_enabled(note, false).unwrap();
    let inner = controller.add_assignment(out(saw), svf, cutoff, 0.7).unwrap();
    controller
        .set_assignment_polarity(inner, Some(loom_core::Polarity::Unipolar))
        .unwrap();
    assert!(lfo < note && note < inner);

    hierarchy.sync(&mut controller).unwrap();
    let live: Vec<(AssignmentId, OutputRef, f32)> = controller
        .graph()
        .assignments()
        .map(|(id, a)| (id, a.source, a.depth))
        .collect();
    assert_eq!(live.len(), 3);
    assert_eq!(live[0], (inner, out(saw), 0.7), "the inner assignment keeps its id");
    assert_eq!((live[1].1, live[1].2), (out(ModuleId(5)), 0.1));
    assert_eq!((live[2].1, live[2].2), (out(ModuleId(1)), 0.3));
    assert!(
        live[1].0 > inner && live[2].0 > live[1].0,
        "the outer ones moved behind it"
    );
    let graph = controller.graph();
    assert!(
        !graph.assignment(live[2].0).unwrap().enabled,
        "options travel with a moved assignment"
    );
    assert_eq!(
        graph.assignment(inner).unwrap().polarity,
        Some(loom_core::Polarity::Unipolar)
    );
    let ids: Vec<AssignmentId> = live.iter().map(|(id, _, _)| *id).collect();
    hierarchy.sync(&mut controller).unwrap();
    let again: Vec<AssignmentId> = controller.graph().assignments().map(|(id, _)| id).collect();
    assert_eq!(again, ids, "a second sync moves nothing");

    let saved = written(&hierarchy, &controller, &Metadata::default());
    let (reloaded, reloaded_h) = load(&saved);
    let order: Vec<(ModulePath, f32)> = reloaded
        .graph()
        .assignments()
        .map(|(_, a)| (reloaded_h.path_of(a.source.module), a.depth))
        .collect();
    assert_eq!(
        order,
        [
            (inside.child(3), 0.7),
            (ModulePath::top(5), 0.1),
            (ModulePath::top(1), 0.3)
        ]
    );
}

/// Two assignments from one source onto one parameter, which only a
/// hand-written file makes, are matched to the file's entries in id order:
/// the first sync settles the order behind the inner assignment and reports
/// the ids it changed, and a second sync changes nothing.
#[test]
fn duplicate_assignments_settle_in_one_sync() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let inside = ModulePath::top(made.instance.0);
    let svf = hierarchy.id_at(&inside, 4).unwrap();
    let saw = hierarchy.id_at(&inside, 3).unwrap();
    hierarchy.expose(controller.graph(), &inside, 4, "cutoff").unwrap();
    let cutoff = controller.graph().node(svf).unwrap().parameter_index("cutoff").unwrap();
    let out = |module: ModuleId| OutputRef { module, port: 0 };
    let first = controller.add_assignment(out(ModuleId(5)), svf, cutoff, 0.1).unwrap();
    let second = controller.add_assignment(out(ModuleId(5)), svf, cutoff, 0.2).unwrap();
    let inner = controller.add_assignment(out(saw), svf, cutoff, 0.3).unwrap();

    let renamed = hierarchy.sync(&mut controller).unwrap();
    assert_eq!(renamed.iter().map(|(old, _)| *old).collect::<Vec<_>>(), [first, second]);
    let ids: Vec<AssignmentId> = controller.graph().assignments().map(|(id, _)| id).collect();
    assert_eq!(ids[0], inner);
    let depths: Vec<f32> = controller.graph().assignments().map(|(_, a)| a.depth).collect();
    assert_eq!(depths, [0.3, 0.1, 0.2], "the duplicates keep their order");
    assert!(
        hierarchy.sync(&mut controller).unwrap().is_empty(),
        "a settled order stays settled"
    );
    let again: Vec<AssignmentId> = controller.graph().assignments().map(|(id, _)| id).collect();
    assert_eq!(again, ids);
}

/// The order sync changes only the graph, so it works with a full command
/// queue, and a moved assignment keeps its depth, polarity, and state. The
/// commit that follows can fail and be retried without a half-done reorder,
/// which the matrix retarget relies on.
#[test]
fn the_order_sync_survives_a_full_command_queue() {
    let config = EngineConfig {
        command_capacity: 64,
        ..EngineConfig::default()
    };
    let EngineParts { mut controller, .. } = create_engine(loom_modules::registry(), config);
    let patch = Patch::from_ron(FLAT).unwrap();
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).unwrap();
    let mut hierarchy = Hierarchy::from_patch(&patch, controller.graph(), ids).unwrap();
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[2, 3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let inside = ModulePath::top(made.instance.0);
    let svf = hierarchy.id_at(&inside, 4).unwrap();
    let saw = hierarchy.id_at(&inside, 3).unwrap();
    hierarchy.expose(controller.graph(), &inside, 4, "cutoff").unwrap();
    let cutoff = controller.graph().node(svf).unwrap().parameter_index("cutoff").unwrap();
    let out = |module: ModuleId| OutputRef { module, port: 0 };
    let outer = controller.add_assignment(out(ModuleId(5)), svf, cutoff, 0.4).unwrap();
    controller
        .set_assignment_polarity(outer, Some(loom_core::Polarity::Unipolar))
        .unwrap();
    controller.set_assignment_enabled(outer, false).unwrap();
    let inner = controller.add_assignment(out(saw), svf, cutoff, 0.7).unwrap();

    // Fill the queue, as a burst of knob turns with no audio callback does.
    let filled = (0..200).any(|i| controller.set_param(ModuleId(5), 0, i as f32 * 0.01).is_err());
    assert!(filled, "the command queue is full");

    let renamed = hierarchy.sync(&mut controller).unwrap();
    assert_eq!(renamed.len(), 1);
    assert_eq!(renamed[0].0, outer);
    let moved = controller.graph().assignment(renamed[0].1).unwrap();
    assert_eq!(
        (moved.depth, moved.polarity, moved.enabled),
        (0.4, Some(loom_core::Polarity::Unipolar), false)
    );
    let ids: Vec<AssignmentId> = controller.graph().assignments().map(|(id, _)| id).collect();
    assert_eq!(ids, [inner, renamed[0].1]);
    assert!(matches!(controller.commit(), Err(loom_core::LoomError::QueueFull)));
}

/// A library item: a pitch port into a sine through a multiply whose gain
/// is exposed, out through a port (sub-patches.md, Library).
const VOICE_FILE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Voice", authors: ["Ada"]),
    modules: [
        PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch"), position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.phase.generator@1", position: Some((200.0, 0.0))),
        PatchModule(id: 3, kind: "loom.osc.sine@1", position: Some((400.0, 0.0))),
        PatchModule(id: 4, kind: "loom.math.mul@1", params: {"gain": 0.5}, position: Some((600.0, 0.0))),
        PatchModule(id: 5, kind: "loom.struct.output@1", label: Some("out"), position: Some((800.0, 0.0))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "a")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
    exposed: [Exposed(name: "gain", module: 4, param: "gain")],
)"#;

/// Inserts [`VOICE_FILE`] at the top of [`FLAT`], as a copy or linked to
/// `Voices/Voice.ron`, and syncs.
fn insert_voice(linked: bool) -> (Controller, Hierarchy, Inserted) {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let file = Patch::from_ron(VOICE_FILE).unwrap();
    let inserted = hierarchy
        .insert(
            &mut controller,
            &ModulePath::root(),
            &file,
            linked.then_some("Voices/Voice.ron"),
        )
        .expect("the file inserts");
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    (controller, hierarchy, inserted)
}

/// Inserting a patch file as a copy makes a tile with the file's ports and
/// surface, adds its modules with ids from the table, places its tiles as
/// the file did, and saves as a definition that follows no file. Cabled
/// in, it plays as the file would.
#[test]
fn inserting_a_file_as_a_copy_makes_a_tile_with_its_ports() {
    let (mut controller, mut hierarchy, inserted) = insert_voice(false);
    assert_eq!(inserted.name, "Voice");
    assert!(inserted.left_out.is_empty());
    assert!(inserted.notes.is_empty());
    assert_eq!(
        inserted.modules.len(),
        3,
        "the phase generator, the sine, and the multiply"
    );
    assert_eq!(hierarchy.tile_name(inserted.instance).as_deref(), Some("Voice"));
    assert_eq!(hierarchy.library_of(inserted.instance), None);
    let (inputs, outputs) = hierarchy.instance_ports(inserted.instance);
    assert_eq!(inputs.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(), ["pitch"]);
    assert_eq!(outputs.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(), ["out"]);
    let inside = hierarchy.path_of(inserted.instance);
    for id in &inserted.modules {
        assert_eq!(hierarchy.level_of(*id), inside);
        assert!(controller.graph().node(*id).is_some());
    }
    // Every inner tile, ports included, has the file's position.
    assert_eq!(inserted.layouts.len(), 5);
    let mul = hierarchy.id_at(&inside, 4).unwrap();
    let placed = inserted.layouts.iter().find(|(id, _)| *id == mul).unwrap();
    assert_eq!(placed.1.position, Some((600.0, 0.0)));
    // The surface item reaches the inner gain.
    let target = hierarchy.exposed_target(inserted.instance, 0).unwrap();
    assert_eq!((target.module, target.param.as_str()), (mul, "gain"));

    // Cabled between the note and the output, the file loads back to it.
    let lid = inside.last();
    hierarchy.connect(
        &ModulePath::root(),
        (1, "pitch".to_string()),
        (lid, "pitch".to_string()),
    );
    hierarchy.connect(&ModulePath::root(), (lid, "out".to_string()), (9, "in".to_string()));
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &Metadata::default());
    let definition = &saved.subpatches["voice"];
    assert_eq!(definition.library, None);
    assert_eq!(definition.metadata.authors, ["Ada"], "the file's credit travels");
    assert_eq!(definition.exposed.len(), 1);
    let (again, again_hierarchy) = load(&saved);
    assert_eq!(
        cables_by_path(&again, &again_hierarchy),
        cables_by_path(&controller, &hierarchy)
    );
}

/// Two linked instances of one file have separate definitions, each
/// following the file; Unlink on one leaves the other linked.
#[test]
fn two_linked_instances_have_separate_definitions_and_unlink_apart() {
    let (mut controller, mut hierarchy, first) = insert_voice(true);
    let file = Patch::from_ron(VOICE_FILE).unwrap();
    let second = hierarchy
        .insert(&mut controller, &ModulePath::root(), &file, Some("Voices/Voice.ron"))
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_ne!(first.instance, second.instance);
    assert_eq!(hierarchy.library_of(first.instance), Some("Voices/Voice.ron"));
    assert_eq!(hierarchy.library_of(second.instance), Some("Voices/Voice.ron"));
    assert!(hierarchy.has_links());
    let saved = written(&hierarchy, &controller, &Metadata::default());
    assert_eq!(saved.subpatches.len(), 2);
    assert!(saved.subpatches.contains_key("voice"));
    assert!(saved.subpatches.contains_key("voice 2"));
    assert!(
        saved
            .subpatches
            .values()
            .all(|d| d.library.as_deref() == Some("Voices/Voice.ron"))
    );
    assert!(saved.has_links());
    let instances: Vec<&PatchModule> = saved.modules.iter().filter(|m| m.is_instance()).collect();
    assert_eq!(instances.len(), 2);
    assert_ne!(instances[0].subpatch, instances[1].subpatch);

    assert_eq!(hierarchy.unlink(first.instance).unwrap(), "Voice");
    assert_eq!(hierarchy.library_of(first.instance), None);
    assert_eq!(hierarchy.library_of(second.instance), Some("Voices/Voice.ron"));
    assert_eq!(
        hierarchy.unlink(first.instance).unwrap_err(),
        "Voice does not follow the library"
    );
    assert_eq!(
        hierarchy.unlink(ModuleId(1)).unwrap_err(),
        "select a sub-patch to unlink"
    );
    let saved = written(&hierarchy, &controller, &Metadata::default());
    assert_eq!(saved.subpatches["voice"].library, None);
    assert_eq!(saved.subpatches["voice 2"].library.as_deref(), Some("Voices/Voice.ron"));
    load(&saved);
}

/// Inserting a file linked inside a sub-patch that follows the same file
/// is refused, naming the loop: the file would contain itself (Patch
/// file). A copy of it inside is fine.
#[test]
fn inserting_a_link_inside_a_sub_patch_linked_to_the_same_file_is_refused() {
    let (mut controller, mut hierarchy, outer) = insert_voice(true);
    let inside = hierarchy.path_of(outer.instance);
    let file = Patch::from_ron(VOICE_FILE).unwrap();
    let before = controller.graph().nodes().count();
    let error = hierarchy
        .insert(&mut controller, &inside, &file, Some("Voices/Voice.ron"))
        .unwrap_err();
    assert_eq!(error.to_string(), "sub-patch Voice › Voice contains itself");
    assert_eq!(controller.graph().nodes().count(), before, "nothing was added");
    let copy = hierarchy.insert(&mut controller, &inside, &file, None).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(hierarchy.level_of(copy.instance), inside);
    load(&written(&hierarchy, &controller, &Metadata::default()));
}

/// A sub-patch written as a library file opens on its own: its ports are
/// ordinary modules at the top, its surface is kept, and inserted again it
/// gives the same definition.
#[test]
fn a_sub_patch_written_as_a_file_opens_on_its_own_and_inserts_back_the_same() {
    let (controller, hierarchy, inserted) = insert_voice(false);
    let file = hierarchy
        .definition_file(controller.graph(), |_| Layout::default(), inserted.instance)
        .expect("a sub-patch tile has a file");
    assert_eq!(file.metadata.name, "Voice");
    assert_eq!(file.metadata.authors, ["Ada"]);
    assert!(file.subpatches.is_empty());
    assert_eq!(file.exposed.len(), 1);
    assert_eq!(file.modules.iter().filter(|m| m.kind == INPUT_KIND).count(), 1);
    let written_text = text(&file);
    // The file opens as a patch of its own, with a Transport added.
    let (alone, alone_hierarchy) = load(&file);
    assert!(alone_hierarchy.is_flat());
    assert_eq!(alone.graph().nodes().count(), 6, "five modules and the Transport");
    // Saved from that session, the surface comes back unchanged.
    let mut saved = alone_hierarchy.unflatten(alone.graph(), |_| Layout::default());
    saved.metadata = file.metadata.clone();
    saved.modules.retain(|m| m.kind != "loom.clock.transport@1");
    assert_eq!(text(&saved), written_text);
    // Inserted again, it is the definition it came from.
    let (fresh, _) = SubPatch::from_patch(&file, controller.registry(), None);
    let stored = written(&hierarchy, &controller, &Metadata::default()).subpatches["voice"].clone();
    assert_eq!(text(&Patch::from_definition(&fresh, &BTreeMap::new())), written_text);
    assert_eq!(fresh.modules, stored.modules);
    assert_eq!(fresh.exposed, stored.exposed);
    assert_eq!(
        hierarchy.definition_file(controller.graph(), |_| Layout::default(), ModuleId(1)),
        None
    );
    assert_eq!(hierarchy.nested_keys(controller.graph(), "voice"), Vec::<String>::new());
}

/// What a linked definition would save changes with an edit inside (a
/// value, a module, a move) and not with an exposed value, which belongs
/// to the instance (sub-patches.md, Copies and editing).
#[test]
fn linked_fingerprints_change_with_an_edit_inside_but_not_with_an_exposed_value() {
    let (mut controller, mut hierarchy, inserted) = insert_voice(true);
    let layout = |_| Layout::default();
    let before = hierarchy.linked_fingerprints(controller.graph(), layout);
    assert_eq!(before.len(), 1);
    assert!(before.contains_key("voice"));
    // The exposed gain is the instance's value.
    let target = hierarchy.exposed_target(inserted.instance, 0).unwrap();
    controller.set_param_named(target.module, &target.param, 0.25).unwrap();
    assert_eq!(hierarchy.linked_fingerprints(controller.graph(), layout), before);
    // An inner value that is not exposed is an edit inside.
    let inside = hierarchy.path_of(inserted.instance);
    let generator = hierarchy.id_at(&inside, 2).unwrap();
    controller.set_param_named(generator, "tune", 0.1).unwrap();
    let after_value = hierarchy.linked_fingerprints(controller.graph(), layout);
    assert_ne!(after_value, before);
    // So is a move: the file keeps positions.
    let moved = |id: u32| Layout {
        position: (id == generator.0).then_some((10.0, 10.0)),
        size: None,
    };
    assert_ne!(hierarchy.linked_fingerprints(controller.graph(), moved), after_value);
    // And a module added inside.
    let (_, added) = hierarchy.new_module(&inside).unwrap();
    controller.add_module_with_id(added, "loom.osc.saw").unwrap();
    assert_ne!(hierarchy.linked_fingerprints(controller.graph(), layout), after_value);
    // Unlinked, there is nothing to compare.
    assert_eq!(hierarchy.unlink_key("voice").as_deref(), Some("Voice"));
    assert_eq!(hierarchy.unlink_key("voice"), None);
    assert!(hierarchy.linked_fingerprints(controller.graph(), layout).is_empty());
    assert!(!hierarchy.has_links());
}

/// A library file opened on its own keeps its surface: the top level's
/// exposed list saves back as it was, follows a module collapsed into a
/// sub-patch and dissolved out again, and loses an entry whose module is
/// gone.
#[test]
fn the_top_level_exposed_list_round_trips_and_drops_gone_modules() {
    let file = Patch::from_ron(VOICE_FILE).unwrap();
    let (mut controller, mut hierarchy) = load(&file);
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert_eq!(saved.exposed, file.exposed);
    // The multiply collapses into a sub-patch with the sine: the entry
    // goes through the new tile's surface, and the module's flat id, which
    // it keeps inside, no longer names it at the top.
    let made = hierarchy
        .collapse(controller.graph(), &ModulePath::root(), &[3, 4])
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &file.metadata);
    let lid = hierarchy.path_of(made.instance).last();
    assert_eq!(saved.exposed.len(), 1);
    // The new surface names the item by the parameter's label, as Expose
    // does; the top entry keeps its own name and points at that item.
    assert_eq!(
        (saved.exposed[0].module, saved.exposed[0].param.as_str()),
        (lid, "Gain")
    );
    assert_eq!(saved.exposed[0].name, "gain");
    let inner = &saved.subpatches["sub-patch"];
    assert_eq!(inner.exposed.len(), 1);
    assert_eq!(inner.exposed[0].name, "Gain");
    assert_eq!((inner.exposed[0].module, inner.exposed[0].param.as_str()), (4, "gain"));
    load(&saved);
    // Dissolved, the entry names the multiply again.
    hierarchy.dissolve(controller.graph(), made.instance).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert_eq!(saved.exposed, file.exposed);
    // A module the list names is removed: its entry goes.
    let removed = hierarchy.remove(controller.graph(), ModuleId(4));
    hierarchy.sync_without(&mut controller, &removed.modules).unwrap();
    controller.remove_module(ModuleId(4)).unwrap();
    let saved = written(&hierarchy, &controller, &file.metadata);
    assert!(saved.exposed.is_empty());
}

/// Inserting a file that has a Transport leaves it out, makes a port of
/// its cable, and reports it; the patch keeps its one Transport.
#[test]
fn inserting_a_file_with_a_transport_leaves_it_out() {
    let (mut controller, mut hierarchy) = load_text(FLAT);
    let mut file = Patch::from_ron(VOICE_FILE).unwrap();
    file.modules.push(PatchModule {
        id: 9,
        kind: "loom.clock.transport@1".to_string(),
        subpatch: None,
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    file.cables.push(PatchCable {
        from: (9, "beat".to_string()),
        to: (2, "reset".to_string()),
    });
    let inserted = hierarchy
        .insert(&mut controller, &ModulePath::root(), &file, None)
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(inserted.left_out.len(), 1);
    assert_eq!(
        inserted.left_out[0].warning(&inserted.name).to_string(),
        "Voice: its Transport was left out; connect its clock ports."
    );
    let (inputs, _) = hierarchy.instance_ports(inserted.instance);
    assert_eq!(inputs.len(), 2);
    assert_eq!(
        controller
            .graph()
            .nodes()
            .filter(|(_, n)| n.descriptor.id == TRANSPORT_MODULE)
            .count(),
        1
    );
}
