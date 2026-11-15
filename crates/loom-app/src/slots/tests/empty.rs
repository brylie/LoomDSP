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
/// copy is refused whole, with the limit's text and the Copies hint:
/// nothing is added, and the slot keeps its part (sub-patches.md, How modes
/// meet the rest and The module limit).
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
    assert_eq!(
        app.alerts.status(),
        format!(
            "Cannot swap Filter: the patch would have more than {} modules. Delete modules or lower Copies on Voice to make room.",
            count + 2
        ),
        "a swap between two parts says why, as a swap into an empty slot does"
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

/// Undo of a swap inside a stack before its fade is done fades back from
/// where it is: no module is added twice and no tile appears, each copy
/// keeps one SVF, one Ladder, and one fade until the fade finishes, and
/// then only the four SVFs are left (ADR-0044).
#[test]
fn undo_during_a_stack_fade_fades_back_without_extra_modules() {
    let mut app = app_with(STACKED);
    let slot = inside(&app, 9);
    let count = |app: &App, kind: &str| {
        app.session
            .controller
            .graph()
            .nodes()
            .filter(|(_, n)| n.descriptor.id == kind)
            .count()
    };
    let tiles = app.ui_snapshot().tiles.len();
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.ladder@1".into()),
    });
    assert_eq!(
        (
            count(&app, "loom.filter.svf"),
            count(&app, "loom.filter.ladder"),
            count(&app, "loom.struct.fade")
        ),
        (4, 4, 4)
    );
    app.process(64);
    app.undo();
    assert_eq!(app.session.hierarchy.fading_slots().len(), 1, "undo fades back");
    assert_eq!(
        (
            count(&app, "loom.filter.svf"),
            count(&app, "loom.filter.ladder"),
            count(&app, "loom.struct.fade")
        ),
        (4, 4, 4),
        "nothing added twice"
    );
    assert_eq!(app.ui_snapshot().tiles.len(), tiles, "no tile appears");
    app.process(600);
    assert!(app.session.hierarchy.fading_slots().is_empty(), "the fade finished");
    assert_eq!(
        (
            count(&app, "loom.filter.svf"),
            count(&app, "loom.filter.ladder"),
            count(&app, "loom.struct.fade")
        ),
        (4, 0, 0)
    );
}

/// [`STACKED`] with `copies` copies, its slot holding the SVF when `part`,
/// else empty.
fn stacked(copies: u32, part: bool) -> String {
    let text = STACKED.replace("copies: 4", &format!("copies: {copies}"));
    if part {
        text
    } else {
        text.replace("                PatchModule(id: 3, kind: \"loom.filter.svf@1\"),\n", "")
            .replace(", part: Some(3)", "")
    }
}

/// The app with `text` open and exactly `free` engine slots left.
fn app_with_room(text: &str, free: usize) -> App {
    let probe = app_with(text);
    let used = EngineConfig::default().max_modules - probe.session.controller.free_module_slots();
    let app = app_with_limit(text, used + free);
    assert_eq!(app.session.controller.free_module_slots(), free, "the room asked for");
    app
}

/// Takes every engine slot left with modules outside the history, after
/// the slots of removed modules are free again.
fn fill(app: &mut App) {
    loop {
        while app.session.controller.free_module_slots() > 0 {
            app.session.controller.add_module("loom.osc.noise").unwrap();
        }
        assert!(app.commit(), "{}", app.alerts.status());
        app.process(64);
        if app.session.controller.free_module_slots() == 0 {
            return;
        }
    }
}

/// How many modules of `kind` the graph holds.
fn count_of(app: &App, kind: &str) -> usize {
    app.session
        .controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == kind)
        .count()
}

/// The graph's modules and cables, to compare before and after a refusal.
fn graph_of(
    app: &App,
) -> (
    Vec<loom_core::ModuleId>,
    Vec<(loom_core::OutputRef, loom_core::InputRef)>,
) {
    let graph = app.session.controller.graph();
    (graph.nodes().map(|(id, _)| id).collect(), graph.cables().collect())
}

