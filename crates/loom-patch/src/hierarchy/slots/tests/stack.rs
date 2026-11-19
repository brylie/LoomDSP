//! Empty slots and swaps across stack copies.

use super::*;

/// An empty slot from the browser has no part, takes the role's name made
/// unique among the level's slots, and adds nothing to the graph
/// (sub-patches.md, Empty slots).
#[test]
fn an_empty_slot_is_named_by_its_role_and_adds_no_module() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(EMPTY_SLOTS).unwrap());
    let before = controller.graph().nodes().count();
    let slot = hierarchy
        .add_empty_slot(&ModulePath::root(), &roles::FILTER)
        .expect("a slot at the top level");
    hierarchy.sync(&mut controller).expect("syncs");
    assert_eq!(
        controller.graph().nodes().count(),
        before,
        "an empty slot is routed through"
    );
    let state = hierarchy.slot(slot).expect("a slot");
    assert_eq!(state.part, None);
    assert_eq!(
        state.name(),
        "Filter 2",
        "the file's empty Filter slot has the plain name"
    );
    let third = hierarchy.add_empty_slot(&ModulePath::root(), &roles::EFFECT).unwrap();
    assert_eq!(hierarchy.slot(third).unwrap().name(), "Effect");
}

/// The hierarchy follows an empty thru slot as the flattener does: what
/// stands behind its `out` is what feeds its `in`, and the inputs behind
/// its `in` are what its `out` feeds. An empty slot of another role has
/// nothing behind it.
#[test]
fn the_hierarchy_follows_an_empty_thru_slot_as_the_flattener_does() {
    let (controller, hierarchy) = load(&Patch::from_ron(EMPTY_SLOTS).unwrap());
    let graph = controller.graph();
    let root = ModulePath::root();
    let sine = module_of(&controller, "loom.osc.sine");
    let out = module_of(&controller, "loom.io.audio-out");
    assert_eq!(
        hierarchy.source_behind(graph, &root, &(9, "out".into())),
        Some((sine, "out".to_string()))
    );
    assert_eq!(
        hierarchy.inputs_behind(graph, &root, &(9, "in".into())),
        vec![(out, "in".to_string())]
    );
    assert!(cables(&controller).contains(&((sine.0, "out".into()), (out.0, "in".into()))));
    assert_eq!(hierarchy.source_behind(graph, &root, &(10, "out".into())), None);
}

/// Remove part on a thru slot keeps the part, fading it out to the thru
/// signal with one Linear fade, and holds its values; on any other slot
/// the part goes at once (sub-patches.md, Empty slots).
#[test]
fn remove_part_fades_a_thru_slot_and_lets_any_other_go_at_once() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(EMPTY_SLOTS).unwrap());
    let layout = |_: u32| Layout::default();
    let filter = hierarchy.id_at(&ModulePath::root(), 9).unwrap();
    let envelope = hierarchy.id_at(&ModulePath::root(), 10).unwrap();
    hierarchy
        .swap_to_module(&mut controller, &layout, filter, "loom.filter.svf@1")
        .expect("into an empty Filter slot");
    let fading = hierarchy
        .slot(filter)
        .unwrap()
        .fading
        .clone()
        .expect("a thru swap-in fades");
    assert_eq!(fading.leaving, None, "it fades in from the thru signal");
    assert_eq!(fading.fades.len(), 1);
    hierarchy.sync(&mut controller).expect("syncs");
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).expect("syncs");
    let svf = hierarchy.part_of_slot(filter).expect("the SVF is in");

    let emptied = hierarchy
        .swap_to_empty(&mut controller, &layout, filter)
        .expect("Remove part");
    assert_eq!(emptied.part_name, "SVF");
    assert!(emptied.gone.is_empty(), "a thru slot fades the part out");
    let state = hierarchy.slot(filter).unwrap();
    assert_eq!(state.part, None);
    let fading = state.fading.clone().expect("fading out");
    assert_eq!(fading.leaving, Some(hierarchy.path_of(svf).last()));
    let law = controller.graph().node(fading.fades[0].1).unwrap().params[0];
    assert_eq!(law, 1.0, "Linear");
    assert!(state.held.contains_key(&PartKey::Module("loom.filter.svf@1".into())));
    assert!(hierarchy.is_hidden(svf), "the leaving part is not a tile");
    hierarchy.sync(&mut controller).expect("syncs while fading");

    hierarchy
        .swap_to_module(&mut controller, &layout, envelope, "loom.env.envelope@1")
        .expect("into an empty Envelope slot");
    assert_eq!(
        hierarchy.slot(envelope).unwrap().fading,
        None,
        "a pulled cable does not fade"
    );
    hierarchy.sync(&mut controller).expect("syncs");
    let env = hierarchy.part_of_slot(envelope).unwrap();
    let emptied = hierarchy.swap_to_empty(&mut controller, &layout, envelope).unwrap();
    assert_eq!(emptied.gone, vec![env], "the envelope goes at once");
    assert_eq!(hierarchy.slot(envelope).unwrap().fading, None);
    assert!(
        hierarchy.swap_to_empty(&mut controller, &layout, envelope).is_err(),
        "an empty slot has no part to remove"
    );
}

