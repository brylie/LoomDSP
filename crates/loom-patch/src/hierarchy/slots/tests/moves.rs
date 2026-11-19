//! Moving a slot across an editor boundary or copying it.

use super::*;

/// The slot, its held parts, its cables, and the audio all survive
/// moving it into and back out of another sub-patch.
#[test]
fn collapsing_and_dissolving_a_slot_preserves_sound_and_parts() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SLOT_FILE).unwrap());
    let level = ModulePath::top(2);
    let slot = inside(&hierarchy, 9);
    let part = hierarchy.part_of_slot(slot);
    let held = hierarchy.slot(slot).unwrap().held.clone();
    let graph_cables = cables(&controller);
    let original = written(&hierarchy, &controller);
    let before = render(&original);
    assert!(before.iter().any(|&sample| sample != 0.0));
    let collapsed = hierarchy.collapse(controller.graph(), &level, &[slot.0]).unwrap();
    assert_eq!(collapsed.members, [slot]);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(render(&written(&hierarchy, &controller)), before);
    let dissolved = hierarchy.dissolve(controller.graph(), collapsed.instance).unwrap();
    assert_eq!(dissolved.members, [slot]);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(hierarchy.part_of_slot(slot), part);
    assert_eq!(hierarchy.slot(slot).unwrap().held, held);
    assert_eq!(cables(&controller), graph_cables);
    assert_eq!(render(&written(&hierarchy, &controller)), before);
}

/// Renders the patch from reset with no output limiter.
fn render(patch: &Patch) -> Vec<f32> {
    let config = EngineConfig {
        output_safety: loom_core::OutputSafety::Off,
        ..EngineConfig::default()
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    patch.load_into(&mut parts.controller).unwrap();
    let mut left = vec![0.0; 1024];
    let mut right = vec![0.0; 1024];
    for (left, right) in left.chunks_mut(64).zip(right.chunks_mut(64)) {
        parts.engine.process(left, right);
    }
    left
}

/// A copied slot rekeys its held sub-patch, so both slots can swap it in.
#[test]
fn copied_slots_own_their_held_definitions() {
    let patch = Patch::from_ron(SLOT_FILE).unwrap();
    let (mut controller, mut hierarchy) = load(&patch);
    let level = ModulePath::top(2);
    let voice = &patch.subpatches["voice"];
    let get = |id| voice.modules.iter().find(|module| module.id == id).unwrap().clone();
    let definitions = BTreeMap::from([("comb".to_string(), patch.subpatches["comb"].clone())]);
    let copy = hierarchy
        .insert_slot_copy(&mut controller, &level, get(9), Some(get(3)), definitions)
        .unwrap()
        .instance;
    hierarchy.sync(&mut controller).unwrap();
    let held_key = hierarchy
        .slot(copy)
        .unwrap()
        .held
        .keys()
        .find_map(|part| match part {
            PartKey::SubPatch(key) => Some(key.clone()),
            PartKey::Module(_) => None,
        })
        .unwrap();
    assert_ne!(held_key, "comb");
    let saved = written(&hierarchy, &controller);
    assert_eq!(saved.subpatches[&held_key], saved.subpatches["comb"]);
    let (mut controller, mut hierarchy) = load(&saved);
    let copy_id = saved.subpatches["voice"]
        .modules
        .iter()
        .find(|module| {
            module
                .slot
                .as_ref()
                .is_some_and(|slot| slot.held.contains_key(&PartKey::SubPatch(held_key.clone())))
        })
        .unwrap()
        .id;
    let copy = inside(&hierarchy, copy_id);
    let original = inside(&hierarchy, 9);
    let original_part = hierarchy.part_of_slot(original);
    hierarchy
        .swap_to_held_subpatch(&mut controller, &|_| Layout::default(), copy, &held_key)
        .unwrap();
    assert_eq!(hierarchy.part_of_slot(original), original_part);
    let restored = hierarchy
        .swap_to_held_subpatch(&mut controller, &|_| Layout::default(), original, "comb")
        .unwrap();
    assert_eq!(hierarchy.part_of_slot(original), Some(restored.part));
    assert_eq!(
        hierarchy
            .part_of_slot(copy)
            .and_then(|id| hierarchy.definition_of(id))
            .unwrap()
            .metadata
            .name,
        "Comb"
    );
}

/// The stack's running filter copies keep their flat ids and samples
/// while a slot collapses into and dissolves out of a new inner tile.
#[test]
fn moving_a_playing_slot_keeps_every_stack_copy() {
    let patch = Patch::from_ron(&super::stack::stacked_slot(2)).unwrap();
    let config = EngineConfig {
        output_safety: loom_core::OutputSafety::Off,
        ..EngineConfig::default()
    };
    let mut edited = create_engine(loom_modules::registry(), config);
    let mut untouched = create_engine(loom_modules::registry(), config);
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut edited.controller, &mut ids).unwrap();
    patch.load_into(&mut untouched.controller).unwrap();
    let mut hierarchy =
        Hierarchy::from_patch(&patch, edited.controller.graph(), ids, edited.controller.registry()).unwrap();
    let level = ModulePath::top(2);
    let slot = inside(&hierarchy, 9);
    let part = hierarchy.part_of_slot(slot).unwrap();
    let copies = hierarchy.copies_of(part).to_vec();
    assert_eq!(copies.len(), 1);
    let compare = |edited: &mut EngineParts, untouched: &mut EngineParts| {
        let mut audible = false;
        for _ in 0..32 {
            let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
            let (mut expected_left, mut expected_right) = ([0.0; 64], [0.0; 64]);
            edited.engine.process(&mut left, &mut right);
            untouched.engine.process(&mut expected_left, &mut expected_right);
            assert_eq!((left, right), (expected_left, expected_right));
            audible |= left.iter().any(|&value| value != 0.0);
        }
        assert!(audible);
    };
    compare(&mut edited, &mut untouched);
    let wrapper = hierarchy
        .collapse(edited.controller.graph(), &level, &[slot.0])
        .unwrap()
        .instance;
    hierarchy.sync(&mut edited.controller).unwrap();
    edited.controller.commit().unwrap();
    assert_eq!(hierarchy.copies_of(part), copies);
    compare(&mut edited, &mut untouched);
    hierarchy.dissolve(edited.controller.graph(), wrapper).unwrap();
    hierarchy.sync(&mut edited.controller).unwrap();
    edited.controller.commit().unwrap();
    assert_eq!(hierarchy.copies_of(part), copies);
    compare(&mut edited, &mut untouched);
}

