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

/// A slot file loads with the part hidden behind the slot tile, the
/// cables routed to the part's own ports, the exposure through the slot,
/// and saves back to the same text (sub-patches.md, Patch file).
#[test]
fn a_slot_file_loads_hidden_part_and_routes_through_it_and_round_trips() {
    let patch = Patch::from_ron(SLOT_FILE).unwrap();
    let (controller, hierarchy) = load(&patch);
    let slot = inside(&hierarchy, 9);
    let part = inside(&hierarchy, 3);
    assert_eq!(
        hierarchy.tile(controller.graph(), slot),
        Some(Tile::Slot("loom.role.filter@1".into()))
    );
    assert_eq!(hierarchy.tile(controller.graph(), part), None, "the part is hidden");
    assert!(hierarchy.is_hidden(part));
    assert_eq!(hierarchy.tile_name(slot), Some("Filter".to_string()));
    assert_eq!(hierarchy.part_of_slot(slot), Some(part));
    assert_eq!(hierarchy.slot_of_part(part), Some(slot));
    let state = hierarchy.slot(slot).unwrap();
    assert_eq!(state.role.id, "loom.role.filter");
    assert_eq!(state.held.len(), 2);
    // Noise → ladder in; ladder out → audio out.
    let noise = ModuleId(1);
    assert!(cables(&controller).contains(&((noise.0, "out".into()), (part.0, "in".into()))));
    assert!(cables(&controller).contains(&((part.0, "out".into()), (3, "in".into()))));
    // The exposed cutoff reaches the ladder's cutoff at the instance value.
    let target = hierarchy.exposed_target(ModuleId(2), 0).unwrap();
    assert_eq!((target.module, target.param.as_str()), (part, "cutoff"));
    let node = controller.graph().node(part).unwrap();
    assert_eq!(node.params[node.parameter_index("cutoff").unwrap()], 1200.0);
    assert_eq!(node.params[node.parameter_index("drive").unwrap()], 0.3);
    // The level shows the two links and nothing of the part's own cables.
    let shown = hierarchy.level_cables(controller.graph(), &ModulePath::top(2));
    assert_eq!(shown.len(), 2);
    assert!(shown.iter().all(|c| c.link));
    // Back to the file.
    let saved = written(&hierarchy, &controller);
    assert_eq!(saved.to_ron().unwrap(), patch.to_ron().unwrap());
    // The held sub-patch is written whole.
    assert_eq!(saved.subpatches["comb"].modules.len(), 4);
}

/// A file whose part does not fill the role, or cables the part directly,
/// is refused naming the slot (sub-patches.md, Patch file).
#[test]
fn bad_slot_files_are_refused_naming_the_slot() {
    let bad_part = SLOT_FILE.replace("loom.filter.ladder@1", "loom.osc.sine@1");
    let bad_part = bad_part.replace(r#"params: {"cutoff": 800.0, "drive": 0.3}"#, "");
    let error = Patch::from_ron(&bad_part)
        .unwrap()
        .flatten(&loom_modules::registry(), &mut FlatIds::new())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("slot 9: its part, module 3, does not fill the role loom.role.filter@1"),
        "{error}"
    );

    let direct = SLOT_FILE.replace(
        r#"PatchCable(from: (1, "out"), to: (9, "in"))"#,
        r#"PatchCable(from: (1, "out"), to: (3, "in"))"#,
    );
    let error = Patch::from_ron(&direct)
        .unwrap()
        .flatten(&loom_modules::registry(), &mut FlatIds::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("module 3 is the part of slot 9"), "{error}");

    let unknown_role = SLOT_FILE.replace(
        "loom.role.filter@1\",\n                    part",
        "loom.role.filter@9\",\n                    part",
    );
    let error = Patch::from_ron(&unknown_role)
        .unwrap()
        .flatten(&loom_modules::registry(), &mut FlatIds::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("slot 9") && error.contains("newer"), "{error}");
}