/// The status after Remove part names what was removed, once each.
#[test]
fn the_remove_part_text_names_what_went() {
    assert_eq!(
        removed_part_text("Filter", "Ladder", &[]),
        "Removed Ladder from Filter."
    );
    assert_eq!(
        removed_part_text("Filter", "Ladder", &["Cutoff".into()]),
        "Removed Ladder from Filter. 1 modulation on Cutoff was removed."
    );
    assert_eq!(
        removed_part_text("Filter", "Ladder", &["Cutoff".into(), "Cutoff".into()]),
        "Removed Ladder from Filter. 2 modulations on Cutoff were removed."
    );
    assert_eq!(
        no_part_text(&roles::FILTER),
        "No part. Passes its input through. Swap to choose one."
    );
    assert_eq!(no_part_text(&roles::ENVELOPE), "No part. Swap to choose one.");
    assert_eq!(
        added_slot_text("Filter 2"),
        "Added Filter 2 slot. Swap a part in from its menu or the inspector."
    );
}

/// A chain of 2,000 empty Effect slots loads into the hierarchy, and the
/// hierarchy follows it to its head and its tail on a small stack, since
/// both follows are loops (sub-patches.md, Empty slots).
#[test]
fn the_hierarchy_follows_a_long_chain_of_empty_thru_slots() {
    const SLOTS: u32 = 2_000;
    let mut modules = String::from(
        "PatchModule(id: 1, kind: \"loom.osc.sine@1\"), PatchModule(id: 2, kind: \"loom.io.audio-out@1\"),",
    );
    let mut cables = String::new();
    let mut from = 1;
    for n in 0..SLOTS {
        let id = 10 + n;
        modules.push_str(&format!(
            "PatchModule(id: {id}, kind: \"loom.struct.slot@1\", slot: Some(Slot(role: \"loom.role.effect@1\"))),"
        ));
        cables.push_str(&format!("PatchCable(from: ({from}, \"out\"), to: ({id}, \"in\")),"));
        from = id;
    }
    cables.push_str(&format!("PatchCable(from: ({from}, \"out\"), to: (2, \"in\")),"));
    let text =
        format!("Patch(version: 1, metadata: Metadata(name: \"Chain\"), modules: [{modules}], cables: [{cables}])");
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let (controller, hierarchy) = load(&Patch::from_ron(&text).unwrap());
            let graph = controller.graph();
            let root = ModulePath::root();
            let sine = module_of(&controller, "loom.osc.sine");
            let out = module_of(&controller, "loom.io.audio-out");
            assert_eq!(
                hierarchy.source_behind(graph, &root, &(from, "out".into())),
                Some((sine, "out".to_string()))
            );
            assert_eq!(
                hierarchy.inputs_behind(graph, &root, &(10, "in".into())),
                vec![(out, "in".to_string())]
            );
        })
        .unwrap()
        .join()
        .expect("no stack overflow");
}

