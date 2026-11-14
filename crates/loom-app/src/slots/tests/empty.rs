//! Empty slots in the app: added from the module browser, their note and
//! menus, Remove part, and undo back to empty (sub-patches.md, Empty
//! slots).

use super::*;
use crate::ui_test::Ui;
use iced::keyboard::{self, key::Named};

/// A sine into an empty Filter slot into the output, with a Voice
/// sub-patch whose surface exposes a Filter slot's cutoff.
const EMPTY: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Empty"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in"), position: Some((0.0, 0.0))),
                PatchModule(id: 3, kind: "loom.filter.svf@1", params: {"cutoff": 800.0}, position: Some((200.0, 0.0))),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out"), position: Some((400.0, 0.0))),
                PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(3))), position: Some((200.0, 0.0))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (9, "in")),
                PatchCable(from: (9, "out"), to: (4, "in")),
            ],
            exposed: [Exposed(name: "cutoff", module: 9, param: "cutoff")],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1", position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), position: Some((200.0, 200.0))),
        PatchModule(id: 3, kind: "loom.io.audio-out@1", position: Some((400.0, 0.0))),
        PatchModule(id: 4, kind: "loom.clock.transport@1", position: Some((0.0, 300.0))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// Opens the browser, types `query`, and presses Enter.
fn add_from_browser(ui: &mut Ui, query: &str) {
    ui.send(Message::OpenBrowser);
    ui.redraw();
    assert!(ui.app().overlays.browser().is_some(), "the browser is open");
    ui.send(Message::Browser(browser::Message::Query(query.into())));
    ui.send(Message::Browser(browser::Message::Submit));
    ui.redraw();
}

/// The one selected tile.
fn selected(app: &App) -> u32 {
    let selection: Vec<u32> = app.session.selection.iter().copied().collect();
    assert_eq!(selection.len(), 1, "one tile is selected");
    selection[0]
}

/// An empty Filter slot from the browser is added and selected, shows the
/// role's ports and draws its note, which the tile's description says to
/// a screen reader too; it adds no module; its menu has the parts and no
/// Dissolve slot; one undo step takes it away (sub-patches.md, Empty slots).
#[test]
fn an_empty_slot_from_the_browser_shows_its_ports_and_note() {
    let mut ui = Ui::with_app(app_with(EMPTY));
    let modules = ui.app().session.controller.graph().nodes().count();
    add_from_browser(&mut ui, "filter slot");
    let slot = selected(ui.app());
    assert_eq!(ui.app().session.controller.graph().nodes().count(), modules);
    assert_eq!(
        ui.app().alerts.status(),
        "Added Filter slot. Swap a part in from its menu or the inspector."
    );
    let tile = ui
        .app()
        .ui_snapshot()
        .tiles
        .into_iter()
        .find(|t| t.id == slot)
        .expect("the slot tile");
    assert_eq!(tile.title, "Filter");
    let inputs: Vec<&str> = tile.inputs.iter().map(|i| i.label.as_str()).collect();
    let outputs: Vec<&str> = tile.outputs.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(inputs, ["In", "Cutoff"]);
    assert_eq!(outputs, ["Out"]);
    let note: Vec<(&str, &str, Option<&String>)> = tile
        .widgets
        .iter()
        .map(|w| (w.kind, w.label.as_str(), w.shown.as_ref()))
        .collect();
    assert_eq!(
        note,
        [
            ("note", "No part.", None),
            ("note", "Passes its input through.", None),
            ("note", "Swap to choose one.", None),
        ],
        "drawn in the panel, never cut short"
    );
    assert_eq!(
        tile.description.as_deref(),
        Some("Slot for Filter. No part. Passes its input through. Swap to choose one.")
    );
    let items = labels(ui.app(), slot);
    assert!(items.contains(&"Swap to SVF".to_string()), "{items:?}");
    assert!(!items.contains(&"Dissolve slot".to_string()), "{items:?}");
    assert!(!items.contains(&"Remove part".to_string()), "{items:?}");
    assert_eq!(ui.app().ui_snapshot().undo.as_deref(), Some("add Filter slot"));
    ui.app_mut().undo();
    assert!(ui.app().session.hierarchy.slot(loom_core::ModuleId(slot)).is_none());
}

/// A second empty slot of one role takes the next name, and an Envelope
/// slot's note does not say it passes its input through.
#[test]
fn slot_names_are_unique_and_the_note_follows_the_role() {
    let mut ui = Ui::with_app(app_with(EMPTY));
    add_from_browser(&mut ui, "filter slot");
    add_from_browser(&mut ui, "filter slot");
    let second = selected(ui.app());
    assert_eq!(
        ui.app()
            .session
            .hierarchy
            .tile_name(loom_core::ModuleId(second))
            .as_deref(),
        Some("Filter 2")
    );
    add_from_browser(&mut ui, "envelope slot");
    let envelope = selected(ui.app());
    let tile = ui
        .app()
        .ui_snapshot()
        .tiles
        .into_iter()
        .find(|t| t.id == envelope)
        .unwrap();
    let lines: Vec<&str> = tile.widgets.iter().map(|w| w.label.as_str()).collect();
    assert_eq!(lines, ["No part.", "Swap to choose one."]);
}

/// The keyboard route: the new slot is selected, Shift+F10 opens its menu,
/// and a swap from it puts a part in.
#[test]
fn an_empty_slot_is_reached_and_filled_by_keyboard() {
    let mut ui = Ui::with_app(app_with(EMPTY));
    add_from_browser(&mut ui, "filter slot");
    let slot = selected(ui.app());
    ui.tap(keyboard::Key::Named(Named::F10), keyboard::Modifiers::SHIFT);
    ui.redraw();
    assert_eq!(
        ui.app().overlays.tile_menu().map(|(m, _)| m),
        Some(slot),
        "its menu opened"
    );
    let swap = ui
        .app()
        .tile_menu_items(slot)
        .into_iter()
        .find(|(label, _)| label == "Swap to SVF")
        .expect("the swap item")
        .1;
    ui.send(swap);
    assert!(
        ui.app()
            .session
            .hierarchy
            .part_of_slot(loom_core::ModuleId(slot))
            .is_some()
    );
}

/// Remove part empties a filled slot as one undo step, keeps the part held
/// so a swap back restores it, and names what went; the menu has one
/// Remove part, before Dissolve slot; undo of a swap into an empty slot
/// leaves no stray module (sub-patches.md, Empty slots).
#[test]
fn remove_part_and_undo_back_to_empty_leave_no_stray_module() {
    let mut ui = Ui::with_app(app_with(EMPTY));
    add_from_browser(&mut ui, "filter slot");
    let slot = selected(ui.app());
    let before = ui.app().session.controller.graph().nodes().count();
    let _ = ui.app_mut().send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".into()),
    });
    let ladder = ui
        .app()
        .session
        .hierarchy
        .part_of_slot(loom_core::ModuleId(slot))
        .unwrap();
    ui.app_mut().process(600);
    assert!(
        ui.app().session.hierarchy.fading_slots().is_empty(),
        "the fade in finished"
    );
    let items = labels(ui.app(), slot);
    let removes = items.iter().filter(|l| *l == "Remove part").count();
    assert_eq!(removes, 1, "{items:?}");
    assert_eq!(&items[items.len() - 2..], ["Remove part", "Dissolve slot"]);
    // Undo of the swap-in empties the slot and leaves no stray module.
    ui.app_mut().undo();
    ui.app_mut().process(600);
    assert_eq!(ui.app().session.hierarchy.part_of_slot(loom_core::ModuleId(slot)), None);
    assert!(ui.app().session.controller.graph().node(ladder).is_none());
    assert_eq!(ui.app().session.controller.graph().nodes().count(), before);
    // Redo, then Remove part from the menu.
    ui.app_mut().redo();
    ui.app_mut().process(600);
    let _ = ui.app_mut().send(Message::SwapSlot {
        slot,
        to: SwapTarget::Empty,
    });
    assert_eq!(ui.app().alerts.status(), "Removed Ladder from Filter.");
    assert_eq!(ui.app().session.hierarchy.part_of_slot(loom_core::ModuleId(slot)), None);
    ui.app_mut().process(600);
    assert_eq!(ui.app().session.controller.graph().nodes().count(), before);
    let held = &ui.app().session.hierarchy.slot(loom_core::ModuleId(slot)).unwrap().held;
    assert!(held.contains_key(&loom_patch::PartKey::Module("loom.filter.ladder@1".into())));
    assert_eq!(ui.app().ui_snapshot().undo.as_deref(), Some("remove part from Filter"));
    // A second Remove part is refused with a reason.
    let _ = ui.app_mut().send(Message::SwapSlot {
        slot,
        to: SwapTarget::Empty,
    });
    assert_eq!(
        ui.app().alerts.status(),
        "Cannot remove the part from Filter: it holds no part"
    );
}