/// A patch with an SVF whose lowpass and highpass are both cabled, inside a
/// Voice sub-patch, with an assignment on its resonance and one on its
/// cutoff from an LFO at the same level.
const SVF_VOICE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Slots"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 3, kind: "loom.filter.svf@1", params: {"cutoff": 800.0, "resonance": 0.4}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 5, kind: "loom.struct.output@1", label: Some("high")),
                PatchModule(id: 6, kind: "loom.mod.lfo@1"),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (3, "in")),
                PatchCable(from: (3, "lowpass"), to: (4, "in")),
                PatchCable(from: (3, "highpass"), to: (5, "in")),
            ],
            modulation: [
                PatchModulation(source: (6, "out"), target: (3, "cutoff"), depth: 0.2),
                PatchModulation(source: (6, "out"), target: (3, "resonance"), depth: 0.1),
            ],
            midi: [
                PatchMidiMapping(target: (3, "cutoff"), channel: 0, controller: 1),
            ],
            exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff")],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"cutoff": 900.0}),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 5, kind: "loom.clock.transport@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)
"#;

/// Make slot from an SVF in place: the cables on `in` and `lowpass` move to
/// the slot's `in` and `out`, the highpass cable is removed and named, the
/// exposure of cutoff follows onto the slot, and the file holds the slot
/// (sub-patches.md, Slots: Make slot).
#[test]
fn make_slot_moves_role_cables_and_drops_the_rest() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SVF_VOICE).unwrap());
    let svf = inside(&hierarchy, 3);
    let made = hierarchy
        .make_slot(controller.graph(), svf, &roles::FILTER)
        .expect("the SVF fills Filter");
    assert_eq!(made.removed, ["Highpass"]);
    assert_eq!(
        made_slot_text(&roles::FILTER, &made.removed),
        "Filter slot made. The cable from Highpass was removed: a slot has only the Filter role's ports."
    );
    hierarchy.sync(&mut controller).expect("syncs");
    controller.commit().unwrap();
    let slot = made.slot;
    assert_eq!(hierarchy.part_of_slot(slot), Some(svf));
    assert!(hierarchy.is_hidden(svf));
    // The engine still has in → svf.in and svf.lowpass → out, and no highpass cable.
    let flat = cables(&controller);
    assert!(flat.contains(&((1, "out".into()), (svf.0, "in".into()))));
    assert!(flat.contains(&((svf.0, "lowpass".into()), (3, "in".into()))));
    assert!(!flat.iter().any(|(from, _)| from == &(svf.0, "highpass".into())));
    // The exposure names the slot and the role parameter.
    let saved = written(&hierarchy, &controller);
    let voice = &saved.subpatches["voice"];
    let slot_lid = hierarchy.path_of(slot).last();
    assert_eq!(voice.exposed[0].module, slot_lid);
    assert_eq!(voice.exposed[0].param, "cutoff");
    let slot_module = voice.modules.iter().find(|m| m.is_slot()).unwrap();
    assert_eq!(slot_module.slot.as_ref().unwrap().part, Some(3));
    // Assignments and the mapping on the part are written through the slot.
    assert!(
        voice
            .modulation
            .iter()
            .all(|a| a.target.0 == slot_lid || a.target.0 == 3)
    );
    assert_eq!(
        voice
            .modulation
            .iter()
            .filter(|a| a.target == (slot_lid, "cutoff".into()))
            .count(),
        1
    );
    assert_eq!(voice.midi[0].target, (slot_lid, "cutoff".into()));
    // The saved file loads again to the same cables.
    let (again, _) = load(&saved);
    assert_eq!(cables(&again).len(), flat.len());
    // Make slot twice is refused, as is on the part.
    assert!(hierarchy.make_slot(controller.graph(), slot, &roles::FILTER).is_err());
    assert!(hierarchy.make_slot(controller.graph(), svf, &roles::FILTER).is_err());
}

