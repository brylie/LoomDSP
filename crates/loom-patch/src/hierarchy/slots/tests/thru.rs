//! Thru crossfades for empty slots in stacks.

use super::stack::{NESTED_STACKS, stacked_slot};
use super::*;

/// A sine into a Voice stacked `copies` times whose Filter slot (holding
/// an SVF when `part`, else empty) feeds an Amplifier inside the Voice,
/// into the output. A Noise inside the Voice feeds nothing, for a cable
/// to be moved onto. With `fed` false, nothing feeds the Voice's input.
fn stacked_thru(copies: u32, part: bool, fed: bool) -> String {
    let svf = if part {
        r#"PatchModule(id: 3, kind: "loom.filter.svf@1", params: {"cutoff": 800.0}),"#
    } else {
        ""
    };
    let holds = if part { ", part: Some(3)" } else { "" };
    let feed = if fed {
        r#"PatchCable(from: (1, "out"), to: (2, "in")),"#
    } else {
        ""
    };
    format!(
        r#"Patch(
    version: 1,
    metadata: Metadata(name: "Stacked thru"),
    subpatches: {{
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            mode: Stack,
            copies: {copies},
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                {svf}
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 5, kind: "loom.level.amp@1"),
                PatchModule(id: 6, kind: "loom.osc.noise@1"),
                PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1"{holds}))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (9, "in")),
                PatchCable(from: (9, "out"), to: (5, "in")),
                PatchCable(from: (5, "out"), to: (4, "in")),
            ],
        ),
    }},
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice")),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        {feed}
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#
    )
}

/// What feeds the input `port` of `module`, as an end.
fn fed_by(controller: &Controller, module: ModuleId, port: &str) -> Option<End> {
    let graph = controller.graph();
    let input = loom_core::InputRef {
        module,
        port: graph.node(module)?.input_index(port)?,
    };
    let from = graph.source_of(input)?;
    Some(cable_ends(graph, from, input).0)
}

/// The Amplifier in copy 0 and in every other copy, by copy level.
fn amps(hierarchy: &Hierarchy) -> Vec<ModuleId> {
    let level = ModulePath::top(2);
    std::iter::once(level.clone())
        .chain(hierarchy.copy_levels(&level))
        .map(|level| hierarchy.ids.get(&level.child(5)).expect("an Amplifier in the copy"))
        .collect()
}

/// Every fade of a fading slot, copy 0's first, then each copy's.
fn all_fades(fading: &Fading) -> Vec<ModuleId> {
    fading
        .fades
        .iter()
        .chain(fading.copies.iter().flat_map(|c| c.fades.iter()))
        .map(|(_, id)| *id)
        .collect()
}

/// The modules of `kind` in the graph.
fn count_of(controller: &Controller, kind: &str) -> usize {
    controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == kind)
        .count()
}

/// Remove part inside a stack of four adds a fade in every copy at that
/// copy's slot path, records each copy's thru ends from the graph (the
/// sine on `in`, that copy's Amplifier on `out`), and wires each copy's
/// fade into its own Amplifier only: copy 0's fade takes none of the
/// slot's own copies. Finishing leaves the thru alone in every copy
/// (ADR-0046).
#[test]
fn remove_part_inside_a_stack_fades_each_copy_to_its_own_thru() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_thru(4, true, true)).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let sine = (module_of(&controller, "loom.osc.sine").0, "out".to_string());
    let amps = amps(&hierarchy);
    let emptied = hierarchy.swap_to_empty(&mut controller, &layout, slot).unwrap();
    assert!(emptied.gone.is_empty(), "every copy fades out");
    let fading = hierarchy.slot(slot).unwrap().fading.clone().expect("fading");
    assert_eq!(fading.leaving, Some(3));
    assert_eq!(fading.copies.len(), 3, "a fade in every other copy");
    assert_eq!(fading.thru_source.as_ref(), Some(&sine));
    assert_eq!(fading.thru_dests, [(amps[0].0, "in".to_string())]);
    let step = hierarchy.path_of(fading.fades[0].1).last();
    for (k, copy) in fading.copies.iter().enumerate() {
        let fade = copy.fades[0].1;
        assert_eq!(
            hierarchy.path_of(fade),
            copy.level.child(9).child(step),
            "at the copy's slot path"
        );
        assert_eq!(copy.thru_source.as_ref(), Some(&sine));
        assert_eq!(copy.thru_dests, [(amps[k + 1].0, "in".to_string())]);
    }
    hierarchy.sync(&mut controller).unwrap();
    for (amp, fade) in amps.iter().zip(all_fades(&fading)) {
        assert_eq!(fed_by(&controller, *amp, "in"), Some((fade.0, "out".to_string())));
        assert_eq!(fed_by(&controller, fade, "b"), Some(sine.clone()), "b is the thru");
        let a = fed_by(&controller, fade, "a").expect("a is the leaving copy");
        assert_eq!(
            controller.graph().node(ModuleId(a.0)).unwrap().descriptor.id,
            "loom.filter.svf"
        );
    }
    let saved = written(&hierarchy, &controller).to_ron().unwrap();
    assert!(!saved.contains("loom.struct.fade"), "{saved}");
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    for amp in &amps {
        assert_eq!(fed_by(&controller, *amp, "in"), Some(sine.clone()), "the thru alone");
    }
    assert_eq!(count_of(&controller, "loom.struct.fade"), 0);
    assert_eq!(count_of(&controller, "loom.filter.svf"), 0, "every leaving copy went");
}

