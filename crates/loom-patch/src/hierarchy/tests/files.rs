//! Assignment order, MIDI ranges, and linked definitions.

use super::*;

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
    let mut hierarchy = Hierarchy::from_patch(&patch, controller.graph(), ids, controller.registry()).unwrap();
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

/// The flat cutoff of the nested example, and its parameter index.
fn nested_cutoff(controller: &Controller) -> (ModuleId, usize) {
    let (svf, node) = controller
        .graph()
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
        .unwrap();
    (svf, node.parameter_index("cutoff").unwrap())
}

/// The range MIDI sweeps on the flat parameter is the range the outermost
/// surface control spans: loading and the hierarchy agree, for ranges that
/// overlap and for ranges that do not (sub-patches.md, MIDI follows the
/// range). Unexposing on the Voice leaves the Filter's own range, and
/// unexposing inside the Filter leaves none, after a sync.
#[test]
fn the_midi_limit_follows_the_outermost_surface_range() {
    let limit = |controller: &Controller| {
        let (svf, cutoff) = nested_cutoff(controller);
        controller
            .graph()
            .node(svf)
            .unwrap()
            .limit(cutoff)
            .map(|r| (r.min, r.max))
    };
    let voice = ModulePath::top(2);
    for (range, expected) in [((100.0, 500.0), (100.0, 500.0)), ((1.0, 10.0), (20.0, 20000.0))] {
        let mut file = nested_example();
        file.subpatches.get_mut("voice").unwrap().exposed[0].range = Some(range);
        let (controller, hierarchy) = load(&file);
        let exposed = hierarchy.definition_at(&voice).unwrap().exposed[0].clone();
        assert_eq!(hierarchy.exposed_range(&voice, &exposed), Some(expected));
        assert_eq!(limit(&controller), Some(expected), "{range:?}");
    }

    let mut file = nested_example();
    file.subpatches.get_mut("filter").unwrap().exposed[0].range = Some((200.0, 2000.0));
    file.subpatches.get_mut("voice").unwrap().exposed[0].range = Some((500.0, 1000.0));
    let (mut controller, mut hierarchy) = load(&file);
    assert_eq!(limit(&controller), Some((500.0, 1000.0)));
    assert!(hierarchy.unexpose(&voice, 5, "cutoff"));
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(limit(&controller), Some((200.0, 2000.0)), "the Filter's own range");
    assert!(hierarchy.unexpose(&voice.child(5), 3, "cutoff"));
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(limit(&controller), None, "no surface left");
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
        slot: None,
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