/// Swapping the slot to a Ladder keeps the cables, carries cutoff and
/// resonance across (both role parameters), moves the assignments and the
/// mapping on them, adds one fade, and writes a file that holds the ladder
/// only. Finishing the fade removes the SVF and the fade. Swapping back
/// restores the SVF's own resonance and holds the ladder's drive
/// (sub-patches.md, Slots; FR-ENCAP-08).
#[test]
fn swap_keeps_cables_carries_role_values_and_swapping_back_restores_the_rest() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SVF_VOICE).unwrap());
    let svf = inside(&hierarchy, 3);
    let made = hierarchy.make_slot(controller.graph(), svf, &roles::FILTER).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    let slot = made.slot;
    // A mapping on the SVF's cutoff, as the file had.
    assert!(
        controller.graph().midi_mapping_of(svf, 1).is_some() || controller.graph().midi_mapping_of(svf, 0).is_some()
    );

    let layout = |_: u32| Layout::default();
    let swapped = hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.ladder@1")
        .expect("the ladder fills Filter");
    hierarchy.sync(&mut controller).expect("syncs with the fade");
    controller.commit().unwrap();
    assert_eq!(swapped.part_name, "Ladder");
    assert!(swapped.removed.is_empty(), "every assignment was on a role parameter");
    let ladder = swapped.part;
    assert_eq!(hierarchy.part_of_slot(slot), Some(ladder));
    let fade = module_of(&controller, "loom.struct.fade");
    assert_eq!(swapped.added, vec![ladder, fade]);
    // Cutoff came across (900 from the instance), resonance too; drive is the default.
    let node = controller.graph().node(ladder).unwrap();
    assert_eq!(node.params[node.parameter_index("cutoff").unwrap()], 900.0);
    assert_eq!(node.params[node.parameter_index("resonance").unwrap()], 0.4);
    // Both parts are fed, and the fade sits between them and the output.
    let flat = cables(&controller);
    assert!(flat.contains(&((1, "out".into()), (svf.0, "in".into()))));
    assert!(flat.contains(&((1, "out".into()), (ladder.0, "in".into()))));
    assert!(flat.contains(&((svf.0, "lowpass".into()), (fade.0, "a".into()))));
    assert!(flat.contains(&((ladder.0, "out".into()), (fade.0, "b".into()))));
    assert!(flat.contains(&((fade.0, "out".into()), (3, "in".into()))));
    assert!(!flat.contains(&((ladder.0, "out".into()), (3, "in".into()))));
    // Assignments and the mapping moved to the ladder.
    assert_eq!(
        controller
            .graph()
            .assignments()
            .filter(|(_, a)| a.target == ladder)
            .count(),
        2
    );
    assert!(controller.graph().assignments().all(|(_, a)| a.target != svf));
    let cutoff = node.parameter_index("cutoff").unwrap();
    let drive = node.parameter_index("drive").unwrap();
    assert!(controller.graph().midi_mapping_of(ladder, cutoff).is_some());
    // The file never holds the fade or the leaving part.
    let saved = written(&hierarchy, &controller);
    let voice = &saved.subpatches["voice"];
    assert!(
        voice
            .modules
            .iter()
            .all(|m| m.kind != "loom.struct.fade@1" && m.kind != "loom.filter.svf@1")
    );
    let slot_module = voice
        .modules
        .iter()
        .find(|m| m.is_slot())
        .unwrap()
        .slot
        .as_ref()
        .unwrap();
    assert_eq!(
        slot_module.held[&PartKey::Module("loom.filter.svf@1".into())].params["resonance"],
        0.4
    );
    assert!(
        hierarchy
            .fading_slots()
            .iter()
            .any(|(s, fades)| *s == slot && fades == &[fade])
    );
    assert_eq!(hierarchy.transient_ids(), BTreeSet::from([svf, fade]));

    // Finishing removes the SVF and the fade, and the ladder feeds the output.
    let removed = hierarchy.finish_fade(&mut controller, slot);
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    assert_eq!(
        removed.into_iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([svf, fade])
    );
    assert!(controller.graph().node(svf).is_none() && controller.graph().node(fade).is_none());
    let flat = cables(&controller);
    assert!(flat.contains(&((ladder.0, "out".into()), (3, "in".into()))));
    assert!(hierarchy.fading_slots().is_empty());

    // Change the ladder's drive and cutoff, then swap back: the SVF comes
    // back with the new cutoff and its own resonance, and the ladder's
    // drive is held.
    controller.set_param(ladder, drive, 0.7).unwrap();
    controller.set_param(ladder, cutoff, 400.0).unwrap();
    let back = hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.svf@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    let svf2 = back.part;
    let node = controller.graph().node(svf2).unwrap();
    assert_eq!(node.params[node.parameter_index("cutoff").unwrap()], 400.0);
    assert_eq!(node.params[node.parameter_index("resonance").unwrap()], 0.4);
    let state = hierarchy.slot(slot).unwrap();
    assert_eq!(
        state.held[&PartKey::Module("loom.filter.ladder@1".into())].params["drive"],
        0.7
    );
    assert!(!state.held.contains_key(&PartKey::Module("loom.filter.svf@1".into())));
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    assert!(cables(&controller).contains(&((svf2.0, "lowpass".into()), (3, "in".into()))));
}