/// A sine into a stack of four copies of a Voice that holds a Filter slot
/// with an SVF, with a spread on the slot's cutoff, into the output.
pub(super) fn stacked_slot(copies: u32) -> String {
    format!(
        r#"Patch(
    version: 1,
    metadata: Metadata(name: "Stacked slot"),
    subpatches: {{
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            mode: Stack,
            copies: {copies},
            spread: {{"cutoff": 0.2}},
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 3, kind: "loom.filter.svf@1", params: {{"cutoff": 800.0}}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(3)))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (9, "in")),
                PatchCable(from: (9, "out"), to: (4, "in")),
            ],
            exposed: [Exposed(name: "cutoff", module: 9, param: "cutoff")],
        ),
    }},
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice")),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#
    )
}

/// The assignments onto `module`, as (id, depth).
fn assignments_onto(controller: &Controller, module: ModuleId) -> Vec<(loom_core::AssignmentId, f32)> {
    controller
        .graph()
        .assignments()
        .filter(|(_, a)| a.target == module)
        .map(|(id, a)| (id, a.depth))
        .collect()
}

/// A swap between two parts inside a stack of four fades in every copy:
/// one fade per copy, every leaving copy keeps its id and its spread
/// assignment (same id, same depth) until the fade ends, a second sync
/// mid-fade changes nothing, a save mid-fade holds no fade or leaving part,
/// and finishing removes them all (ADR-0044).
#[test]
fn a_swap_inside_a_stack_fades_every_copy() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_slot(4)).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let svf0 = hierarchy.part_of_slot(slot).unwrap();
    let svfs: Vec<ModuleId> = std::iter::once(svf0)
        .chain(hierarchy.copies_of(svf0).iter().map(|(id, _)| *id))
        .collect();
    assert_eq!(svfs.len(), 4, "four copies of the SVF");
    let spreads: Vec<_> = svfs.iter().map(|id| assignments_onto(&controller, *id)).collect();
    assert!(
        spreads.iter().all(|s| s.len() == 1),
        "a spread on each copy: {spreads:?}"
    );

    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.ladder@1")
        .expect("the ladder fills Filter");
    hierarchy.sync(&mut controller).expect("syncs");
    let fading = hierarchy.slot(slot).unwrap().fading.clone().expect("fading");
    assert_eq!(fading.fades.len(), 1);
    assert_eq!(fading.copies.len(), 3, "a fade in every other copy");
    let ladders = controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == "loom.filter.ladder")
        .count();
    assert_eq!(ladders, 4, "a new part in every copy");
    for (svf, spread) in svfs.iter().zip(&spreads) {
        assert!(
            controller.graph().node(*svf).is_some(),
            "the leaving copy keeps running"
        );
        assert_eq!(
            &assignments_onto(&controller, *svf),
            spread,
            "its spread stays as it was"
        );
    }
    let count = controller.graph().assignments().count();
    hierarchy.sync(&mut controller).expect("syncs again");
    assert_eq!(
        controller.graph().assignments().count(),
        count,
        "a second sync changes nothing"
    );
    let saved = written(&hierarchy, &controller).to_ron().unwrap();
    assert!(!saved.contains("loom.struct.fade"), "{saved}");
    assert!(
        !saved.contains("kind: \"loom.filter.svf"),
        "the leaving part is not in the file, only held: {saved}"
    );

    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).expect("syncs");
    assert!(
        svfs.iter().all(|id| controller.graph().node(*id).is_none()),
        "every leaving copy went"
    );
    let fades = controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == "loom.struct.fade")
        .count();
    assert_eq!(fades, 0);
    let ladders: Vec<ModuleId> = controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == "loom.filter.ladder")
        .map(|(id, _)| id)
        .collect();
    assert!(
        ladders.iter().all(|id| assignments_onto(&controller, *id).len() == 1),
        "spread on each new copy"
    );
}

