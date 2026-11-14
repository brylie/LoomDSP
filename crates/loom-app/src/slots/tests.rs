use super::*;
use crate::inspector::Inspector;
use crate::{Loaded, Source};
use loom_core::{EngineConfig, ModuleNames};
use loom_patch::{ModulePath, hierarchy};

mod empty;

/// Noise into a Voice sub-patch, out to the audio output. Inside Voice an
/// SVF has both its lowpass and its highpass cabled to ports, and its
/// cutoff is exposed on the Voice tile.
const VOICE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Slots"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in"), position: Some((0.0, 0.0))),
                PatchModule(id: 3, kind: "loom.filter.svf@1", params: {"cutoff": 800.0, "resonance": 0.4}, position: Some((200.0, 0.0))),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out"), position: Some((400.0, 0.0))),
                PatchModule(id: 5, kind: "loom.struct.output@1", label: Some("high"), position: Some((400.0, 120.0))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (3, "in")),
                PatchCable(from: (3, "lowpass"), to: (4, "in")),
                PatchCable(from: (3, "highpass"), to: (5, "in")),
            ],
            exposed: [Exposed(name: "Cutoff", module: 3, param: "cutoff")],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1", position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"Cutoff": 900.0}, position: Some((200.0, 0.0))),
        PatchModule(id: 3, kind: "loom.io.audio-out@1", position: Some((400.0, 0.0))),
        PatchModule(id: 4, kind: "loom.clock.transport@1", position: Some((0.0, 200.0))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// The app with [`VOICE`] open, no audio.
fn voice_app() -> App {
    let patch = loom_patch::Patch::from_ron(VOICE).unwrap();
    let loaded = Loaded::read(
        EngineConfig::default(),
        false,
        Source::Recovered(Box::new(patch)),
        &loom_patch::NoLibrary,
    )
    .unwrap();
    App::assemble(
        Err("test".to_string()),
        48_000.0,
        loaded,
        1,
        &crate::tests::app_args(),
        crate::settings::Rig::empty(),
        false,
    )
}

/// The flat id of the tile with level id `lid` inside the Voice tile.
fn inside(app: &App, lid: u32) -> u32 {
    app.session
        .hierarchy
        .id_at(&ModulePath::top(2), lid)
        .expect("a tile inside Voice")
        .0
}

/// The tile menu's labels for a tile.
fn labels(app: &App, id: u32) -> Vec<String> {
    app.tile_menu_items(id).into_iter().map(|(l, _)| l).collect()
}

/// The titles of the tiles the canvas shows.
fn titles(app: &App) -> Vec<String> {
    app.ui_snapshot().tiles.into_iter().map(|t| t.title).collect()
}

/// The app showing the inside of Voice, with the SVF made a Filter slot
/// from its tile menu.
fn with_slot() -> (App, u32, u32) {
    let mut app = voice_app();
    app.show_level(&ModulePath::top(2));
    let svf = inside(&app, 3);
    assert!(
        labels(&app, svf).contains(&hierarchy::make_slot_label(&loom_core::roles::FILTER)),
        "{:?}",
        labels(&app, svf)
    );
    let _ = app.send(Message::MakeSlot {
        module: svf,
        role: "loom.role.filter".to_string(),
    });
    let slot = app
        .session
        .hierarchy
        .slot_of_part(loom_core::ModuleId(svf))
        .expect("the SVF is a part now")
        .0;
    (app, slot, svf)
}

/// Make slot from the tile menu: the SVF tile becomes a Filter slot tile
/// with the role's ports and the SVF's panel, where the SVF stood; the
/// highpass cable is removed and named; the exposure follows onto the
/// slot; undo brings the SVF tile back in one step (sub-patches.md, Slots:
/// Make slot).
#[test]
fn make_slot_from_the_tile_menu() {
    let (mut app, slot, svf) = with_slot();
    assert_eq!(
        app.alerts.status(),
        "Filter slot made. The cable from Highpass was removed: a slot has only the Filter role's ports."
    );
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == slot).expect("the slot tile");
    assert_eq!(tile.title, "Filter");
    assert_eq!(
        tile.inputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["In", "Cutoff"]
    );
    assert_eq!(
        tile.outputs.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["Out"]
    );
    assert!(
        tile.widgets.iter().any(|w| w.label == "Cutoff") && tile.widgets.iter().any(|w| w.label == "Resonance"),
        "the SVF's panel: {:?}",
        tile.widgets
    );
    assert!(snapshot.tiles.iter().all(|t| t.id != svf), "the SVF is hidden");
    assert_eq!(app.session.positions.get(&slot), Some(&(200.0, 0.0)));
    // Two cables through the slot, none to the high port.
    assert_eq!(snapshot.cables.len(), 2);
    assert_eq!(snapshot.selection, [slot]);
    // The exposure names the slot.
    let saved = app.current_patch();
    let voice = &saved.subpatches["voice"];
    let slot_lid = app.session.hierarchy.path_of(loom_core::ModuleId(slot)).last();
    assert_eq!(voice.exposed[0].module, slot_lid);
    assert_eq!(voice.exposed[0].param, "cutoff");
    // The Voice tile's surface control still moves the SVF's cutoff.
    let target = app.session.hierarchy.exposed_target(loom_core::ModuleId(2), 0).unwrap();
    assert_eq!(target.module.0, svf);
    // One undo step: the SVF tile is back where it was, with its cables.
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("make Filter slot"));
    app.undo();
    let snapshot = app.ui_snapshot();
    assert!(snapshot.tiles.iter().any(|t| t.id == svf && t.title == "SVF"));
    assert!(snapshot.tiles.iter().all(|t| t.id != slot));
    assert_eq!(snapshot.cables.len(), 3);
    assert_eq!(app.session.positions.get(&svf), Some(&(200.0, 0.0)));
}

