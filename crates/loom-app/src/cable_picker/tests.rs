use crate::files::fake;
use crate::recovery::tests::TempDir;
use crate::ui_test::Ui;
use iced::keyboard::{Key, Modifiers, key::Named};
use loom_ui_core::Point;

/// Opens and chooses a browser row with keys, including with the pointer outside.
fn add(ui: &mut Ui, name: &str) {
    ui.tap(Key::Character("a".into()), Modifiers::empty());
    ui.redraw();
    assert!(ui.app().overlays.browser().is_some());
    ui.typewrite(name);
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    assert!(ui.app().overlays.browser().is_none());
}

/// Moves to a port by its visible label and picks it with Enter.
fn choose(ui: &mut Ui, name: &str) {
    let model = ui.app().menu_model().unwrap();
    let at = model
        .items
        .iter()
        .position(|row| row.label == name)
        .unwrap_or_else(|| panic!("{name}: {model:?}"));
    for _ in 0..at {
        ui.tap(Key::Named(Named::ArrowDown), Modifiers::empty());
    }
    assert_eq!(ui.a11y_tree().focused().name(), Some(name));
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
}

/// The pointer can stay outside the canvas while adding, patching, editing and saving.
#[test]
fn keys_add_connect_edit_and_save_with_the_pointer_outside() {
    let mut ui = Ui::with_app(crate::tests::app());
    ui.move_to(Point::new(4.0, 4.0));
    add(&mut ui, "constant");
    add(&mut ui, "svf");
    ui.tap(Key::Character("c".into()), Modifiers::empty());
    choose(&mut ui, "SVF In (input)");
    choose(&mut ui, "Constant Out (output)");
    let connections = ui.app().session.scene.cables().len();
    assert_eq!(connections, 1);

    // Undo and redo the cable as a whole, then change the selected tile's knob.
    ui.tap(Key::Character("z".into()), Modifiers::COMMAND);
    assert!(ui.app().session.scene.cables().is_empty());
    ui.tap(Key::Character("z".into()), Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(ui.app().session.scene.cables().len(), connections);
    let id = loom_core::ModuleId(*ui.app().session.selection.first().unwrap());
    let value = |ui: &Ui| {
        let node = ui.app().session.controller.graph().node(id).unwrap();
        node.params[node.descriptor.param_index("cutoff").unwrap()]
    };
    let before = value(&ui);
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    ui.tap(Key::Named(Named::ArrowUp), Modifiers::empty());
    assert!(value(&ui) > before);
    let messages = ui.tap(Key::Named(Named::Space), Modifiers::empty());
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, crate::Message::Canvas(loom_canvas::CanvasEvent::ToggleRun)))
    );

    let folder = TempDir::new("key-patch");
    let path = folder.0.join("keys.ron");
    fake::answer_save(Some(path.clone()));
    ui.tap(Key::Character("s".into()), Modifiers::COMMAND);
    let patch = loom_patch::Patch::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(patch.cables.len(), 1);
}

/// The port picker cancels cleanly and a disconnect is a single undo step.
#[test]
fn cable_picker_cancels_and_disconnects() {
    let mut ui = Ui::open("subtractive.ron");
    let filter = ui
        .app()
        .session
        .scene
        .modules()
        .iter()
        .find(|tile| tile.title == "SVF")
        .unwrap()
        .id;
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Select(vec![filter])));
    let count = ui.app().session.scene.cables().len();

    ui.send(crate::Message::ShowShortcuts);
    ui.tap(Key::Character("c".into()), Modifiers::empty());
    assert!(ui.app().overlays.cable_menu().is_none());
    ui.send(crate::Message::CloseHelp);

    ui.tap(Key::Character("c".into()), Modifiers::empty());
    let input = loom_canvas::PortRef {
        module: filter,
        output: false,
        index: 0,
    };
    let jump_label = ui.app().session.patch_map.structure.named_port(input);
    assert_eq!(
        ui.app().menu_model().unwrap().items[0].label,
        format!("{jump_label} (input)")
    );
    choose(&mut ui, "SVF In (input)");
    let structure = &ui.app().session.patch_map.structure;
    let wire = structure.cables.iter().find(|wire| wire.to == input).unwrap();
    let jump_label = structure.named_port(wire.from);
    assert!(
        ui.app()
            .menu_model()
            .unwrap()
            .items
            .iter()
            .any(|row| row.label == format!("{jump_label} (output)"))
    );
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(ui.app().session.scene.cables().len(), count);
    ui.tap(Key::Character("c".into()), Modifiers::empty());
    choose(&mut ui, "SVF In (input)");
    choose(&mut ui, "Disconnect cable");
    assert_eq!(ui.app().session.scene.cables().len(), count - 1);
    ui.tap(Key::Character("z".into()), Modifiers::COMMAND);
    assert_eq!(ui.app().session.scene.cables().len(), count);
}