/// A swap to a part that does not declare the role is refused, and a
/// swap during a fade finishes the first fade (sub-patches.md, How a swap
/// crossfades, point 5).
#[test]
fn a_swap_during_a_fade_finishes_the_first_and_a_bad_part_is_refused() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SVF_VOICE).unwrap());
    let svf = inside(&hierarchy, 3);
    let slot = hierarchy
        .make_slot(controller.graph(), svf, &roles::FILTER)
        .unwrap()
        .slot;
    hierarchy.sync(&mut controller).unwrap();
    let layout = |_: u32| Layout::default();
    assert_eq!(
        hierarchy.swap_to_module(&mut controller, &layout, slot, "loom.osc.sine@1"),
        Err("Sine does not fill Filter".to_string())
    );
    let first = hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.ladder@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    let second = hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.onepole@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    assert!(controller.graph().node(svf).is_none(), "the first fade finished");
    assert!(
        controller.graph().node(first.part).is_some(),
        "the ladder is now leaving"
    );
    assert_eq!(hierarchy.fading_slots().len(), 1);
    let state = hierarchy.slot(slot).unwrap();
    assert_eq!(state.held.len(), 2);
    assert_eq!(hierarchy.part_of_slot(slot), Some(second.part));
}

/// Dissolve slot puts the part back as a plain module with its cables on
/// its own ports, and forgets the held parts.
#[test]
fn dissolve_slot_restores_a_plain_module() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SLOT_FILE).unwrap());
    let slot = inside(&hierarchy, 9);
    let ladder = inside(&hierarchy, 3);
    let removed = hierarchy.dissolve_slot(controller.graph(), slot).unwrap();
    assert!(removed.is_empty(), "nothing was fading");
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    assert_eq!(hierarchy.tile(controller.graph(), ladder), Some(Tile::Module));
    assert!(hierarchy.slot(slot).is_none());
    let flat = cables(&controller);
    assert!(flat.contains(&((1, "out".into()), (ladder.0, "in".into()))));
    assert!(flat.contains(&((ladder.0, "out".into()), (3, "in".into()))));
    let saved = written(&hierarchy, &controller);
    let voice = &saved.subpatches["voice"];
    assert!(voice.modules.iter().all(|m| !m.is_slot()));
    assert_eq!(voice.exposed[0].module, 3);
    assert!(!saved.subpatches.contains_key("comb"), "the held definition went");
    assert_eq!(voice.cables.len(), 2);
}

/// Expose on a role parameter of the part writes the exposure against the
/// slot, so it follows a swap; Expose on a parameter only the part has is
/// refused with the spec's text (sub-patches.md, Slots).
#[test]
fn expose_on_a_part_goes_through_the_slot_or_is_refused() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SLOT_FILE).unwrap());
    let level = ModulePath::top(2);
    // Resonance is a role parameter: exposed as the slot's.
    let name = hierarchy
        .expose(controller.graph(), &level, 3, "resonance")
        .expect("a role parameter");
    assert_eq!(name, "Resonance");
    let definition = hierarchy.definition_at(&level).unwrap();
    let entry = definition.exposed.iter().find(|e| e.name == "Resonance").unwrap();
    assert_eq!((entry.module, entry.param.as_str()), (9, "resonance"));
    // Slope is the ladder's alone.
    assert_eq!(
        hierarchy.expose(controller.graph(), &level, 3, "slope"),
        Err("Expose a Filter parameter: Slope is only on Ladder".to_string())
    );
    // After a swap to the SVF the exposure follows.
    let slot = inside(&hierarchy, 9);
    let layout = |_: u32| Layout::default();
    let swapped = hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.svf@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    let target = hierarchy.exposed_target(ModuleId(2), 1).unwrap();
    assert_eq!((target.module, target.param.as_str()), (swapped.part, "resonance"));
    assert_eq!(
        hierarchy.exposure_of(swapped.part, "resonance"),
        Some((ModuleId(2), "Resonance".to_string()))
    );
}

