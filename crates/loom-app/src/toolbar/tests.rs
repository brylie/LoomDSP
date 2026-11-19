use super::*;
use loom_ui_core::UiId;

/// Every button has a name of its own, a UI ID that is `toolbar` and that
/// name, and a message for its press. A name the toolbar does not have
/// sends nothing.
#[test]
fn every_button_has_an_id_and_a_message() {
    let app = crate::tests::app();
    let model = app.toolbar_model();
    let mut names = std::collections::BTreeSet::new();
    for button in &model.buttons {
        assert!(names.insert(button.name), "{} is there twice", button.name);
        let id: UiId = button.id.parse().unwrap();
        assert_eq!(id, UiId::panel("toolbar").with(button.name));
        assert_eq!(id.to_string(), button.id, "the text reads back");
        assert!(message(button.name).is_some(), "{}", button.name);
        assert!(!button.label.is_empty());
    }
    for name in ["new", "open", "save", "save_as", "browse", "recent", "add_module"] {
        assert!(names.contains(name), "{name}");
    }
    for name in ["hold", "panic", "undo", "redo", "midi_learn", "preferences"] {
        assert!(names.contains(name), "{name}");
    }
    // At the top level there is no sub-patch to save to the library.
    assert!(!names.contains("save_to_library"));
    assert!(message("save_to_library").is_some());
    assert!(message("nonsense").is_none());
}

/// A button that cannot be pressed says why in its hover text, and a mode
/// button says whether its mode is on.
#[test]
fn a_disabled_button_says_why() {
    let mut app = crate::tests::app();
    let button = |app: &crate::App, name: &str| {
        app.toolbar_model()
            .buttons
            .into_iter()
            .find(|button| button.name == name)
            .unwrap()
    };
    let undo = button(&app, "undo");
    assert!(!undo.enabled);
    assert_eq!(undo.hover.as_deref(), Some("Nothing to undo"));
    assert_eq!(button(&app, "redo").hover.as_deref(), Some("Nothing to redo"));
    let hold = button(&app, "hold");
    assert_eq!(hold.enabled, hold.hover.is_none(), "disabled exactly when it says why");
    let recent = button(&app, "recent");
    assert_eq!(recent.enabled, !app.rig.recent.files().is_empty());
    assert!(recent.hover.is_some());

    assert_eq!(button(&app, "midi_learn").on, Some(false));
    let _ = app.send(Message::Learn(learn::Message::Toggle));
    assert_eq!(button(&app, "midi_learn").on, Some(true));
    assert_eq!(button(&app, "new").on, None);

    // One edit: a knob turned and let go.
    let module = app.session.controller.add_module("loom.filter.svf").unwrap().0;
    app.commit();
    let _ = app.send(Message::Canvas(loom_canvas::CanvasEvent::SetControl {
        module,
        widget: 0,
        position: 0.25,
        part: loom_canvas::ControlPart::Main,
    }));
    let _ = app.send(Message::SliderReleased);
    let undo = button(&app, "undo");
    assert!(undo.enabled);
    assert!(undo.hover.is_some_and(|hover| hover.starts_with("Undo: ")));
}

/// The snapshot carries the toolbar, with the fields a script reads and
/// without the ones only the window needs.
#[test]
fn the_snapshot_has_the_toolbar() {
    let app = crate::tests::app();
    let json = serde_json::to_value(app.ui_snapshot()).unwrap();
    let buttons = json["toolbar"]["buttons"].as_array().unwrap();
    assert_eq!(buttons.len(), app.toolbar_model().buttons.len());
    assert_eq!(buttons[0]["name"], "new");
    assert_eq!(buttons[0]["label"], "New");
    assert_eq!(buttons[0]["enabled"], true);
    assert!(buttons[0].get("id").is_none());
    assert!(buttons[0].get("group").is_none());
    assert!(buttons[0].get("hover").is_none(), "left out when there is none");
}

/// The toolbar's nodes as text: one line per node under `toolbar`, with
/// its role, name, description, whether it says it is on, whether it is
/// turned off, and whether it offers Click. Bounds and the Focus action
/// are left out: bounds move when the toolbar changes toolkit, and Focus
/// is new in row 2 (ui-layer.md, section 7, rule 22).
fn toolbar_nodes(ui: &mut crate::ui_test::Ui) -> String {
    let tree = ui.a11y_tree();
    let toolbar = tree.node_by_id(&UiId::panel("toolbar"));
    let mut lines = vec![format!("{:?} {:?}", toolbar.role(), toolbar.name())];
    for node in toolbar.children() {
        lines.push(format!(
            "  {:?} {:?} description={:?} toggled={:?} disabled={} click={}",
            node.role(),
            node.name(),
            // The shortcut key is Cmd on a Mac and Ctrl elsewhere: written
            // one way, so the snapshot holds on every platform.
            node.description().map(|text| text.replace("Ctrl+", "Cmd+")),
            node.toggled(),
            node.is_disabled(),
            node.offers(accesskit::Action::Click),
        ));
    }
    lines.join("\n")
}