/// The limit's refusal text after `start`, with the Copies hint on Voice
/// when `in_stack` (sub-patches.md, The module limit).
fn no_room(app: &App, start: &str, in_stack: bool) -> String {
    let limit = app.session.controller.config().max_modules;
    let hint = if in_stack { " or lower Copies on Voice" } else { "" };
    format!("{start}: the patch would have more than {limit} modules. Delete modules{hint} to make room.")
}

/// The slot's fade now: how many other copies fade, and how many fade
/// modules the graph holds.
fn fading_now(app: &App, slot: u32) -> (usize, usize) {
    let copies = app
        .session
        .hierarchy
        .slot(loom_core::ModuleId(slot))
        .and_then(|s| s.fading.clone())
        .map_or(0, |f| f.copies.len());
    (copies, count_of(app, "loom.struct.fade"))
}

/// What feeds the input `port` of `module` in the app's graph.
fn fed_by(app: &App, module: loom_core::ModuleId, port: &str) -> Option<loom_core::OutputRef> {
    let controller = &app.session.controller;
    controller.graph().source_of(controller.input_ref(module, port).ok()?)
}

/// The one module of `kind` in the graph.
fn one_of(app: &App, kind: &str) -> loom_core::ModuleId {
    let ids: Vec<loom_core::ModuleId> = app
        .session
        .controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == kind)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids.len(), 1, "one {kind}");
    ids[0]
}

/// Checks a removal's fades in [`STACKED`]: copy k's fade takes that
/// copy's thru source, the sine, on `b`, and its `out` feeds copy k's own
/// input on the stack's Sum, copy 0's included. No fade takes over another
/// copy's input or the Audio Out, which still plays the Sum (ADR-0046).
fn check_removal_wiring(app: &App, slot: u32, what: &str) {
    let controller = &app.session.controller;
    let fading = app
        .session
        .hierarchy
        .slot(loom_core::ModuleId(slot))
        .and_then(|s| s.fading.clone())
        .expect("fading");
    let fades: Vec<loom_core::ModuleId> = fading
        .fades
        .iter()
        .chain(fading.copies.iter().flat_map(|c| c.fades.iter()))
        .map(|(_, id)| *id)
        .collect();
    assert_eq!(fades.len(), 4, "{what}: a fade per copy");
    let sine = controller.output_ref(one_of(app, "loom.osc.sine"), "out").unwrap();
    let sum = one_of(app, "loom.struct.sum");
    let inputs = controller.graph().node(sum).unwrap().descriptor.inputs;
    for (k, &fade) in fades.iter().enumerate() {
        assert_eq!(fed_by(app, fade, "b"), Some(sine), "{what}: copy {k}'s b is its thru");
        assert_eq!(
            fed_by(app, sum, inputs[k].name),
            Some(controller.output_ref(fade, "out").unwrap()),
            "{what}: copy {k}'s fade into its own input on the Sum"
        );
    }
    let out = one_of(app, "loom.io.audio-out");
    assert_eq!(
        fed_by(app, out, "in").map(|from| from.module),
        Some(sum),
        "{what}: the Sum still plays"
    );
}

/// Inside a stack of 4, all four directions fade every copy: Remove part,
/// undo of it, and redo of it; a swap into the empty slot, undo of it, and
/// redo of it. Each adds one fade per copy, and each ends with only the
/// thru or the four parts (sub-patches.md, Empty-slot swaps inside a
/// stack). The two removals through undo and redo wire each copy's fade
/// into that copy alone ([`check_removal_wiring`]).
#[test]
fn every_direction_of_an_empty_slot_swap_inside_a_stack_fades_every_copy() {
    let mut app = app_with(&stacked(4, true));
    let slot = inside(&app, 9);
    let check = |app: &mut App, what: &str, parts: usize| {
        assert_eq!(fading_now(app, slot), (3, 4), "{what}: a fade in every copy");
        app.process(600);
        assert_eq!(fading_now(app, slot), (0, 0), "{what}: the fades finished");
        assert_eq!(count_of(app, "loom.filter.svf"), parts, "{what}");
    };
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Empty,
    });
    check(&mut app, "Remove part", 0);
    app.undo();
    check(&mut app, "undo of Remove part", 4);
    app.redo();
    check_removal_wiring(&app, slot, "redo of Remove part");
    check(&mut app, "redo of Remove part", 0);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.svf@1".into()),
    });
    check(&mut app, "a swap in", 4);
    app.undo();
    check_removal_wiring(&app, slot, "undo of the swap in");
    check(&mut app, "undo of the swap in", 0);
    app.redo();
    check(&mut app, "redo of the swap in", 4);
}