/// Swapping to a held sub-patch part brings its definition back whole,
/// with the role value carried and its own exposed values restored, and
/// swapping away again stores it.
#[test]
fn swapping_to_a_held_sub_patch_restores_its_definition() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SLOT_FILE).unwrap());
    let slot = inside(&hierarchy, 9);
    let layout = |_: u32| Layout::default();
    let swapped = hierarchy
        .swap_to_held_subpatch(&mut controller, &layout, slot, "comb")
        .expect("the held comb comes back");
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    assert_eq!(swapped.part_name, "Comb");
    let part_path = hierarchy.path_of(swapped.part);
    let comb = hierarchy.definition_at(&part_path).expect("an instance");
    assert_eq!(comb.metadata.name, "Comb");
    assert!(comb.stored.is_none());
    // The comb's one-pole cutoff took the ladder's 1200 Hz.
    let target = hierarchy.exposed_target(ModuleId(2), 0).unwrap();
    let node = controller.graph().node(target.module).unwrap();
    assert_eq!(node.descriptor.id, "loom.filter.onepole");
    assert_eq!(node.params[node.parameter_index("cutoff").unwrap()], 1200.0);
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    let saved = written(&hierarchy, &controller);
    let voice = &saved.subpatches["voice"];
    let slot_module = voice
        .modules
        .iter()
        .find(|m| m.is_slot())
        .unwrap()
        .slot
        .as_ref()
        .unwrap();
    assert!(
        slot_module
            .held
            .contains_key(&PartKey::Module("loom.filter.ladder@1".into()))
    );
    assert!(!slot_module.held.iter().any(|(k, _)| matches!(k, PartKey::SubPatch(_))));
    assert_eq!(saved.subpatches.len(), 2, "voice and the live comb");
    // Away again: the comb is held whole.
    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.svf@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller);
    let held: Vec<&PartKey> = saved.subpatches["voice"]
        .modules
        .iter()
        .find(|m| m.is_slot())
        .unwrap()
        .slot
        .as_ref()
        .unwrap()
        .held
        .keys()
        .collect();
    assert!(held.iter().any(|k| matches!(k, PartKey::SubPatch(_))));
    let key = held
        .iter()
        .find_map(|k| match k {
            PartKey::SubPatch(key) => Some(key.clone()),
            PartKey::Module(_) => None,
        })
        .unwrap();
    assert_eq!(saved.subpatches[&key].modules.len(), 4);
}

/// Removing a slot tile removes its part too; collapsing a slot and
/// dissolving a sub-patch that holds one are refused for now.
#[test]
fn removing_a_slot_takes_its_part_and_moves_are_refused() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SLOT_FILE).unwrap());
    let slot = inside(&hierarchy, 9);
    let ladder = inside(&hierarchy, 3);
    assert_eq!(
        hierarchy.collapse(controller.graph(), &ModulePath::top(2), &[slot.0]),
        Err(NO_SLOT_IN_COLLAPSE.to_string())
    );
    assert_eq!(
        hierarchy.dissolve(controller.graph(), ModuleId(2)),
        Err(NO_SLOT_IN_DISSOLVE.to_string())
    );
    let removed = hierarchy.remove(controller.graph(), slot);
    assert!(removed.modules.contains(&ladder));
    assert!(removed.tiles.contains(&slot) && removed.tiles.contains(&ladder));
    for id in removed.modules {
        controller.remove_module(id).unwrap();
    }
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller);
    assert!(saved.subpatches["voice"].exposed.is_empty());
    assert!(!saved.subpatches.contains_key("comb"));
}

/// A MIDI mapping learned on the slot's part moves with a swap, and an
/// assignment whose source is the part's role output moves too.
#[test]
fn a_source_on_the_part_moves_with_a_swap() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SVF_VOICE).unwrap());
    let lfo = inside(&hierarchy, 6);
    let slot = hierarchy
        .make_slot(controller.graph(), lfo, &roles::MODULATOR)
        .unwrap()
        .slot;
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    let svf = inside(&hierarchy, 3);
    controller
        .map_midi_named(
            MidiControl {
                channel: 0,
                controller: 7,
            },
            lfo,
            "rate",
        )
        .unwrap();
    let layout = |_: u32| Layout::default();
    let swapped = hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.rand.random@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    // The two assignments from the LFO's out go through the fade while it
    // runs, then start at the random's out.
    let fade = module_of(&controller, "loom.struct.fade");
    let from = |controller: &Controller, module: ModuleId| {
        controller
            .graph()
            .assignments()
            .filter(|(_, a)| a.source.module == module && a.target == svf)
            .count()
    };
    assert_eq!(from(&controller, fade), 2, "through the fade");
    assert_eq!(from(&controller, swapped.part), 0);
    hierarchy.finish_fade(&mut controller, slot);
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    assert_eq!(
        from(&controller, swapped.part),
        2,
        "at the new part once the fade is done"
    );
    assert!(controller.graph().node(fade).is_none());
    // Rate is optional and the random has none: its mapping went, named.
    assert_eq!(swapped.removed, ["Rate"]);
    assert!(
        controller
            .graph()
            .midi_mappings()
            .all(|(_, m)| m.target != lfo && m.target != swapped.part),
        "the random has no rate to map"
    );
}