/// The copy levels of a slot in a stack of two inside a stack of two are
/// the three other combinations, and a slot outside a stack has none.
#[test]
fn copy_levels_cover_nested_stacks() {
    let (_, mut hierarchy) = load(&Patch::from_ron(&stacked_slot(2)).unwrap());
    let level = ModulePath::top(2);
    assert_eq!(hierarchy.copy_levels(&level), vec![level.copy(1)]);
    assert!(hierarchy.copy_levels(&ModulePath::root()).is_empty());
    // A stack of one is no stack.
    hierarchy.set_copies(ModuleId(2), 1).unwrap();
    assert!(hierarchy.copy_levels(&level).is_empty());
}

/// Single and stack switch with a slot inside; a layer or select is refused
/// with the slot's text, since the inside would move under a member.
#[test]
fn a_slot_allows_a_stack_and_refuses_a_layer() {
    let (_, hierarchy) = load(&Patch::from_ron(&stacked_slot(1)).unwrap());
    let role = |r: &str| r.to_string();
    assert_eq!(
        hierarchy.mode_refusal(ModuleId(2), crate::modes::Mode::Single, &role),
        None
    );
    assert_eq!(
        hierarchy
            .mode_refusal(ModuleId(2), crate::modes::Mode::Layer, &role)
            .as_deref(),
        Some("Cannot make Voice a layer yet: it holds a slot; dissolve the slot first")
    );
}

/// The cutoff of the part in `slot`.
fn part_cutoff(controller: &Controller, hierarchy: &Hierarchy, slot: ModuleId) -> f32 {
    let part = hierarchy.part_of_slot(slot).expect("a part");
    let node = controller.graph().node(part).unwrap();
    node.params[node.parameter_index("cutoff").unwrap()]
}

/// Role values carry through an empty slot: Remove part keeps the Ladder's
/// cutoff with the slot, the file saves it as the slot's `values`, and the
/// SVF swapped in after a reload takes it (FR-ENCAP-08; sub-patches.md,
/// Empty slots).
#[test]
fn role_values_carry_through_an_empty_slot_and_a_reload() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(EMPTY_SLOTS).unwrap());
    let layout = |_: u32| Layout::default();
    let filter = hierarchy.id_at(&ModulePath::root(), 9).unwrap();
    hierarchy
        .swap_to_module(&mut controller, &layout, filter, "loom.filter.ladder@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    hierarchy.finish_all_fades(&mut controller);
    let ladder = hierarchy.part_of_slot(filter).unwrap();
    let index = controller
        .graph()
        .node(ladder)
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    controller.set_param(ladder, index, 640.0).unwrap();
    hierarchy.swap_to_empty(&mut controller, &layout, filter).unwrap();
    hierarchy.sync(&mut controller).unwrap();
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(hierarchy.slot(filter).unwrap().values.get("cutoff"), Some(&640.0));
    // Saved and loaded again, the empty slot still holds the value.
    let text = written(&hierarchy, &controller).to_ron().unwrap();
    assert!(text.contains("values"), "{text}");
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&text).unwrap());
    let filter = hierarchy.id_at(&ModulePath::root(), 9).unwrap();
    hierarchy
        .swap_to_module(&mut controller, &layout, filter, "loom.filter.svf@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(
        part_cutoff(&controller, &hierarchy, filter),
        640.0,
        "the cutoff carried across"
    );
    assert!(
        hierarchy.slot(filter).unwrap().values.is_empty(),
        "a filled slot keeps no values"
    );
}

/// A slot's `values` keep only the role's own parameters on load: a name
/// the Filter role does not have, as a hand-edited file might give, is
/// dropped, so nothing carries it into a part.
#[test]
fn slot_values_keep_only_role_parameters_on_load() {
    let text = EMPTY_SLOTS.replace(
        r#"Slot(role: "loom.role.filter@1")"#,
        r#"Slot(role: "loom.role.filter@1", values: {"cutoff": 640.0, "slope": 1.0})"#,
    );
    let (_, hierarchy) = load(&Patch::from_ron(&text).unwrap());
    let filter = hierarchy.id_at(&ModulePath::root(), 9).unwrap();
    let values = &hierarchy.slot(filter).unwrap().values;
    assert_eq!(values.get("cutoff"), Some(&640.0));
    assert_eq!(values.get("slope"), None, "Slope is not a Filter role parameter");
}

