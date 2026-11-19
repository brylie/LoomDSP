use crate::files::fake;
use crate::recovery::tests::TempDir;
use crate::ui_test::Ui;
use iced::keyboard::{Key, Modifiers, key::Named};
use loom_ui_core::Point;

/// Opens and chooses a browser row with keys, including with the pointer outside.
fn add(ui: &mut Ui, name: &str) {
    ui.tap(Key::Character("a".into()), Modifiers::empty());
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
    choose(&mut ui, "SVF In");
    choose(&mut ui, "Constant Out");
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
    choose(&mut ui, "SVF In");
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(ui.app().session.scene.cables().len(), count);
    ui.tap(Key::Character("c".into()), Modifiers::empty());
    choose(&mut ui, "SVF In");
    choose(&mut ui, "Disconnect cable");
    assert_eq!(ui.app().session.scene.cables().len(), count - 1);
    ui.tap(Key::Character("z".into()), Modifiers::COMMAND);
    assert_eq!(ui.app().session.scene.cables().len(), count);
}