/// The toolbar's nodes at the top level and inside a sub-patch match the
/// snapshot taken from the toolbar as Iced drew it, before row 2 of plan
/// 3.2 moved it: the move keeps every name, description, state, and
/// action. `LOOM_UPDATE_REFERENCES=1` writes the snapshot.
#[test]
fn the_toolbar_nodes_match_the_snapshot() {
    let mut top = crate::ui_test::Ui::open("subtractive.ron");
    let mut inside = crate::ui_test::Ui::open("nested-subtractive.ron");
    inside.send(crate::Message::OpenLevel(loom_patch::ModulePath::top(2)));
    let got = format!(
        "top level\n{}\n\ninside a sub-patch\n{}\n",
        toolbar_nodes(&mut top),
        toolbar_nodes(&mut inside)
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/toolbar/nodes.txt");
    if std::env::var_os("LOOM_UPDATE_REFERENCES").is_some() {
        std::fs::write(&path, &got).expect("the snapshot is written");
    }
    let want = std::fs::read_to_string(&path).expect("the snapshot exists: run with LOOM_UPDATE_REFERENCES=1");
    assert_eq!(got, want);
}

/// The toolbar is a named group of buttons, the window's first child. Each
/// button has its label, its hover text as the description, Click while
/// it works, and Focus too, since the toolbar is `loom-ui`'s (ui-layer.md,
/// section 7, rule 22, from row 2). A mode button says whether its mode is
/// on. Each node's bounds are where the runtime's layout put the button.
#[test]
fn the_toolbar_is_a_group_of_focusable_buttons() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let tree = ui.a11y_tree();
    let toolbar = tree.node_by_id(&toolbar_id());
    assert_eq!(toolbar.role(), accesskit::Role::Group);
    assert_eq!(toolbar.name(), Some("Toolbar"));
    assert_eq!(
        tree.root().children().first().and_then(|first| first.ui_id()),
        Some(&toolbar_id()),
        "the window's first child"
    );
    let model = ui.app().toolbar_model();
    assert_eq!(toolbar.children().len(), model.buttons.len());
    let scale = f64::from(ui.runtime().window().scale);
    for button in &model.buttons {
        let id = toolbar_id().with(button.name);
        let found = tree.node_by_id(&id);
        assert_eq!(found.role(), accesskit::Role::Button, "{}", button.name);
        assert_eq!(found.name(), Some(button.label));
        assert_eq!(found.description(), button.hover.as_deref());
        assert_eq!(
            found.offers(accesskit::Action::Click),
            button.enabled,
            "{}",
            button.name
        );
        assert_eq!(
            found.offers(accesskit::Action::Focus),
            button.enabled,
            "{}",
            button.name
        );
        assert_eq!(found.is_disabled(), !button.enabled, "{}", button.name);
        assert_eq!(found.toggled().is_some(), button.on.is_some(), "{}", button.name);
        let laid = ui.runtime().bounds(&id).expect("laid out");
        let bounds = found.bounds().expect("bounds");
        assert!((bounds.x0 - f64::from(laid.x) * scale).abs() < 1e-3, "{}", button.name);
        assert!((bounds.y0 - f64::from(laid.y) * scale).abs() < 1e-3, "{}", button.name);
    }
}

/// After an edit, the toolbar's Undo turns on and names the step, and the
/// tree the screen reader is sent says so: the toolbar's panel is built
/// again when its model changes.
#[test]
fn undo_follows_an_edit_in_the_tree() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let undo = toolbar_id().with("undo");
    assert!(ui.a11y_tree().node_by_id(&undo).is_disabled());
    let (module, widget) = ui
        .app()
        .ui_snapshot()
        .tiles
        .iter()
        .find_map(|tile| {
            let at = tile
                .widgets
                .iter()
                .position(|w| w.label.eq_ignore_ascii_case("cutoff"))?;
            Some((tile.id, at))
        })
        .expect("the patch has a cutoff knob");
    let _ = ui.deliver(crate::Message::Canvas(loom_canvas::CanvasEvent::SetControl {
        module,
        widget,
        position: 0.25,
        part: loom_canvas::ControlPart::Main,
    }));
    let _ = ui.deliver(crate::Message::SliderReleased);
    ui.redraw();
    let tree = ui.a11y_tree();
    let node = tree.node_by_id(&undo);
    assert!(!node.is_disabled(), "Undo works now");
    assert!(node.offers(accesskit::Action::Click));
    let model = ui.app().toolbar_model();
    let hover = model
        .buttons
        .iter()
        .find(|button| button.name == "undo")
        .and_then(|button| button.hover.as_deref());
    assert_eq!(node.description(), hover, "it names the step");
}