/// A swap into an empty Filter slot inside a stack of four fades every
/// copy in from its own thru signal: a fade per copy at that copy's slot
/// path, `a` on the sine, `b` on that copy's new SVF, `out` into that
/// copy's Amplifier. Finishing leaves each SVF feeding its Amplifier
/// (ADR-0046).
#[test]
fn a_swap_into_an_empty_slot_inside_a_stack_fades_each_copy_in() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_thru(4, false, true)).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let sine = (module_of(&controller, "loom.osc.sine").0, "out".to_string());
    let amps = amps(&hierarchy);
    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.svf@1")
        .unwrap();
    let fading = hierarchy.slot(slot).unwrap().fading.clone().expect("fading");
    assert_eq!(fading.leaving, None, "from the thru signal");
    assert_eq!(fading.copies.len(), 3);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(count_of(&controller, "loom.filter.svf"), 4, "a new part in every copy");
    let mut parts = BTreeSet::new();
    for (amp, fade) in amps.iter().zip(all_fades(&fading)) {
        assert_eq!(fed_by(&controller, *amp, "in"), Some((fade.0, "out".to_string())));
        assert_eq!(fed_by(&controller, fade, "a"), Some(sine.clone()), "a is the thru");
        let b = fed_by(&controller, fade, "b").expect("b is the copy's part");
        assert_eq!(
            controller.graph().node(ModuleId(b.0)).unwrap().descriptor.id,
            "loom.filter.svf"
        );
        parts.insert(b.0);
    }
    assert_eq!(parts.len(), 4, "each fade has its own copy's part");
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(count_of(&controller, "loom.struct.fade"), 0);
    for amp in &amps {
        let from = fed_by(&controller, *amp, "in").expect("fed");
        assert_eq!(
            controller.graph().node(ModuleId(from.0)).unwrap().descriptor.id,
            "loom.filter.svf"
        );
    }
}

/// Remove part on a slot that feeds the stack's output: each copy's fade
/// feeds that copy's own input on the stack's Sum, copy 0's included,
/// which no path or link names (ADR-0046).
#[test]
fn remove_part_feeding_the_stack_output_fades_into_every_copy_s_sum_input() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_slot(4)).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let sum = module_of(&controller, "loom.struct.sum");
    hierarchy.swap_to_empty(&mut controller, &layout, slot).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let fading = hierarchy.slot(slot).unwrap().fading.clone().unwrap();
    let node = controller.graph().node(sum).unwrap();
    let feeds: Vec<Option<End>> = node
        .descriptor
        .inputs
        .iter()
        .take(4)
        .map(|port| fed_by(&controller, sum, port.name))
        .collect();
    let fades: Vec<Option<End>> = all_fades(&fading)
        .into_iter()
        .map(|f| Some((f.0, "out".to_string())))
        .collect();
    assert_eq!(feeds, fades, "copy k's fade into copy k's input on the Sum");
    let out = module_of(&controller, "loom.io.audio-out");
    assert_eq!(
        fed_by(&controller, out, "in").map(|e| e.0),
        Some(sum.0),
        "the Sum still plays"
    );
}