/// The inspector's Part list starts with `No part`, current for an empty
/// slot, and leaves out Dissolve slot while the slot is empty.
#[test]
fn the_inspector_offers_no_part_and_no_dissolve_for_an_empty_slot() {
    let mut ui = Ui::with_app(app_with(EMPTY));
    add_from_browser(&mut ui, "filter slot");
    let Inspector::Slot(rows) = ui.app().inspector_model() else {
        panic!("a slot's inspector")
    };
    assert_eq!(rows.choices[0].label, "No part");
    assert!(rows.choices[0].current);
    assert_eq!(rows.choices[0].to, SwapTarget::Empty);
    let items = crate::inspector::access::items(&ui.app().inspector_model());
    let text = format!("{items:?}");
    assert!(!text.contains("Dissolve slot"), "{text}");
}

/// A surface control bound to an empty slot's role parameter is drawn
/// dimmed, is not a control, and says why (sub-patches.md, Empty slots).
#[test]
fn a_surface_control_bound_to_an_empty_slot_is_dimmed_and_says_why() {
    let mut app = app_with(EMPTY);
    let slot = inside(&app, 9);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Empty,
    });
    app.process(600);
    app.refresh_values();
    assert_eq!(app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot)), None);
    let graph = app.session.controller.graph();
    assert!(!crate::surface::is_control(graph, &app.session.hierarchy, 2, 0));
    let value = app.session.values.get(2, 0).expect("a value for the control");
    assert!(value.dimmed);
    let panel = crate::surface::surface_panel(graph, &app.session.hierarchy, loom_core::ModuleId(2));
    assert_eq!(
        panel.widgets[0].hover.description,
        "Filter has no part. Swap one in from the tile's menu."
    );
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == 2).unwrap();
    assert_eq!(
        tile.widgets[0].inert.as_deref(),
        Some("Filter has no part. Swap one in from the tile's menu.")
    );
}