/// At most one primary button in the toolbar: MIDI learn, only while it
/// is on. Panic is danger (visual-system.md, rules 2 and 3).
#[test]
fn the_toolbar_has_one_primary_at_most() {
    use crate::style::Role;
    let mut app = crate::tests::app();
    for learning in [false, true] {
        if learning {
            let _ = app.send(crate::Message::Learn(crate::learn::Message::Toggle));
        }
        let model = app.toolbar_model();
        let primaries: Vec<&str> = model
            .buttons
            .iter()
            .filter(|button| role_of(button) == Role::Primary)
            .map(|button| button.name)
            .collect();
        let want: &[&str] = if learning { &["midi_learn"] } else { &[] };
        assert_eq!(primaries, want, "learning: {learning}");
        let panic = model.buttons.iter().find(|button| button.name == "panic").unwrap();
        assert_eq!(role_of(panic), Role::Danger);
    }
}

/// The root panel's slots and Iced's space above the canvas take the same
/// height, at the top level and inside a sub-patch, so the canvas starts
/// where the toolbar and the breadcrumb end.
#[test]
fn the_slots_and_the_iced_space_agree() {
    let mut top = crate::ui_test::Ui::open("nested-subtractive.ron");
    for inside in [false, true] {
        if inside {
            top.send(crate::Message::OpenLevel(loom_patch::ModulePath::top(2)));
        }
        top.lay_out_panels();
        let toolbar = top.runtime().bounds(&toolbar_id()).expect("laid out");
        let mut bottom = toolbar.y + toolbar.height;
        if inside {
            let crumbs = top
                .runtime()
                .bounds(&crate::breadcrumb::breadcrumb_id())
                .expect("inside, the breadcrumb shows");
            assert_eq!(crumbs.y, bottom, "under the toolbar");
            bottom = crumbs.y + crumbs.height;
        }
        assert_eq!(bottom, crate::style::top_height(inside), "inside: {inside}");
        let canvas = top.canvas();
        assert_eq!(canvas.bounds.y, bottom, "the canvas starts there, inside: {inside}");
    }
}

/// The cutoff knob's module and widget in `ui`'s patch.
fn cutoff_knob(ui: &crate::ui_test::Ui) -> (u32, usize) {
    ui.app()
        .ui_snapshot()
        .tiles
        .iter()
        .find_map(|tile| {
            let at = tile
                .widgets
                .iter()
                .position(|w| w.label.eq_ignore_ascii_case("cutoff"))?;
            Some((tile.id, at))
        })
        .expect("the patch has a cutoff knob")
}

/// Turns the cutoff knob to `position`, one undo step.
fn turn(ui: &mut crate::ui_test::Ui, position: f32) {
    let (module, widget) = cutoff_knob(ui);
    let _ = ui.deliver(crate::Message::Canvas(loom_canvas::CanvasEvent::SetControl {
        module,
        widget,
        position,
        part: loom_canvas::ControlPart::Main,
    }));
    let _ = ui.deliver(crate::Message::SliderReleased);
    ui.redraw();
}

/// The cutoff knob's position.
fn position(ui: &crate::ui_test::Ui) -> f32 {
    let (module, widget) = cutoff_knob(ui);
    let tile = ui
        .app()
        .ui_snapshot()
        .tiles
        .into_iter()
        .find(|t| t.id == module)
        .unwrap();
    tile.widgets[widget].position
}

/// Space on a focused toolbar button clicks it when the key comes up, once,
/// and goes no further: Run, which Space flips on the canvas, stays.
#[test]
fn space_on_a_toolbar_button_clicks_it_and_leaves_run() {
    use iced::keyboard::{Key, key::Named};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let learn = toolbar_id().with("midi_learn");
    ui.a11y_action(&learn, accesskit::Action::Focus);
    assert_eq!(ui.runtime().focused(), Some(&learn));
    let running = ui.app().session.controller.transport().running;
    let mut heard = ui.step(&[crate::ui_test::key_pressed(Key::Named(Named::Space), false)]);
    heard.extend(ui.step(&[crate::ui_test::key_released(
        Key::Named(Named::Space),
        Default::default(),
    )]));
    ui.redraw();
    assert!(ui.app().session.learn.is_on(), "the click turned learning on");
    assert_eq!(ui.app().session.controller.transport().running, running, "Run stays");
    assert!(
        !heard
            .iter()
            .any(|m| matches!(m, crate::Message::Canvas(loom_canvas::CanvasEvent::ToggleRun))),
        "the canvas never heard Space: {heard:?}"
    );
}