/// Swap from the tile menu: the slot offers every filter but the current
/// one, the swap carries cutoff and resonance to the ladder, the tile shows
/// the ladder's panel, the fade finishes once the engine ran 10 ms, and
/// undo swaps back with a fade and restores the SVF's own values
/// (sub-patches.md, Slots: Swap; FR-ENCAP-08).
#[test]
fn swap_from_the_tile_menu_and_undo_fades_back() {
    let (mut app, slot, svf) = with_slot();
    let items = labels(&app, slot);
    assert!(items.contains(&"Swap to Ladder".to_string()), "{items:?}");
    assert!(items.contains(&"Swap to One-Pole".to_string()), "{items:?}");
    assert!(
        !items.contains(&"Swap to SVF".to_string()),
        "the current part is not offered"
    );
    assert_eq!(items.last().map(String::as_str), Some("Dissolve slot"));
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".to_string()),
    });
    assert_eq!(app.alerts.status(), "Swapped Filter to Ladder.");
    let ladder = app
        .session
        .hierarchy
        .part_of_slot(loom_core::ModuleId(slot))
        .expect("a part")
        .0;
    assert_ne!(ladder, svf);
    let (drive, descriptor_id, cutoff, resonance) = {
        let node = app
            .session
            .controller
            .graph()
            .node(loom_core::ModuleId(ladder))
            .unwrap();
        (
            node.parameter_index("drive").unwrap(),
            node.descriptor.id,
            node.params[node.parameter_index("cutoff").unwrap()],
            node.params[node.parameter_index("resonance").unwrap()],
        )
    };
    assert_eq!(descriptor_id, "loom.filter.ladder");
    assert_eq!(cutoff, 900.0);
    assert_eq!(resonance, 0.4);
    let tile = app
        .ui_snapshot()
        .tiles
        .into_iter()
        .find(|t| t.id == slot)
        .expect("the slot tile");
    assert!(
        tile.widgets.iter().any(|w| w.label == "Drive"),
        "the ladder's panel: {:?}",
        tile.widgets
    );
    // Both parts run until the fade is done; the engine ran nothing yet.
    assert_eq!(app.session.hierarchy.fading_slots().len(), 1);
    assert!(app.session.controller.graph().node(loom_core::ModuleId(svf)).is_some());
    app.process(600);
    assert!(app.session.hierarchy.fading_slots().is_empty(), "the fade finished");
    assert!(app.session.controller.graph().node(loom_core::ModuleId(svf)).is_none());
    // The exposure follows the slot.
    let target = app.session.hierarchy.exposed_target(loom_core::ModuleId(2), 0).unwrap();
    assert_eq!(target.module.0, ladder);
    // One undo step, and it fades: the SVF is back with its own resonance.
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("swap Filter"));
    app.session
        .controller
        .set_param(loom_core::ModuleId(ladder), drive, 0.6)
        .unwrap();
    app.undo();
    assert_eq!(app.session.hierarchy.fading_slots().len(), 1, "undo fades too");
    let back = app
        .session
        .hierarchy
        .part_of_slot(loom_core::ModuleId(slot))
        .expect("a part")
        .0;
    assert_eq!(back, svf, "the SVF comes back at its id");
    {
        let node = app.session.controller.graph().node(loom_core::ModuleId(svf)).unwrap();
        assert_eq!(node.descriptor.id, "loom.filter.svf");
        assert_eq!(node.params[node.parameter_index("resonance").unwrap()], 0.4);
    }
    app.process(600);
    assert!(app.session.hierarchy.fading_slots().is_empty());
    assert!(
        app.session
            .controller
            .graph()
            .node(loom_core::ModuleId(ladder))
            .is_none()
    );
    // Redo fades again and brings the ladder back.
    app.redo();
    assert_eq!(app.session.hierarchy.fading_slots().len(), 1);
    let again = app
        .session
        .hierarchy
        .part_of_slot(loom_core::ModuleId(slot))
        .expect("a part")
        .0;
    assert_eq!(again, ladder);
}