/// A Voice stacked twice inside a Pad stacked twice, the Voice holding a
/// Filter slot.
pub(super) const NESTED_STACKS: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Nested"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            mode: Stack,
            copies: 2,
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 3, kind: "loom.filter.svf@1"),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(3)))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (9, "in")),
                PatchCable(from: (9, "out"), to: (4, "in")),
            ],
        ),
        "pad": SubPatch(
            metadata: Metadata(name: "Pad"),
            mode: Stack,
            copies: 2,
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice")),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "in")),
                PatchCable(from: (2, "out"), to: (4, "in")),
            ],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("pad")),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// In a stack of two inside a stack of two, the slot's level has three
/// other copies; a swap fades in all four, keeps every leaving copy, and
/// finishing removes them all (ADR-0044).
#[test]
fn a_swap_inside_nested_stacks_fades_every_copy() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(NESTED_STACKS).unwrap());
    let level = ModulePath::top(2).child(2);
    assert_eq!(hierarchy.copy_levels(&level).len(), 3, "two times two copies");
    let layout = |_: u32| Layout::default();
    let slot = hierarchy.id_at(&level, 9).unwrap();
    let count = |controller: &Controller, id: &str| {
        controller
            .graph()
            .nodes()
            .filter(|(_, n)| n.descriptor.id == id)
            .count()
    };
    assert_eq!(count(&controller, "loom.filter.svf"), 4);
    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.ladder@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let fading = hierarchy.slot(slot).unwrap().fading.clone().unwrap();
    assert_eq!(fading.copies.len(), 3);
    assert_eq!(count(&controller, "loom.filter.ladder"), 4);
    assert_eq!(count(&controller, "loom.filter.svf"), 4, "every leaving copy runs");
    assert_eq!(count(&controller, "loom.struct.fade"), 4);
    // Every fade is fed on both sides.
    for (_, fade) in fading
        .fades
        .iter()
        .chain(fading.copies.iter().flat_map(|c| c.fades.iter()))
    {
        let node = controller.graph().node(*fade).unwrap();
        for input in 0..2 {
            assert!(
                controller
                    .graph()
                    .source_of(loom_core::InputRef {
                        module: *fade,
                        port: input
                    })
                    .is_some(),
                "fade {fade:?} input {input} of {}",
                node.descriptor.id
            );
        }
    }
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(count(&controller, "loom.filter.svf"), 0);
    assert_eq!(count(&controller, "loom.struct.fade"), 0);
}

/// A cable edit at the top level during a fade inside a stack keeps every
/// copy's fade wired: `sync` builds the copies' wiring again each time.
#[test]
fn a_cable_edit_during_a_stack_fade_keeps_the_wiring_of_every_copy() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(&stacked_slot(4)).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.filter.ladder@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let wired = |controller: &Controller| {
        controller
            .graph()
            .nodes()
            .filter(|(_, n)| n.descriptor.id == "loom.struct.fade")
            .filter(|(id, _)| {
                (0..2).all(|port| {
                    controller
                        .graph()
                        .source_of(loom_core::InputRef { module: *id, port })
                        .is_some()
                })
            })
            .count()
    };
    assert_eq!(wired(&controller), 4);
    // A second output from the sine, then a sync, as a cable edit makes.
    let sine = module_of(&controller, "loom.osc.sine");
    let scope = controller.add_module("loom.display.scope@1").unwrap();
    controller.connect_named(sine, "out", scope, "in").unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(wired(&controller), 4, "every copy's fade is still wired");
}

/// A Voice stacked four times in which a Modulator slot drives an
/// Amplifier's gain through an assignment.
const STACKED_MODULATOR: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Stacked modulator"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            mode: Stack,
            copies: 4,
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 3, kind: "loom.rand.random@1", params: {"low": 0.5, "high": 0.5}),
                PatchModule(id: 5, kind: "loom.level.amp@1"),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.modulator@1", part: Some(3)))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (5, "in")),
                PatchCable(from: (5, "out"), to: (4, "in")),
            ],
            modulation: [
                PatchModulation(source: (9, "out"), target: (5, "gain"), depth: 0.5),
            ],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice")),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// While a Modulator slot in a stack fades, every copy's assignment from