/// A gate or phase output switches on the swap frame; audio, modulation,
/// and pitch outputs fade (sub-patches.md, How a swap crossfades, point 6).
#[test]
fn gate_and_phase_outputs_are_not_faded() {
    use loom_core::{RolePort, SignalType};
    let port = |signal| RolePort {
        name: "out",
        label: "Out",
        description: "",
        signal,
        required: true,
    };
    assert!(!fades(&port(SignalType::Gate)));
    assert!(!fades(&port(SignalType::Phase)));
    assert!(fades(&port(SignalType::Audio)));
    assert!(fades(&port(SignalType::Modulation)));
    assert!(fades(&port(SignalType::Pitch)));
    assert_eq!(
        swapped_text("Filter", "SVF", &["Slope".to_string(), "Slope".to_string()]),
        "Swapped Filter to SVF. 2 modulations on Slope were removed."
    );
}

/// Removing a sub-patch that holds a slot takes the slot's record, its
/// fade, and the definitions it held with it: the hierarchy is flat
/// again, nothing fades, and the file has no orphan definition.
#[test]
fn removing_a_sub_patch_that_holds_a_slot_takes_the_slot_with_it() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SLOT_FILE).unwrap());
    let slot = inside(&hierarchy, 9);
    let layout = |_: u32| Layout::default();
    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.svf@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    assert_eq!(hierarchy.fading_slots().len(), 1);
    let removed = hierarchy.remove(controller.graph(), ModuleId(2));
    for id in &removed.modules {
        controller.remove_module(*id).unwrap();
    }
    assert!(removed.tiles.contains(&slot));
    assert!(hierarchy.is_flat(), "no instance and no slot is left");
    assert!(hierarchy.fading_slots().is_empty());
    assert!(hierarchy.slot(slot).is_none());
    hierarchy.sync(&mut controller).unwrap();
    let saved = written(&hierarchy, &controller);
    assert!(
        saved.subpatches.is_empty(),
        "no orphan definition: {:?}",
        saved.subpatches.keys()
    );
    assert!(controller.graph().nodes().all(|(_, n)| !matches!(
        n.descriptor.id,
        "loom.struct.fade" | "loom.filter.ladder" | "loom.filter.svf"
    )));
}

/// A file written during a fade holds a modulation from the slot's output
/// as coming from the slot, as finishing the fade makes it, so a save with
/// the engine idle loses nothing (sub-patches.md, How a swap crossfades,
/// point 4).
#[test]
fn a_save_during_a_fade_keeps_modulations_from_the_slot() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SVF_VOICE).unwrap());
    let lfo = inside(&hierarchy, 6);
    let slot = hierarchy
        .make_slot(controller.graph(), lfo, &roles::MODULATOR)
        .unwrap()
        .slot;
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    let slot_lid = hierarchy.path_of(slot).last();
    let before = written(&hierarchy, &controller);
    assert_eq!(before.subpatches["voice"].modulation.len(), 2);
    let layout = |_: u32| Layout::default();
    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.rand.random@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    controller.commit().unwrap();
    // The engine ran nothing: the fade is in progress and the assignments
    // start at the fade.
    let fade = module_of(&controller, "loom.struct.fade");
    assert!(controller.graph().assignments().all(|(_, a)| a.source.module == fade));
    let during = written(&hierarchy, &controller);
    let voice = &during.subpatches["voice"];
    assert_eq!(voice.modulation.len(), 2, "{:?}", voice.modulation);
    assert!(
        voice
            .modulation
            .iter()
            .all(|a| a.source == (slot_lid, "out".to_string()))
    );
    assert!(voice.modules.iter().all(|m| m.kind != "loom.struct.fade@1"));
    // The file reloads with the modulations on the random's output.
    let (again, hierarchy_again) = load(&during);
    let random = module_of(&again, "loom.rand.random");
    assert!(again.graph().assignments().all(|(_, a)| a.source.module == random));
    assert_eq!(again.graph().assignments().count(), 2);
    let _ = hierarchy_again;
}