/// The inspector for a slot shows the Part list with the current part
/// marked, the role's rows first and the part's own under its name, and
/// a swap from it works; Expose on a role parameter writes against the
/// slot, and on a part-only parameter is refused (sub-patches.md, Slots).
#[test]
fn the_inspector_shows_the_slot_and_swaps_it() {
    let (mut app, slot, svf) = with_slot();
    app.session.selection = std::collections::BTreeSet::from([slot]);
    let Inspector::Slot(rows) = app.inspector_model() else {
        panic!("a slot inspector");
    };
    assert_eq!(rows.name, "Filter");
    assert_eq!(rows.role, "Filter");
    assert_eq!(rows.part.as_deref(), Some("SVF"));
    assert_eq!(rows.part_id, Some(svf));
    let current: Vec<&str> = rows
        .choices
        .iter()
        .filter(|c| c.current)
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(current, ["SVF"]);
    assert!(rows.choices.iter().any(|c| c.label == "Ladder"));
    assert_eq!(
        rows.role_params.iter().map(|(l, _, _)| l.as_str()).collect::<Vec<_>>(),
        ["Cutoff", "Resonance"]
    );
    assert!(rows.role_params.iter().all(|(_, module, _)| *module == svf));
    assert!(rows.other_params.iter().all(|(l, _, _)| l != "Cutoff"));
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.onepole@1".to_string()),
    });
    let Inspector::Slot(rows) = app.inspector_model() else {
        panic!("a slot inspector");
    };
    assert_eq!(rows.part.as_deref(), Some("One-Pole"));
    assert_eq!(
        rows.role_params.iter().map(|(l, _, _)| l.as_str()).collect::<Vec<_>>(),
        ["Cutoff"],
        "the one-pole has no resonance"
    );
    // Expose on a part-only parameter is refused; on a role parameter it
    // goes through the slot.
    app.process(600);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".to_string()),
    });
    let ladder = app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot)).unwrap().0;
    let (slope, resonance) = {
        let node = app
            .session
            .controller
            .graph()
            .node(loom_core::ModuleId(ladder))
            .unwrap();
        (
            node.parameter_index("slope").unwrap(),
            node.parameter_index("resonance").unwrap(),
        )
    };
    let _ = app.send(Message::Inspector(crate::inspector::Message::Expose {
        module: ladder,
        index: slope,
    }));
    assert_eq!(
        app.alerts.status(),
        "Cannot expose: Expose a Filter parameter: Slope is only on Ladder"
    );
    let _ = app.send(Message::Inspector(crate::inspector::Message::Expose {
        module: ladder,
        index: resonance,
    }));
    assert_eq!(app.alerts.status(), "Resonance is exposed on Voice");
    let voice = app.session.hierarchy.definition_of(loom_core::ModuleId(2)).unwrap();
    let slot_lid = app.session.hierarchy.path_of(loom_core::ModuleId(slot)).last();
    assert!(
        voice
            .exposed
            .iter()
            .any(|e| e.module == slot_lid && e.param == "resonance")
    );
}