/// At the module limit, a swap into an empty slot is refused whole, inside
/// a stack and outside one: nothing is added, the cables are as before,
/// the undo history is kept, the inspector's Part list still reports no
/// part, and the status says why, with the Copies hint only inside a stack
/// (sub-patches.md, The module limit).
#[test]
fn a_swap_into_an_empty_slot_at_the_limit_is_refused_whole() {
    for (copies, in_stack) in [(4, true), (1, false)] {
        // A part and a fade per copy do not fit in one slot less.
        let mut ui = Ui::with_app(app_with_room(&stacked(copies, false), 2 * copies as usize - 1));
        let slot = inside(ui.app(), 9);
        let before = graph_of(ui.app());
        let undo = ui.app().undo_hint();
        let _ = ui.app_mut().send(Message::SwapSlot {
            slot,
            to: SwapTarget::Module("loom.filter.svf@1".into()),
        });
        assert_eq!(
            ui.app().alerts.status(),
            no_room(ui.app(), "Cannot swap Filter", in_stack)
        );
        assert_eq!(graph_of(ui.app()), before, "nothing is kept");
        assert_eq!(ui.app().undo_hint(), undo, "the history is as it was");
        assert!(ui.app().session.hierarchy.fading_slots().is_empty());
        ui.app_mut().session.selection = std::collections::BTreeSet::from([slot]);
        ui.redraw();
        assert_eq!(ui.a11y_tree().node("Part").value(), Some("No part"));
    }
}

/// At the module limit, Remove part still works, inside a stack and
/// outside one: it adds no fade module, and each copy's inputs ramp over
/// 5 ms by live-patching rule 9 instead. Undo of it, which adds the part
/// back, is then refused at the limit with the history kept, the
/// inspector's Part list still reports no part, and a screen reader meets
/// the refusal as a status (sub-patches.md, The module limit).
#[test]
fn remove_part_at_the_limit_ramps_and_its_undo_is_refused() {
    for (copies, in_stack) in [(4, true), (1, false)] {
        let mut ui = Ui::with_app(app_with_room(&stacked(copies, true), 0));
        let app = ui.app_mut();
        let slot = inside(app, 9);
        let _ = app.send(Message::SwapSlot {
            slot,
            to: SwapTarget::Empty,
        });
        assert_eq!(app.alerts.status(), "Removed SVF from Filter.");
        assert_eq!(fading_now(app, slot), (0, 0), "no fade module");
        assert_eq!(count_of(app, "loom.filter.svf"), 0, "every copy's part went");
        assert!(
            app.session.controller.last_report().cable_fades >= copies as usize,
            "a 5 ms ramp in every copy"
        );
        fill(app);
        app.undo();
        assert_eq!(
            app.alerts.status(),
            no_room(app, "Cannot undo remove part from Filter", in_stack)
        );
        assert_eq!(app.undo_hint(), Ok("Undo: remove part from Filter".to_string()));
        assert_eq!(app.session.hierarchy.part_of_slot(loom_core::ModuleId(slot)), None);
        ui.redraw();
        let status_id = crate::a11y::tree::fixed_id(crate::bars::STATUS_ID);
        let role = ui.a11y_tree().node_by_id(&status_id).role();
        assert_eq!(role, accesskit::Role::Status, "the refusal is a status");
        // `fill` adds modules behind the hierarchy, and one can take the
        // slot tile's id, so they go before the inspector is read.
        let app = ui.app_mut();
        let filler: Vec<_> = app
            .session
            .controller
            .graph()
            .nodes()
            .filter(|(_, n)| n.descriptor.id == "loom.osc.noise")
            .map(|(id, _)| id)
            .collect();
        for id in filler {
            app.session.controller.remove_module(id).unwrap();
        }
        assert!(app.commit(), "{}", app.alerts.status());
        app.session.selection = std::collections::BTreeSet::from([slot]);
        ui.redraw();
        assert_eq!(ui.a11y_tree().node("Part").value(), Some("No part"));
    }
}