/// the slot comes from that copy's fade, never from both parts at once,
/// and a second sync keeps it so; after the finish each comes from its new
/// part (ADR-0044).
#[test]
fn a_stacked_modulator_drives_each_copy_through_its_fade() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(STACKED_MODULATOR).unwrap());
    let layout = |_: u32| Layout::default();
    let slot = inside(&hierarchy, 9);
    let onto_amps = |controller: &Controller| -> Vec<&'static str> {
        let graph = controller.graph();
        let mut sources: Vec<&'static str> = graph
            .assignments()
            .filter(|(_, a)| {
                graph
                    .node(a.target)
                    .is_some_and(|n| n.descriptor.id == "loom.level.amp")
            })
            .map(|(_, a)| graph.node(a.source.module).unwrap().descriptor.id)
            .collect();
        sources.sort();
        sources
    };
    assert_eq!(onto_amps(&controller), ["loom.rand.random"; 4]);
    hierarchy
        .swap_to_module(&mut controller, &layout, slot, "loom.mod.lfo@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(
        onto_amps(&controller),
        ["loom.struct.fade"; 4],
        "every copy goes through its fade"
    );
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(onto_amps(&controller), ["loom.struct.fade"; 4]);
    hierarchy.finish_all_fades(&mut controller);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(onto_amps(&controller), ["loom.mod.lfo"; 4]);
}

/// Two cables into one input of a slot keep one link, the later, as a flat
/// patch keeps the later cable, so a crafted file cannot reach an end by
/// two routes through it.
#[test]
fn two_cables_into_one_slot_input_keep_the_later() {
    let text = r#"Patch(version: 1, modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.osc.noise@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.effect@1"))),
    ], cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (2, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (3, "in")),
    ])"#;
    let (_, hierarchy) = load(&Patch::from_ron(text).unwrap());
    let root = ModulePath::root();
    let into: Vec<&Link> = hierarchy
        .links
        .iter()
        .filter(|l| l.to == (9, "in".to_string()))
        .collect();
    assert_eq!(into.len(), 1, "{into:?}");
    assert_eq!(into[0].from, (2, "out".to_string()));
    assert_eq!(
        hierarchy.links.linked_into(&root, &(9, "in".to_string())),
        Some(into[0])
    );
}

/// Copy assignments keep file order when their slot source becomes a fade.
#[test]
fn a_stack_fade_keeps_its_assignment_first() {
    let mut patch = Patch::from_ron(STACKED_MODULATOR).unwrap();
    let voice = patch.subpatches.get_mut("voice").unwrap();
    for id in [7, 8] {
        voice.modules.push(PatchModule {
            id,
            kind: "loom.math.constant@1".into(),
            ..voice.modules[0].clone()
        });
        voice.modulation.push(PatchModulation {
            source: (id, "out".into()),
            depth: if id == 7 { 0.25 } else { 0.75 },
            ..voice.modulation[0].clone()
        });
    }
    let (mut controller, mut hierarchy) = load(&patch);
    let slot = inside(&hierarchy, 9);
    let amp = inside(&hierarchy, 5);
    let copy = hierarchy.copies_of(amp)[0].0;
    hierarchy
        .swap_to_module(&mut controller, &|_| Layout::default(), slot, "loom.mod.lfo@1")
        .unwrap();
    hierarchy.sync(&mut controller).unwrap();
    let graph = controller.graph();
    let ordered: Vec<_> = graph
        .assignments()
        .filter(|(_, a)| a.target == copy)
        .map(|(_, a)| (graph.node(a.source.module).unwrap().descriptor.id, a.depth))
        .collect();
    assert_eq!(
        ordered,
        [
            ("loom.struct.fade", 0.5),
            ("loom.math.constant", 0.25),
            ("loom.math.constant", 0.75)
        ]
    );
}