/// The Voice tile's menu offers the swaps of the slot its surface control
/// is bound to, since the canvas has no menu on a control; the swap from
/// there keeps the surface control working (FR-PANEL-11).
#[test]
fn a_bound_surface_control_offers_the_swap_through_its_tile() {
    let (mut app, slot, _) = with_slot();
    app.show_level(&ModulePath::root());
    let items = labels(&app, 2);
    assert!(items.contains(&"Swap Filter to Ladder".to_string()), "{items:?}");
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".to_string()),
    });
    let target = app.session.hierarchy.exposed_target(loom_core::ModuleId(2), 0).unwrap();
    let node = app.session.controller.graph().node(target.module).unwrap();
    assert_eq!(node.descriptor.id, "loom.filter.ladder");
    assert_eq!(target.param, "cutoff");
    let surface = crate::surface::control_target(
        app.session.controller.graph(),
        &app.session.hierarchy,
        2,
        0,
        loom_canvas::ControlPart::Main,
    )
    .expect("the surface control resolves");
    assert_eq!(surface.module, target.module);
}

/// Dissolve slot puts the SVF tile back where the slot stood; copying a
/// slot tile is refused with the spec's text.
#[test]
fn dissolve_slot_and_the_clipboard_refusal() {
    let (mut app, slot, svf) = with_slot();
    app.session.selection = std::collections::BTreeSet::from([slot]);
    assert!(app.copy().is_none());
    assert_eq!(app.alerts.status(), CANNOT_COPY_SLOT);
    let _ = app.send(Message::DissolveSlot(slot));
    assert_eq!(app.alerts.status(), "Filter slot dissolved");
    assert!(titles(&app).contains(&"SVF".to_string()));
    assert!(!titles(&app).contains(&"Filter".to_string()));
    assert_eq!(app.session.positions.get(&svf), Some(&(200.0, 0.0)));
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("dissolve Filter slot"));
    let saved = app.current_patch();
    assert!(saved.subpatches["voice"].modules.iter().all(|m| !m.is_slot()));
}

/// Noise into a Voice whose Ladder is already a Filter slot's part, with an
/// LFO inside assigned to the ladder's slope (a parameter outside the
/// role) and a MIDI mapping on it.
const LADDER_SLOT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Slots"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in"), position: Some((0.0, 0.0))),
                PatchModule(id: 3, kind: "loom.filter.ladder@1", params: {"cutoff": 800.0, "drive": 0.3}, position: Some((200.0, 0.0))),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out"), position: Some((400.0, 0.0))),
                PatchModule(id: 6, kind: "loom.mod.lfo@1", position: Some((200.0, 200.0))),
                PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(3))), position: Some((200.0, 0.0))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (9, "in")),
                PatchCable(from: (9, "out"), to: (4, "in")),
            ],
            modulation: [PatchModulation(source: (6, "out"), target: (3, "slope"), depth: 0.3)],
            midi: [PatchMidiMapping(target: (3, "slope"), channel: 0, controller: 7)],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1", position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), position: Some((200.0, 0.0))),
        PatchModule(id: 3, kind: "loom.io.audio-out@1", position: Some((400.0, 0.0))),
        PatchModule(id: 4, kind: "loom.clock.transport@1", position: Some((0.0, 200.0))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// Noise into a Voice whose Filter slot holds the Comb sub-patch, with an
/// SVF held.
const COMB_SLOT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Slots"),
    subpatches: {
        "comb": SubPatch(
            metadata: Metadata(name: "Comb"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 2, kind: "loom.time.delay@1", params: {"time": 0.002}),
                PatchModule(id: 3, kind: "loom.filter.onepole@1", params: {"cutoff": 500.0}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "in")),
                PatchCable(from: (2, "out"), to: (3, "in")),
                PatchCable(from: (3, "lowpass"), to: (4, "in")),
            ],
            exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff")],
            roles: ["loom.role.filter@1"],
        ),
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 3, kind: "loom.struct.subpatch@1", subpatch: Some("comb"), params: {"cutoff": 700.0}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 9, kind: "loom.struct.slot@1", label: Some("Filter"), slot: Some(Slot(
                    role: "loom.role.filter@1",
                    part: Some(3),
                    held: {Module("loom.filter.svf@1"): Held(params: {"cutoff": 800.0, "resonance": 0.4})},
                ))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (9, "in")),
                PatchCable(from: (9, "out"), to: (4, "in")),
            ],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice")),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 5, kind: "loom.clock.transport@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// The app with `text` open, no audio.