/// At the module limit, undo of a swap into an empty slot still works,
/// inside a stack and outside one, with the 5 ms ramp and no fade module;
/// redo of the swap in is then refused at the limit, with the history kept
/// (sub-patches.md, The module limit).
#[test]
fn undo_of_a_swap_in_at_the_limit_ramps_and_its_redo_is_refused() {
    for (copies, in_stack) in [(4, true), (1, false)] {
        let mut app = app_with_room(&stacked(copies, false), 2 * copies as usize);
        let slot = inside(&app, 9);
        let _ = app.send(Message::SwapSlot {
            slot,
            to: SwapTarget::Module("loom.filter.svf@1".into()),
        });
        assert_eq!(app.alerts.status(), "Swapped Filter to SVF.");
        app.process(600);
        fill(&mut app);
        app.undo();
        assert_eq!(app.alerts.status(), "Undo: swap Filter");
        assert_eq!(fading_now(&app, slot), (0, 0), "no fade module");
        assert_eq!(count_of(&app, "loom.filter.svf"), 0);
        assert!(
            app.session.controller.last_report().cable_fades >= copies as usize,
            "a 5 ms ramp in every copy"
        );
        fill(&mut app);
        app.redo();
        assert_eq!(app.alerts.status(), no_room(&app, "Cannot redo swap Filter", in_stack));
        assert_eq!(app.redo_hint(), Ok("Redo: swap Filter".to_string()));
        assert_eq!(count_of(&app, "loom.filter.svf"), 0, "nothing is kept");
    }
}

/// A sine as both sides of a stereo signal into a Voice stacked four times
/// whose Filter slot holds an SVF (when `part`), so every copy's part, and
/// every fade, runs once per side.
fn stereo_stacked(part: bool) -> String {
    stacked(4, part)
        .replace(
            "PatchModule(id: 3, kind: \"loom.io.audio-out@1\"),",
            "PatchModule(id: 3, kind: \"loom.io.audio-out@1\"),\n        PatchModule(id: 5, kind: \"loom.mix.join@1\"),",
        )
        .replace(
            "PatchCable(from: (1, \"out\"), to: (2, \"in\")),",
            "PatchCable(from: (1, \"out\"), to: (5, \"left\")),\n        PatchCable(from: (1, \"out\"), to: (5, \"right\")),\n        PatchCable(from: (5, \"out\"), to: (2, \"in\")),",
        )
}

/// Remove part inside a stack on a stereo signal, with room for one fade
/// per copy but not for their right sides, succeeds with the 5 ms ramp and
/// no fade module, and the commit does not fail: the room is measured
/// after the sync, by the commit's own rule. A swap in on the same stereo
/// signal with the same room is refused, and the graph is as before
/// (sub-patches.md, The module limit).
#[test]
fn remove_part_on_a_stereo_signal_at_the_limit_ramps() {
    let mut app = app_with_room(&stereo_stacked(true), 4);
    let slot = inside(&app, 9);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Empty,
    });
    assert_eq!(app.alerts.status(), "Removed SVF from Filter.");
    assert_eq!(fading_now(&app, slot), (0, 0), "no fade module");
    assert_eq!(count_of(&app, "loom.filter.svf"), 0);
    assert!(app.session.controller.last_report().cable_fades > 0, "the 5 ms ramp");

    let mut app = app_with_room(&stereo_stacked(false), 4);
    let slot = inside(&app, 9);
    let before = graph_of(&app);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.filter.svf@1".into()),
    });
    assert_eq!(app.alerts.status(), no_room(&app, "Cannot swap Filter", true));
    assert_eq!(graph_of(&app), before, "nothing is kept");
}

