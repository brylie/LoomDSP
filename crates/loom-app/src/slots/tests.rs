use super::*;
use crate::inspector::Inspector;
use crate::{Loaded, Source};
use loom_core::{EngineConfig, ModuleNames};
use loom_patch::{ModulePath, hierarchy};

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
    let _ = app.update(Message::MakeSlot {
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
    let _ = app.update(Message::SwapSlot {
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
        rows.role_params.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
        ["Cutoff", "Resonance"]
    );
    assert!(rows.other_params.iter().all(|(l, _)| l != "Cutoff"));
    let _ = app.update(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.onepole@1".to_string()),
    });
    let Inspector::Slot(rows) = app.inspector_model() else {
        panic!("a slot inspector");
    };
    assert_eq!(rows.part.as_deref(), Some("One-Pole"));
    assert_eq!(
        rows.role_params.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
        ["Cutoff"],
        "the one-pole has no resonance"
    );
    // Expose on a part-only parameter is refused; on a role parameter it
    // goes through the slot.
    app.process(600);
    let _ = app.update(Message::SwapSlot {
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
    let _ = app.update(Message::Inspector(crate::inspector::Message::Expose {
        module: ladder,
        index: slope,
    }));
    assert_eq!(
        app.alerts.status(),
        "Cannot expose: Expose a Filter parameter: Slope is only on Ladder"
    );
    let _ = app.update(Message::Inspector(crate::inspector::Message::Expose {
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
    let _ = app.update(Message::SwapSlot {
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
    let _ = app.update(Message::DissolveSlot(slot));
    assert_eq!(app.alerts.status(), "Filter slot dissolved");
    assert!(titles(&app).contains(&"SVF".to_string()));
    assert!(!titles(&app).contains(&"Filter".to_string()));
    assert_eq!(app.session.positions.get(&svf), Some(&(200.0, 0.0)));
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("dissolve Filter slot"));
    let saved = app.current_patch();
    assert!(saved.subpatches["voice"].modules.iter().all(|m| !m.is_slot()));
}