fn app_with(text: &str) -> App {
    let patch = loom_patch::Patch::from_ron(text).unwrap();
    let loaded = Loaded::read(
        EngineConfig::default(),
        false,
        Source::Recovered(Box::new(patch)),
        &loom_patch::NoLibrary,
    )
    .unwrap();
    App::assemble(
        Err("test".to_string()),
        48_000.0,
        loaded,
        1,
        &crate::tests::app_args(),
        crate::settings::Rig::empty(),
        false,
    )
}

/// A swap removes the assignment and the mapping on a parameter only the
/// leaving part has, and names them; undo brings both back with their
/// ids; redo removes them again (sub-patches.md, Slots).
#[test]
fn undo_restores_the_links_a_swap_removed() {
    let mut app = app_with(LADDER_SLOT);
    let slot = inside(&app, 9);
    let ladder = inside(&app, 3);
    let (assignment, slope) = {
        let graph = app.session.controller.graph();
        let (id, a) = graph.assignments().next().expect("the slope assignment");
        assert_eq!(a.target.0, ladder);
        (id, a.param)
    };
    assert!(
        app.session
            .controller
            .graph()
            .midi_mapping_of(loom_core::ModuleId(ladder), slope)
            .is_some()
    );
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.svf@1".to_string()),
    });
    assert_eq!(
        app.alerts.status(),
        "Swapped Filter to SVF. 2 modulations on Slope were removed."
    );
    app.process(600);
    assert!(app.session.controller.graph().assignment(assignment).is_none());
    assert!(app.session.controller.graph().midi_mappings().next().is_none());
    app.undo();
    app.process(600);
    let graph = app.session.controller.graph();
    let back = graph
        .assignment(assignment)
        .expect("the assignment is back with its id");
    assert_eq!((back.target.0, back.param), (ladder, slope));
    assert!(graph.midi_mapping_of(loom_core::ModuleId(ladder), slope).is_some());
    app.redo();
    app.process(600);
    assert!(app.session.controller.graph().assignment(assignment).is_none());
    assert!(app.session.controller.graph().midi_mappings().next().is_none());
}

/// A sub-patch part leaves and comes back through undo at the ids it
/// had: the graph's module set is what it was, nothing is left over, the
/// saved file is the same, and redo leaves no extra module either.
#[test]
fn undo_of_a_swap_from_a_sub_patch_part_leaves_no_extra_modules() {
    let mut app = app_with(COMB_SLOT);
    let slot = inside(&app, 9);
    let modules = |app: &App| -> std::collections::BTreeSet<u32> {
        app.session.controller.graph().nodes().map(|(id, _)| id.0).collect()
    };
    let before = modules(&app);
    let saved_before = app.current_patch().to_ron().unwrap();
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".to_string()),
    });
    assert_eq!(app.alerts.status(), "Swapped Filter to Ladder.");
    app.process(600);
    let after_swap = modules(&app);
    assert!(after_swap.len() < before.len(), "the comb's modules went");
    app.undo();
    app.process(600);
    assert_eq!(modules(&app), before, "the comb is back at its ids, nothing left over");
    assert_eq!(app.current_patch().to_ron().unwrap(), saved_before);
    app.redo();
    app.process(600);
    assert_eq!(modules(&app), after_swap);
}