/// A slot move refuses a fade in progress before changing either boundary.
#[test]
fn moving_a_fading_slot_requires_the_swap_to_finish() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SLOT_FILE).unwrap());
    let slot = inside(&hierarchy, 9);
    hierarchy
        .swap_to_module(&mut controller, &|_| Layout::default(), slot, "loom.filter.svf@1")
        .unwrap();
    let level = ModulePath::top(2);
    let before = hierarchy.clone();
    assert!(
        hierarchy
            .collapse(controller.graph(), &level, &[slot.0])
            .unwrap_err()
            .contains("Finish the slot swap")
    );
    assert!(
        hierarchy
            .dissolve(controller.graph(), ModuleId(2))
            .unwrap_err()
            .contains("Finish the slot swap")
    );
    assert_eq!(hierarchy, before);
}

/// An empty slot's exposed role name survives a new boundary, but a made-up
/// exposed name still fails when loaded.
#[test]
fn an_empty_slot_can_be_reexposed_and_resolves_when_filled() {
    let mut patch = Patch::from_ron(SLOT_FILE).unwrap();
    let voice = patch.subpatches.get_mut("voice").unwrap();
    voice.modules.retain(|module| module.id != 3);
    voice
        .modules
        .iter_mut()
        .find(|module| module.id == 9)
        .unwrap()
        .slot
        .as_mut()
        .unwrap()
        .part = None;
    let (mut controller, mut hierarchy) = load(&patch);
    let slot = inside(&hierarchy, 9);
    let level = ModulePath::top(2);
    let nested = hierarchy
        .collapse(controller.graph(), &level, &[slot.0])
        .unwrap()
        .instance;
    hierarchy.sync(&mut controller).unwrap();
    assert!(hierarchy.exposed_target(ModuleId(2), 0).is_none());
    let mut invalid = written(&hierarchy, &controller);
    invalid.subpatches.get_mut("voice").unwrap().exposed[0].param = "made up".to_string();
    assert!(invalid.flatten(controller.registry(), &mut FlatIds::new()).is_err());
    let part = hierarchy
        .swap_to_module(&mut controller, &|_| Layout::default(), slot, "loom.filter.onepole@1")
        .unwrap()
        .part;
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(hierarchy.exposed_target(ModuleId(2), 0).unwrap().module, part);
    assert_eq!(hierarchy.exposed_target(nested, 0).unwrap().module, part);
}