/// A sine into a Voice stacked four times whose Effect slot holds Dynamics
/// (when `part`), a module with stereo ports: on a mono signal each part
/// takes one slot, but its output is stereo, so each fade runs once per
/// side.
fn stereo_effect(part: bool) -> String {
    let text = stacked(4, true)
        .replace("loom.role.filter@1", "loom.role.effect@1")
        .replace("loom.filter.svf@1", "loom.dyn.dynamics@1");
    if part {
        text
    } else {
        text.replace(
            "                PatchModule(id: 3, kind: \"loom.dyn.dynamics@1\"),\n",
            "",
        )
        .replace(", part: Some(3)", "")
    }
}

/// Dynamics in an Effect slot on a mono signal, in a stack of 4, with room
/// for 4 fade slots but not 8: Remove part succeeds, ramps over 5 ms, adds
/// no fade module, and the commit does not fail (sub-patches.md, The
/// module limit).
#[test]
fn remove_part_with_a_stereo_effect_on_a_mono_signal_at_the_limit() {
    let mut app = app_with_room(&stereo_effect(true), 4);
    let slot = inside(&app, 9);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Empty,
    });
    assert_eq!(app.alerts.status(), "Removed Dynamics from Effect.");
    assert_eq!(fading_now(&app, slot), (0, 0), "no fade module");
    assert_eq!(count_of(&app, "loom.dyn.dynamics"), 0);
    assert!(app.session.controller.last_report().cable_fades > 0, "the 5 ms ramp");
}

/// The twin: a swap of Dynamics into the empty Effect slot, with room for
/// the parts and the fades by count but not for the fades' right sides, is
/// refused with the limit's text after the sync measures it, and nothing
/// is kept (sub-patches.md, The module limit).
#[test]
fn a_swap_in_of_a_stereo_effect_at_the_limit_is_refused() {
    let mut app = app_with_room(&stereo_effect(false), 8);
    let slot = inside(&app, 9);
    let before = graph_of(&app);
    let _ = app.send(Message::SwapSlot {
        slot,
        to: SwapTarget::Module("loom.dyn.dynamics@1".into()),
    });
    assert_eq!(app.alerts.status(), no_room(&app, "Cannot swap Effect", true));
    assert_eq!(graph_of(&app), before, "nothing is kept");
    assert_eq!(
        app.session.controller.free_module_slots(),
        8,
        "every slot is free again"
    );
}

/// A sine through two Filter slots in a row, each holding an SVF.
const TWO_SLOTS: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Two slots"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.filter.svf@1"),
        PatchModule(id: 3, kind: "loom.filter.svf@1"),
        PatchModule(id: 4, kind: "loom.io.audio-out@1"),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(2)))),
        PatchModule(id: 10, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.filter@1", part: Some(3)))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (10, "in")),
        PatchCable(from: (10, "out"), to: (4, "in")),
    ],
)"#;

/// Remove part while another slot's fade runs, with room for the new fade
/// only once the running fade's modules are freed: the running fade is
/// finished and committed first, so the removal crossfades (sub-patches.md,
/// The module limit).
#[test]
fn remove_part_at_the_limit_during_a_fade() {
    let mut app = app_with_room(TWO_SLOTS, 2);
    let _ = app.send(Message::SwapSlot {
        slot: 9,
        to: SwapTarget::Module("loom.filter.ladder@1".into()),
    });
    assert_eq!(app.session.hierarchy.fading_slots().len(), 1);
    assert_eq!(app.session.controller.free_module_slots(), 0, "no room while it fades");
    let _ = app.send(Message::SwapSlot {
        slot: 10,
        to: SwapTarget::Empty,
    });
    assert_eq!(app.alerts.status(), "Removed SVF from Filter.");
    let fading: Vec<_> = app
        .session
        .hierarchy
        .fading_slots()
        .into_iter()
        .map(|(slot, _)| slot.0)
        .collect();
    assert_eq!(fading, [10], "the first fade finished, the removal crossfades");
    assert_eq!(count_of(&app, "loom.struct.fade"), 1);
}