/// A swap asked for during a fade finishes the fade first, so a refused
/// swap leaves the graph and the hierarchy in step and adds no undo step
/// (sub-patches.md, How a swap crossfades, point 5).
#[test]
fn a_refused_swap_during_a_fade_leaves_everything_in_step() {
    let (mut app, slot, svf) = with_slot();
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".to_string()),
    });
    assert_eq!(app.session.hierarchy.fading_slots().len(), 1);
    let steps = app.session.edits.history.len();
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.osc.sine@1".to_string()),
    });
    assert_eq!(app.alerts.status(), "Cannot swap Filter: Sine does not fill Filter");
    assert!(
        app.session.hierarchy.fading_slots().is_empty(),
        "the fade was finished first"
    );
    assert!(app.session.controller.graph().node(loom_core::ModuleId(svf)).is_none());
    assert!(
        app.session
            .controller
            .graph()
            .nodes()
            .all(|(_, n)| n.descriptor.id != "loom.struct.fade" && n.descriptor.id != "loom.osc.sine")
    );
    assert_eq!(app.session.edits.history.len(), steps, "no step for a refused swap");
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".to_string()),
    });
    assert_eq!(app.alerts.status(), "Cannot swap Filter: it holds that part already");
}

/// Swapping from the app to a sub-patch the slot holds brings it back
/// whole, through the Part list's entry, and the status bar names it.
#[test]
fn swapping_back_to_a_held_sub_patch_from_the_app() {
    let mut app = app_with(COMB_SLOT);
    let slot = inside(&app, 9);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.svf@1".to_string()),
    });
    app.process(600);
    app.session.selection = std::collections::BTreeSet::from([slot]);
    let Inspector::Slot(rows) = app.inspector_model() else {
        panic!("a slot inspector");
    };
    let held = rows
        .choices
        .iter()
        .find(|c| matches!(c.to, SwapTarget::Held(_)))
        .expect("the held comb is offered");
    assert_eq!(held.label, "Comb");
    assert!(!held.current);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: held.to.clone(),
    });
    assert_eq!(app.alerts.status(), "Swapped Filter to Comb.");
    app.process(600);
    let part = app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot)).unwrap();
    assert_eq!(app.session.hierarchy.tile_name(part).as_deref(), Some("Comb"));
    let Inspector::Slot(rows) = app.inspector_model() else {
        panic!("a slot inspector");
    };
    assert_eq!(rows.part.as_deref(), Some("Comb"));
    assert!(rows.choices.iter().any(|c| c.current && c.label == "Comb"));
    // The comb's row names the inner one-pole, so its slider moves it.
    let (_, module, _) = &rows.role_params[0];
    let inner = app
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(*module))
        .unwrap();
    assert_eq!(inner.descriptor.id, "loom.filter.onepole");
    assert_eq!(
        rows.role_params.iter().map(|(l, _, _)| l.as_str()).collect::<Vec<_>>(),
        ["cutoff"]
    );
    let saved = app.current_patch();
    assert_eq!(saved.subpatches.len(), 2, "voice and the live comb");
}

/// The slot's inspector draws: its name, the role, the Part list, the
/// role's rows, the part's own rows under its name, and Dissolve slot.
#[test]
fn the_slot_inspector_draws() {
    let (mut app, slot, _) = with_slot();
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".to_string()),
    });
    app.session.selection = std::collections::BTreeSet::from([slot]);
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.redraw();
    assert!(ui.text("Slot for Filter").is_some());
    assert!(ui.text("Ladder only").is_some());
    assert!(ui.text("Dissolve slot").is_some());
    assert!(ui.text("Slope").is_some(), "the ladder's own row");
}

/// Redo of Make slot and undo of Dissolve slot are plain hierarchy
/// changes, not swaps: both apply, and the history stays.
#[test]
fn make_slot_redoes_and_dissolve_slot_undoes() {
    let (mut app, slot, svf) = with_slot();
    app.undo();
    assert!(titles(&app).contains(&"SVF".to_string()));
    app.redo();
    assert_eq!(app.alerts.status(), "Redo: make Filter slot");
    assert!(titles(&app).contains(&"Filter".to_string()));
    assert_eq!(
        app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot)),
        Some(loom_core::ModuleId(svf))
    );
    let _ = app.send(Message::DissolveSlot(slot));
    assert!(titles(&app).contains(&"SVF".to_string()));
    app.undo();
    assert_eq!(app.alerts.status(), "Undo: dissolve Filter slot");
    assert!(titles(&app).contains(&"Filter".to_string()));
    assert_eq!(
        app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot)),
        Some(loom_core::ModuleId(svf))
    );
    assert!(app.ui_snapshot().redo.is_some(), "the history is intact");
}