/// A Voice stacked four times whose Filter slot holds an SVF.
const STACKED: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Stacked"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            mode: Stack,
            copies: 4,
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

/// The app with `text` open and an engine that holds at most `max_modules`.
fn app_with_limit(text: &str, max_modules: usize) -> App {
    let patch = loom_patch::Patch::from_ron(text).unwrap();
    let config = EngineConfig {
        max_modules,
        ..EngineConfig::default()
    };
    let loaded = Loaded::read(
        config,
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

/// Raising Copies while a swap inside the stack still fades finishes the
/// fade first, so the new copies never meet a half-done fade (ADR-0044).
#[test]
fn a_mode_edit_during_a_fade_finishes_it_first() {
    let mut app = app_with(STACKED);
    let slot = inside(&app, 9);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".into()),
    });
    assert_eq!(app.session.hierarchy.fading_slots().len(), 1);
    app.mode_message(crate::modes::Message::SetCopies { tile: 2, copies: 5 });
    assert!(
        app.session.hierarchy.fading_slots().is_empty(),
        "the fade finished first"
    );
    let fades = app
        .session
        .controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == "loom.struct.fade")
        .count();
    assert_eq!(fades, 0);
}

/// A swap inside a stack that would pass the engine's module limit in some
/// copy is refused whole: nothing is added, and the slot keeps its part
/// (sub-patches.md, How modes meet the rest).
#[test]
fn a_swap_past_the_module_limit_is_refused_whole() {
    let probe = app_with(STACKED);
    let count = probe.session.controller.graph().nodes().count();
    // Room for two more modules: four ladders and four fades do not fit.
    let mut app = app_with_limit(STACKED, count + 2);
    let slot = inside(&app, 9);
    let part = app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot));
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".into()),
    });
    assert!(
        app.alerts.status().starts_with("Cannot swap Filter"),
        "{}",
        app.alerts.status()
    );
    assert_eq!(app.session.controller.graph().nodes().count(), count);
    assert_eq!(app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot)), part);
    assert!(app.session.hierarchy.fading_slots().is_empty());
}

/// Remove part on a Ladder whose Slope is modulated and mapped names the
/// Slope in the status, takes both away with the part, and one undo puts
/// the part, the modulation, and the mapping back (sub-patches.md, Empty
/// slots).
#[test]
fn remove_part_names_the_modulation_it_removes_and_undo_restores_it() {
    let mut app = app_with(LADDER_SLOT);
    let slot = inside(&app, 9);
    let assignments = app.session.controller.graph().assignments().count();
    let mappings = app.session.controller.graph().midi_mappings().count();
    assert_eq!((assignments, mappings), (1, 1));
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Empty,
    });
    let status = app.alerts.status().to_string();
    assert!(status.starts_with("Removed Ladder from Filter."), "{status}");
    assert!(status.contains("Slope"), "{status}");
    app.process(600);
    assert_eq!(app.session.controller.graph().assignments().count(), 0);
    assert_eq!(app.session.controller.graph().midi_mappings().count(), 0);
    app.undo();
    app.process(600);
    assert!(app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot)).is_some());
    assert_eq!(app.session.controller.graph().assignments().count(), assignments);
    assert_eq!(app.session.controller.graph().midi_mappings().count(), mappings);
}