/// A long port list distinguishes matching names and reveals the first row on the next stage.
#[test]
fn the_next_cable_picker_stage_starts_at_the_top() {
    let mut ports = String::new();
    for (first, direction) in [(1, "input"), (51, "output")] {
        for offset in 0..16 {
            let id = first + offset;
            ports.push_str(&format!(
                r#"PatchModule(id: {id}, kind: "loom.struct.{direction}@1", label: Some("Signal {offset}")),"#
            ));
        }
    }
    let mut ui = Ui::with_app(crate::tests::app_with_text(&format!(
        r#"Patch(version: 1, subpatches: {{"ports": SubPatch(
            metadata: Metadata(name: "Ports"), modules: [{ports}],
        )}}, modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("ports"))])"#
    )));
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Select(vec![1])));
    ui.tap(Key::Character("c".into()), Modifiers::empty());
    let model = ui.app().menu_model().unwrap();
    assert!(model.items.iter().any(|item| item.label == "Ports Signal 0 (input)"));
    assert!(model.items.iter().any(|item| item.label == "Ports Signal 0 (output)"));
    // The list's offset, read from the runtime's layout.
    let offset = |ui: &mut Ui| {
        let _ = ui.redraw();
        ui.runtime()
            .scroll_offset(&super::list_id())
            .expect("the list scrolls")
            .0
    };
    ui.tap(Key::Named(Named::End), Modifiers::empty());
    assert!(offset(&mut ui) > 0.0);
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    let model = ui.app().menu_model().unwrap();
    assert_eq!(model.highlight, Some(0));
    assert_eq!(offset(&mut ui), 0.0);
}

/// The SVF tile of the example patch selected, with C pressed: the picker
/// open on its ports, drawn.
fn open_on_the_filter() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    let filter = ui
        .app()
        .session
        .scene
        .modules()
        .iter()
        .find(|tile| tile.title == "SVF")
        .unwrap()
        .id;
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Select(vec![filter])));
    ui.tap(Key::Character("c".into()), Modifiers::empty());
    let _ = ui.redraw();
    assert!(ui.app().overlays.cable_menu().is_some(), "the picker is open");
    ui
}

/// A screen reader meets the picker as a menu named by its heading, with
/// the focus on its first row, which says its place among all the rows.
/// The heading gives no node of its own (the WAI-ARIA menu pattern).
#[test]
fn a_screen_reader_meets_the_picker_as_a_menu() {
    use accesskit::Role;
    let mut ui = open_on_the_filter();
    let rows = ui.app().menu_model().unwrap().items.len();
    let tree = ui.a11y_tree();
    let menu = tree.node_by_id(&super::cable_id());
    assert_eq!(menu.role(), Role::Menu);
    assert_eq!(menu.name(), Some("Choose a port"));
    let focused = tree.focused();
    assert_eq!(focused.role(), Role::MenuItem);
    assert_eq!(focused.name(), Some("SVF In (input)"));
    assert_eq!(focused.position_in_set(), Some(0));
    assert_eq!(menu.size_of_set(), Some(rows));
    assert_eq!(
        tree.names().iter().filter(|name| *name == "Choose a port").count(),
        1,
        "the heading is not heard twice"
    );
}