/// `restore_links` puts back the assignments and mappings a failed swap
/// moved: one retargeted comes back to its source and target with its id,
/// one removed is restored, and a mapping that went is mapped again.
#[test]
fn restore_links_puts_moved_links_back() {
    let mut app = app_with(LADDER_SLOT);
    let ladder = inside(&app, 3);
    let lfo = inside(&app, 6);
    let links = crate::slots::Links::of(app.session.controller.graph());
    let (id, assignment) = links.assignments[0];
    assert_eq!(assignment.target.0, ladder);
    // Move the assignment onto the LFO's own rate, drop the mapping.
    let rate = loom_core::ModuleNames::parameter_index(
        app.session.controller.graph().node(loom_core::ModuleId(lfo)).unwrap(),
        "rate",
    )
    .unwrap();
    app.session
        .controller
        .retarget_assignment(id, assignment.source, loom_core::ModuleId(lfo), rate)
        .unwrap();
    app.session
        .controller
        .unmap_midi(links.midi[0].target, links.midi[0].param)
        .unwrap();
    app.restore_links(&links);
    assert_eq!(app.session.controller.graph().assignment(id), Some(&assignment));
    assert!(
        app.session
            .controller
            .graph()
            .midi_mapping_of(links.midi[0].target, links.midi[0].param)
            .is_some()
    );
    // Removed outright, it comes back with its id too.
    app.session.controller.remove_assignment(id).unwrap();
    app.restore_links(&links);
    assert_eq!(app.session.controller.graph().assignment(id), Some(&assignment));
}

/// A slot tile's controls are keyed under its part, so swapping the part
/// makes them new elements for a screen reader: new UI IDs and new node
/// IDs, even for a control whose name repeats, while the slot tile keeps
/// its own node (ui-layer.md, section 4).
#[test]
fn swapping_a_part_makes_its_controls_new_elements() {
    use crate::a11y::tree::{SlotPart, part_id, tile_id};
    let (mut app, slot, _) = with_slot();
    // The ID and node of the slot tile's control bound to `cutoff`, and the
    // slot tile's own node.
    let look = |app: &mut App| {
        let snapshot = app.ui_snapshot();
        let ids = app.a11y_widget_ids(&snapshot);
        let update = app.a11y_tree(&crate::places::Places::default());
        let tile = snapshot.tiles.iter().find(|t| t.id == slot).expect("the slot tile");
        let (tile_node, tile_built) = update
            .nodes
            .iter()
            .find(|(_, n)| n.role() == accesskit::Role::Group && n.label() == Some(tile.title.as_str()))
            .expect("the slot tile's node");
        let index = (0..tile.widgets.len())
            .find(|i| ids.get(slot, *i).is_some_and(|id| id.to_string().ends_with("/cutoff")))
            .expect("a control bound to cutoff");
        let id = ids.get(slot, index).unwrap().clone();
        let label = tile.widgets[index].label.clone();
        let node = *tile_built
            .children()
            .iter()
            .find(|child| {
                update
                    .nodes
                    .iter()
                    .any(|(n, built)| n == *child && built.label() == Some(label.as_str()))
            })
            .expect("the cutoff control's node");
        let all: Vec<accesskit::NodeId> = update.nodes.iter().map(|(n, _)| *n).collect();
        (id, node, *tile_node, all)
    };

    let (before_id, before_node, tile_before, _) = look(&mut app);
    assert_eq!(
        before_id,
        part_id(slot, SlotPart::Module("loom.filter.svf"))
            .with("param")
            .with("cutoff")
    );
    assert!(before_id.is_under(&tile_id(slot)));

    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.onepole@1".to_string()),
    });
    let (after_id, after_node, tile_after, all) = look(&mut app);

    assert_eq!(
        after_id,
        part_id(slot, SlotPart::Module("loom.filter.onepole"))
            .with("param")
            .with("cutoff")
    );
    assert_ne!(after_id, before_id);
    assert_ne!(after_node, before_node, "the new part's cutoff is a new element");
    assert!(!all.contains(&before_node), "the old part's control left the tree");
    assert_eq!(tile_after, tile_before, "the slot tile is the same element");

    // A sub-patch part is keyed apart from a module type of the same name.
    assert_ne!(
        part_id(slot, SlotPart::SubPatch("loom.filter.svf")),
        part_id(slot, SlotPart::Module("loom.filter.svf"))
    );
}