/// A cable moved or pulled during a stack fade wins: the fade does not
/// take back the input the player plugged elsewhere, and a pulled cable is
/// not put back, during the fade or when it finishes (ADR-0046).
#[test]
fn a_cable_moved_or_pulled_during_a_stack_fade_is_not_put_back() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_thru(4, true, true)).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let amps = amps(&hierarchy);
    let level = ModulePath::top(2);
    let noise = hierarchy.id_at(&level, 6).unwrap();
    hierarchy.swap_to_empty(&mut controller, &layout, slot).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    // Moved: the Amplifier now reads the Noise, not the slot.
    let amp_in = (5, "in".to_string());
    assert!(hierarchy.disconnect(&level, &amp_in));
    controller.connect_named(noise, "out", amps[0], "in").unwrap();
    hierarchy.sync(&mut controller).unwrap();
    for amp in &amps {
        let from = fed_by(&controller, *amp, "in").expect("the player's cable");
        assert_eq!(
            controller.graph().node(ModuleId(from.0)).unwrap().descriptor.id,
            "loom.osc.noise",
            "the player's cable"
        );
    }
    // Pulled: nothing feeds the Amplifier, and nothing is put back.
    let input = controller.input_ref(amps[0], "in").unwrap();
    controller.disconnect(input).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    for amp in &amps {
        assert_eq!(fed_by(&controller, *amp, "in"), None, "not put back mid-fade");
    }
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    for amp in &amps {
        assert_eq!(fed_by(&controller, *amp, "in"), None, "not put back at the finish");
    }
}

/// With nothing on the Voice's input, Remove part fades each copy's part
/// to 0 into its own Amplifier. Deleting the Amplifier during the fade
/// takes it out of every copy, and no later sync wires a fade to an input
/// whose module is gone (ADR-0046).
#[test]
fn a_destination_deleted_during_a_stack_fade_is_not_wired_again() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_thru(4, true, false)).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let amps = amps(&hierarchy);
    hierarchy.swap_to_empty(&mut controller, &layout, slot).unwrap();
    let fading = hierarchy.slot(slot).unwrap().fading.clone().unwrap();
    assert!(fading.copies.iter().all(|c| c.thru_source.is_none()), "nothing on in");
    hierarchy.sync(&mut controller).unwrap();
    for (amp, fade) in amps.iter().zip(all_fades(&fading)) {
        assert_eq!(fed_by(&controller, *amp, "in"), Some((fade.0, "out".to_string())));
        assert_eq!(fed_by(&controller, fade, "b"), None, "the part fades to 0");
    }
    let removed = hierarchy.remove(controller.graph(), amps[0]);
    hierarchy.sync_without(&mut controller, &removed.modules).unwrap();
    for id in &removed.modules {
        controller.remove_module(*id).unwrap();
    }
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(count_of(&controller, "loom.level.amp"), 0, "gone from every copy");
    for fade in all_fades(&fading) {
        assert!(
            controller.graph().cables().all(|(from, _)| from.module != fade),
            "fade {fade:?} feeds nothing"
        );
    }
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(count_of(&controller, "loom.struct.fade"), 0);
}

/// A sub-patch part swapped into an empty slot in a stack of four wires
/// every copy's fade `b` to that copy's own inner output module, found by
/// its path once the flattener has added the copy (ADR-0046).
#[test]
fn a_sub_patch_part_into_an_empty_slot_in_a_stack_wires_every_copy_s_inner_output() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_thru(4, false, true)).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let comb = Patch::from_ron(SLOT_FILE).unwrap().subpatches["comb"].clone();
    let file = Patch::from_definition(&comb, &BTreeMap::new());
    hierarchy
        .swap_to_subpatch(&mut controller, &layout, slot, &file, None)
        .expect("the comb fills Filter");
    hierarchy.sync(&mut controller).unwrap();
    let fading = hierarchy.slot(slot).unwrap().fading.clone().unwrap();
    assert_eq!(fading.copies.len(), 3);
    let amps = amps(&hierarchy);
    let mut inner = BTreeSet::new();
    for (k, (amp, fade)) in amps.iter().zip(all_fades(&fading)).enumerate() {
        assert_eq!(fed_by(&controller, *amp, "in"), Some((fade.0, "out".to_string())));
        let b = fed_by(&controller, fade, "b").expect("b is fed");
        let node = controller.graph().node(ModuleId(b.0)).unwrap();
        assert_eq!(node.descriptor.id, "loom.filter.onepole", "the comb's output module");
        if k > 0 {
            let level = &fading.copies[k - 1].level;
            assert!(hierarchy.path_of(ModuleId(b.0)).starts_with(level), "in copy {k}");
        }
        inner.insert(b.0);
    }
    assert_eq!(inner.len(), 4, "each copy's own inner module");
}