/// A press beside the picker closes it and reaches nothing under it, and
/// a press on its box keeps it open (ui-layer.md, section 5, rule 14).
#[test]
fn a_press_beside_the_picker_closes_it_and_reaches_nothing_under_it() {
    let mut ui = open_on_the_filter();
    let list = ui.runtime().bounds(&super::list_id()).expect("the list");
    ui.move_to(Point::new(list.x + 10.0, list.y - 10.0));
    let mut messages = ui.press();
    messages.extend(ui.release());
    assert!(
        ui.app().overlays.cable_menu().is_some(),
        "a press on the box keeps it open"
    );

    let selection = ui.app().session.selection.clone();
    ui.move_to(Point::new(list.x - 40.0, list.y + 10.0));
    let mut messages = ui.press();
    messages.extend(ui.release());
    let _ = ui.redraw();
    assert!(ui.app().overlays.cable_menu().is_none(), "a press beside it closes it");
    assert!(
        !messages.iter().any(|m| matches!(m, crate::Message::Canvas(_))),
        "the canvas never heard it: {messages:?}"
    );
    assert_eq!(ui.app().session.selection, selection);
}

/// A right press beside the picker closes it too, as a press outside any
/// menu does with either button, and opens no menu under it (ui-layer.md,
/// section 5, rule 14).
#[test]
fn a_right_press_beside_the_picker_closes_it_and_opens_nothing() {
    use iced::mouse::{Button, Event as Mouse};
    let mut ui = open_on_the_filter();
    let list = ui.runtime().bounds(&super::list_id()).expect("the list");
    ui.move_to(Point::new(list.x - 40.0, list.y + 10.0));
    let mut messages = ui.step(&[iced::Event::Mouse(Mouse::ButtonPressed(Button::Right))]);
    messages.extend(ui.step(&[iced::Event::Mouse(Mouse::ButtonReleased(Button::Right))]));
    let _ = ui.redraw();
    assert!(
        ui.app().overlays.cable_menu().is_none(),
        "a right press beside it closes it"
    );
    assert!(ui.app().menu_model().is_none(), "and opens no menu: {messages:?}");
    assert!(
        !messages.iter().any(|m| matches!(m, crate::Message::Canvas(_))),
        "the canvas never heard it: {messages:?}"
    );
}

/// Tab closes the picker, as it closes any menu (the WAI-ARIA menu
/// pattern; ui-layer.md, section 6, rule 8).
#[test]
fn tab_closes_the_picker() {
    let mut ui = open_on_the_filter();
    ui.tap(Key::Named(Named::Tab), Modifiers::empty());
    let _ = ui.redraw();
    assert!(ui.app().overlays.cable_menu().is_none());
}

/// The picker at a stage with no port to offer: inside a sub-patch that
/// holds one patch input, which has an output and no input, so nothing on
/// that level can take its cable. Its output is picked, by Enter.
fn open_with_no_port() -> Ui {
    let mut ui = Ui::with_app(crate::tests::app_with_text(
        r#"Patch(version: 1, subpatches: {"one": SubPatch(
            metadata: Metadata(name: "One"),
            modules: [PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("In"))],
        )}, modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("one"))])"#,
    ));
    ui.send(crate::Message::OpenTile(1));
    let _ = ui.redraw();
    let inner = ui.app().session.scene.modules().first().expect("the patch input").id;
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Select(vec![inner])));
    ui.tap(Key::Character("c".into()), Modifiers::empty());
    let _ = ui.redraw();
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    let _ = ui.redraw();
    ui
}