/// Inner local id 3 also names Audio Out in the outer level. Only the
/// inner endpoint moves. The top-level range keeps the surface overlap.
#[test]
fn dissolving_a_slot_keeps_outer_ids_and_the_surface_overlap() {
    let mut patch = Patch::from_ron(SLOT_FILE).unwrap();
    patch.subpatches.get_mut("voice").unwrap().exposed[0].range = Some((200.0, 600.0));
    patch.exposed.push(Exposed {
        name: "Frequency".into(),
        module: 2,
        param: "cutoff".into(),
        range: Some((100.0, 400.0)),
    });
    let (mut controller, mut hierarchy) = load(&patch);
    let original_cables = cables(&controller);
    hierarchy.dissolve(controller.graph(), ModuleId(2)).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(cables(&controller), original_cables);
    let patch = written(&hierarchy, &controller);
    assert_eq!(patch.exposed[0].range, Some((200.0, 400.0)));
}

/// A rejected copy count leaves no partial modules or reserved copy paths.
#[test]
fn sync_preflights_room_for_the_whole_stack_growth() {
    let patch = Patch::from_ron(&super::stack::stacked_slot(2)).unwrap();
    let (baseline, _) = load(&patch);
    let max_modules = baseline.config().max_modules - baseline.free_module_slots() + 1;
    let config = EngineConfig {
        max_modules,
        ..EngineConfig::default()
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut parts.controller, &mut ids).unwrap();
    let mut hierarchy =
        Hierarchy::from_patch(&patch, parts.controller.graph(), ids, parts.controller.registry()).unwrap();
    hierarchy.set_copies(ModuleId(2), 4).unwrap();
    let before_ids = hierarchy.ids().clone();
    let before_modules: Vec<_> = parts.controller.graph().nodes().map(|(id, _)| id).collect();
    let before_cables = cables(&parts.controller);
    assert_eq!(parts.controller.free_module_slots(), 1);
    assert!(matches!(
        hierarchy.sync(&mut parts.controller),
        Err(PatchError::Engine(loom_core::LoomError::TooManyModules(_)))
    ));
    assert_eq!(*hierarchy.ids(), before_ids);
    assert_eq!(
        parts.controller.graph().nodes().map(|(id, _)| id).collect::<Vec<_>>(),
        before_modules
    );
    assert_eq!(cables(&parts.controller), before_cables);
    assert_eq!(parts.controller.free_module_slots(), 1);
}

/// When a part containing a slot leaves, its nested slot is retired. The
/// outer slot's stored copy still restores the nested slot and held part.
#[test]
fn a_departing_subpatch_retires_its_nested_slot() {
    let mut patch = Patch::from_ron(SLOT_FILE).unwrap();
    patch
        .subpatches
        .insert("spare".into(), patch.subpatches["comb"].clone());
    let comb = patch.subpatches.get_mut("comb").unwrap();
    comb.modules.push(
        ron::from_str(
            r#"PatchModule(id: 9, kind: "loom.struct.slot@1",
        slot: Some(Slot(role: "loom.role.filter@1", part: Some(3), held: {SubPatch("spare"): Held()})))"#,
        )
        .unwrap(),
    );
    comb.cables[1].to = (9, "in".into());
    comb.cables[2].from = (9, "out".into());
    comb.exposed[0].module = 9;
    let (mut controller, mut hierarchy) = load(&patch);
    let slot = inside(&hierarchy, 9);
    let layout = |_: u32| Layout::default();
    let comb = hierarchy
        .swap_to_held_subpatch(&mut controller, &layout, slot, "comb")
        .unwrap()
        .part;
    hierarchy.sync(&mut controller).unwrap();
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    let nested = hierarchy.id_at(&hierarchy.path_of(comb), 9).unwrap();
    assert!(hierarchy.slot(nested).is_some());
    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.ladder@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    assert!(hierarchy.slot(nested).is_none());
    assert!(hierarchy.held_definition_name("spare").is_none());
    assert!(written(&hierarchy, &controller).subpatches.contains_key("spare"));
    let back = hierarchy
        .swap_to_held_subpatch(&mut controller, &layout, slot, "comb")
        .unwrap()
        .part;
    let nested = hierarchy.id_at(&hierarchy.path_of(back), 9).unwrap();
    assert!(
        hierarchy
            .slot(nested)
            .unwrap()
            .held
            .keys()
            .any(|key| matches!(key, PartKey::SubPatch(_)))
    );
}