/// In a stack of two inside a stack of two, Remove part fades in all four
/// copies, three of them past copy 0, each fade into its own input, and
/// finishing leaves no fade or leaving part (ADR-0046).
#[test]
fn remove_part_inside_nested_stacks_fades_every_copy() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(NESTED_STACKS).unwrap());
    let level = ModulePath::top(2).child(2);
    let layout = |_: u32| Layout::default();
    let slot = hierarchy.id_at(&level, 9).unwrap();
    hierarchy.swap_to_empty(&mut controller, &layout, slot).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let fading = hierarchy.slot(slot).unwrap().fading.clone().unwrap();
    assert_eq!(fading.copies.len(), 3, "three copy levels");
    let mut fed = BTreeSet::new();
    for fade in all_fades(&fading) {
        for port in ["a", "b"] {
            assert!(fed_by(&controller, fade, port).is_some(), "fade {fade:?} {port}");
        }
        let into: Vec<_> = controller
            .graph()
            .cables()
            .filter(|(from, _)| from.module == fade)
            .map(|(_, to)| to)
            .collect();
        assert_eq!(into.len(), 1, "fade {fade:?} feeds one input");
        fed.insert(into[0]);
    }
    assert_eq!(fed.len(), 4, "each fade into its own input");
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(count_of(&controller, "loom.struct.fade"), 0);
    assert_eq!(count_of(&controller, "loom.filter.svf"), 0);
}

/// `drop_removal_fades` takes back every copy's fade, which never reached
/// the engine, so their slots are free at once, and lets every copy's
/// part go: the caller syncs without them and removes them, and each
/// Amplifier then reads the thru (sub-patches.md, The module limit).
#[test]
fn drop_removal_fades_lets_every_copy_go_at_once() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_thru(4, true, true)).unwrap());
    controller.commit().unwrap();
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let sine = (module_of(&controller, "loom.osc.sine").0, "out".to_string());
    let free = controller.free_module_slots();
    hierarchy.swap_to_empty(&mut controller, &layout, slot).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(controller.free_module_slots(), free - 4, "four fades");
    let gone = hierarchy.drop_removal_fades(&mut controller, slot);
    assert_eq!(gone.len(), 4, "every copy's SVF goes");
    assert!(
        gone.iter()
            .all(|id| controller.graph().node(*id).unwrap().descriptor.id == "loom.filter.svf")
    );
    assert_eq!(
        controller.free_module_slots(),
        free,
        "the fades' slots are free at once"
    );
    assert_eq!(hierarchy.slot(slot).unwrap().fading, None);
    assert_eq!(count_of(&controller, "loom.struct.fade"), 0);
    hierarchy.sync_without(&mut controller, &gone).unwrap();
    for id in &gone {
        controller.remove_module(*id).unwrap();
    }
    hierarchy.sync(&mut controller).unwrap();
    for amp in amps(&hierarchy) {
        assert_eq!(fed_by(&controller, amp, "in"), Some(sine.clone()));
    }
    assert!(
        hierarchy.drop_removal_fades(&mut controller, slot).is_empty(),
        "nothing left to drop"
    );
}

/// When the engine has no slot left for every copy's fade, Remove part
/// still succeeds: what it added is taken away again, and every copy's
/// part goes at once, as a slot with no thru does (sub-patches.md, The
/// module limit).
#[test]
fn remove_part_with_no_room_for_its_fades_lets_every_copy_go() {
    let patch = Patch::from_ron(&stacked_thru(4, true, true)).unwrap();
    let (probe, _) = load(&patch);
    let used = EngineConfig::default().max_modules - probe.free_module_slots();
    let config = EngineConfig {
        max_modules: used + 2,
        ..EngineConfig::default()
    };
    let EngineParts { mut controller, .. } = create_engine(loom_modules::registry(), config);
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).unwrap();
    let mut hierarchy = Hierarchy::from_patch(&patch, controller.graph(), ids, controller.registry()).unwrap();
    controller.commit().unwrap();
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let modules = controller.graph().nodes().count();
    let emptied = hierarchy
        .swap_to_empty(&mut controller, &layout, slot)
        .expect("never fails for room");
    assert_eq!(emptied.gone.len(), 4, "every copy's part goes at once");
    assert_eq!(controller.graph().nodes().count(), modules, "no fade is left");
    assert_eq!(controller.free_module_slots(), 2);
    assert_eq!(hierarchy.slot(slot).unwrap().fading, None);
    hierarchy.sync_without(&mut controller, &emptied.gone).unwrap();
    for id in &emptied.gone {
        controller.remove_module(*id).unwrap();
    }
    controller.commit().unwrap();
}