/// With Undo focused, the Undo shortcut undoes one step and does not also
/// click the button; Enter on it clicks it, which undoes one more step,
/// and runs no shortcut. Each is one step.
#[test]
fn undo_by_shortcut_and_by_enter_is_one_step_each() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let original = position(&ui);
    turn(&mut ui, 0.25);
    turn(&mut ui, 0.5);
    let undo = toolbar_id().with("undo");
    ui.a11y_action(&undo, accesskit::Action::Focus);
    assert_eq!(ui.runtime().focused(), Some(&undo));
    ui.tap(Key::Character("z".into()), Modifiers::COMMAND);
    ui.redraw();
    assert!((position(&ui) - 0.25).abs() < 1e-6, "the shortcut undid one step");
    assert_eq!(
        ui.runtime().focused(),
        Some(&undo),
        "Undo still works, so it keeps the focus"
    );
    ui.tap(Key::Named(Named::Enter), Modifiers::default());
    ui.redraw();
    assert!((position(&ui) - original).abs() < 1e-6, "Enter undid one more");
}

/// The app's shortcuts still reach the app while a toolbar button has
/// focus: Cmd+comma (Ctrl+comma elsewhere) opens Preferences (ui-layer.md,
/// section 6, rule 6: the focused element, then the app's shortcuts).
#[test]
fn shortcuts_work_while_a_toolbar_button_has_focus() {
    use iced::keyboard::{Key, Modifiers};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.a11y_action(&toolbar_id().with("new"), accesskit::Action::Focus);
    assert!(ui.app().preferences_model().is_none());
    ui.tap(Key::Character(",".into()), Modifiers::COMMAND);
    ui.redraw();
    assert!(ui.app().preferences_model().is_some(), "Preferences opened");
}

/// A click on a toolbar button takes no keyboard focus (ui-layer.md,
/// section 6, rule 3; Brylie's decision, 2026-11-19). When one of our
/// stops has the focus, it keeps it: Preferences, focused, stays focused
/// while a click turns MIDI learn on.
#[test]
fn a_click_leaves_our_focus_where_it_was() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let preferences = toolbar_id().with("preferences");
    ui.a11y_action(&preferences, accesskit::Action::Focus);
    assert_eq!(ui.runtime().focused(), Some(&preferences));
    ui.click_text("MIDI learn");
    ui.redraw();
    assert!(ui.app().session.learn.is_on(), "the click landed");
    assert_eq!(ui.runtime().focused(), Some(&preferences), "the focus stayed");
}

/// When the canvas has the keys, a click on a toolbar button takes them
/// from the focused knob, as a click on Iced's toolbar did on main, and
/// moves no focus of ours: the next arrow goes back to the canvas and
/// moves the selected tile.
#[test]
fn a_click_takes_the_keys_from_a_knob_back_to_the_canvas() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let (module, _) = cutoff_knob(&ui);
    let knob: UiId = format!("canvas/{module}/param/cutoff").parse().unwrap();
    ui.a11y_action(&knob, accesskit::Action::Focus);
    ui.redraw();
    assert!(ui.app().a11y.focus().is_some(), "the knob has the keys");
    // Panic: it changes nothing the arrows depend on, as learn mode would.
    let heard = ui.click_text("Panic");
    ui.redraw();
    assert!(
        heard.iter().any(|m| matches!(m, crate::Message::Panic)),
        "the click landed"
    );
    assert_eq!(ui.runtime().focused(), None, "no focus of ours");
    assert_eq!(ui.app().a11y.focus(), None, "the knob let go");
    let place = ui.app().session.positions[&module];
    ui.tap(Key::Named(Named::ArrowRight), Modifiers::default());
    ui.redraw();
    assert_ne!(ui.app().session.positions[&module], place, "the arrow moved the tile");
}

/// A focused button that turns off hands the focus to its panel's first
/// stop (rule 4's fallback, Brylie's decision of 2026-11-16): Undo,
/// focused from the keyboard, undoes the last step on Enter, turns off,
/// and the focus moves to New.
#[test]
fn a_focused_undo_that_turns_off_hands_the_focus_to_new() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    turn(&mut ui, 0.25);
    let undo = toolbar_id().with("undo");
    ui.a11y_action(&undo, accesskit::Action::Focus);
    assert_eq!(ui.runtime().focused(), Some(&undo));
    ui.tap(Key::Named(Named::Enter), Modifiers::default());
    ui.redraw();
    let model = ui.app().toolbar_model();
    assert!(
        !model.buttons.iter().find(|b| b.name == "undo").unwrap().enabled,
        "Undo turned off"
    );
    assert_eq!(
        ui.runtime().focused(),
        Some(&toolbar_id().with("new")),
        "the focus fell back"
    );
}