/// A slot whose part is a sub-patch keys its controls under `subpatch`
/// and the part's key, with the kind `exposed`, so they are not taken for
/// a module's parameters of the same name. Each has a node under the slot
/// tile (ui-layer.md, section 4).
#[test]
fn a_sub_patch_part_keys_the_slots_controls_under_its_key() {
    use crate::a11y::tree::{SlotPart, part_id};
    let mut app = app_with(COMB_SLOT);
    app.show_level(&ModulePath::top(2));
    let slot = inside(&app, 9);
    let snapshot = app.ui_snapshot();
    let ids = app.a11y_widget_ids(&snapshot);
    let tile = snapshot.tiles.iter().find(|t| t.id == slot).expect("the slot tile");
    let found: Vec<_> = (0..tile.widgets.len()).filter_map(|i| ids.get(slot, i)).collect();
    assert_eq!(
        found,
        [&part_id(slot, SlotPart::SubPatch("comb")).with("exposed").with("cutoff")]
    );
    let update = app.a11y_tree(&crate::places::Places::default());
    let cutoff = update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == accesskit::Role::Slider)
        .expect("the comb's cutoff is a slider");
    assert_eq!(cutoff.1.label(), Some(tile.widgets[0].label.as_str()));
}

/// Every item of a tile menu is keyed by what it does, never by what it
/// says: on a module, a slot, and a sub-patch tile, each item has an ID of
/// its own, and none falls back to the unnamed key (ui-layer.md, section 4).
#[test]
fn tile_menu_items_are_keyed_by_what_they_do() {
    use crate::overlays::{ContextMenu, Overlay};
    let (mut app, slot, svf) = with_slot();
    let check = |app: &mut App, module: u32| {
        app.overlays.open(Overlay::Menu(ContextMenu::Tile {
            module,
            at: loom_ui_core::Point::new(10.0, 10.0),
        }));
        let menu = app.menu_model().expect("the menu is open");
        let ids: Vec<String> = menu.items.iter().map(|item| item.id.to_string()).collect();
        assert!(!ids.is_empty(), "{module}: items");
        assert!(ids.iter().all(|id| !id.ends_with("/unnamed")), "{module}: {ids:?}");
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "{module}: {ids:?}");
        app.overlays.close_tile_menu();
    };
    check(&mut app, slot);
    app.overlays
        .open(crate::overlays::Overlay::Menu(crate::overlays::ContextMenu::Tile {
            module: slot,
            at: loom_ui_core::Point::new(10.0, 10.0),
        }));
    let ids: Vec<String> = app
        .menu_model()
        .unwrap()
        .items
        .iter()
        .map(|item| item.id.to_string())
        .collect();
    assert!(ids.contains(&format!("menu/tile/{slot}/dissolve-slot")), "{ids:?}");
    assert!(
        ids.iter()
            .any(|id| id.starts_with(&format!("menu/tile/{slot}/swap/{slot}/"))),
        "{ids:?}"
    );
    app.overlays.close_tile_menu();
    let other = app
        .ui_snapshot()
        .tiles
        .iter()
        .map(|tile| tile.id)
        .find(|id| *id != slot && *id != svf)
        .expect("another tile inside the Voice");
    check(&mut app, other);
    app.show_level(&ModulePath::root());
    check(&mut app, 2);
}

/// A slot's role parameter takes a typed value in the inspector: the draft
/// is keyed by the part's module, and every key stays in it until Enter
/// commits it.
#[test]
fn a_slot_value_field_takes_a_typed_value() {
    let (mut app, slot, svf) = with_slot();
    app.session.selection = std::collections::BTreeSet::from([slot]);
    let cutoff = app
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(svf))
        .unwrap()
        .descriptor
        .param_index("cutoff")
        .unwrap();
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.redraw();
    assert!(matches!(ui.app().inspector_model(), Inspector::Slot(_)));
    ui.type_a_value(svf, cutoff, "440");
    let value = ui
        .app()
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(svf))
        .unwrap()
        .params[cutoff];
    assert!((value - 440.0).abs() < 0.5, "{value}");
}