/// With no port to offer, the picker holds one item that says so: turned
/// off, it has the focus, offers no Click, and Enter picks nothing, while
/// Escape still closes the menu (the WAI-ARIA menu pattern).
#[test]
fn with_no_port_to_offer_one_turned_off_item_says_so() {
    use accesskit::{Action, Role};
    let mut ui = open_with_no_port();
    let none = super::list_id().with("none");
    assert_eq!(ui.runtime().focused(), Some(&none));
    let tree = ui.a11y_tree();
    let menu = tree.node_by_id(&super::cable_id());
    assert_eq!(menu.role(), Role::Menu);
    assert_eq!(menu.size_of_set(), Some(1));
    let item = tree.focused();
    assert_eq!(item.role(), Role::MenuItem);
    assert_eq!(item.name(), Some("No ports to connect"));
    assert!(item.is_disabled());
    assert!(item.offers(Action::Focus));
    assert!(!item.offers(Action::Click));
    let cables = ui.app().session.scene.cables().len();
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    ui.tap(Key::Named(Named::Space), Modifiers::empty());
    assert!(ui.app().overlays.cable_menu().is_some(), "Enter and Space pick nothing");
    assert_eq!(ui.app().session.scene.cables().len(), cables);
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert!(ui.app().overlays.cable_menu().is_none(), "Escape closes it");
}

/// The turned-off item draws in the plain role's off text colour, as a
/// turned-off control in Preferences does (visual-system.md).
#[test]
fn the_turned_off_item_draws_in_the_off_text_colour() {
    let mut ui = open_with_no_port();
    let colours = ui.painted_text_colours(loom_ui::Layer::Overlays);
    let (_, colour) = colours
        .iter()
        .find(|(text, _)| text == "No ports to connect")
        .expect("the item is painted");
    let off = crate::style::Role::Plain.looks().off.text.expect("an off text colour");
    assert_eq!(*colour, off);
    assert_ne!(off, crate::style::QUIET, "not just the quiet text");
}

/// Page Down and Page Up move the focus ten rows, stopping at the ends,
/// and never scroll the list away from it.
#[test]
fn page_keys_move_the_focus_by_a_page() {
    let mut ports = String::new();
    for offset in 0..24 {
        let id = 1 + offset;
        ports.push_str(&format!(
            r#"PatchModule(id: {id}, kind: "loom.struct.input@1", label: Some("Signal {offset}")),"#
        ));
    }
    let mut ui = Ui::with_app(crate::tests::app_with_text(&format!(
        r#"Patch(version: 1, subpatches: {{"ports": SubPatch(
            metadata: Metadata(name: "Ports"), modules: [{ports}],
        )}}, modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("ports"))])"#
    )));
    ui.send(crate::Message::Canvas(loom_canvas::CanvasEvent::Select(vec![1])));
    ui.tap(Key::Character("c".into()), Modifiers::empty());
    let _ = ui.redraw();
    let rows = ui.app().menu_model().unwrap().items.len();
    assert!(rows > 20, "a long list: {rows}");
    let focus_at = |ui: &Ui| {
        let model = ui.app().menu_model().unwrap();
        model
            .items
            .iter()
            .position(|item| Some(&item.id) == ui.runtime().focused())
    };
    ui.tap(Key::Named(Named::PageDown), Modifiers::empty());
    let _ = ui.redraw();
    assert_eq!(focus_at(&ui), Some(10));
    let (offset, _) = ui.runtime().scroll_offset(&super::list_id()).expect("the list scrolls");
    ui.tap(Key::Named(Named::ArrowDown), Modifiers::empty());
    let _ = ui.redraw();
    assert_eq!(focus_at(&ui), Some(11), "Down goes on from the page");
    for _ in 0..5 {
        ui.tap(Key::Named(Named::PageDown), Modifiers::empty());
    }
    let _ = ui.redraw();
    assert_eq!(focus_at(&ui), Some(rows - 1), "it stops at the end");
    ui.tap(Key::Named(Named::PageUp), Modifiers::empty());
    let _ = ui.redraw();
    assert_eq!(focus_at(&ui), Some(rows - 11));
    assert!(offset > 0.0, "the page scrolled the list to the focus");
}
