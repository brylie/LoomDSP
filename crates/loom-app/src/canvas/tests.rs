//! Tests for the patch canvas as a custom element (`canvas.rs`): its panel
//! and UI ID, and its sync with the app after each update.

use crate::Message;
use crate::ui_test::Ui;
use loom_canvas::{CanvasEvent, CanvasInput};

/// The canvas is one custom element that fills its own panel and takes
/// the panel's UI ID, `canvas`, so its parts keep `canvas/…` UI IDs.
#[test]
fn the_canvas_is_its_own_panel_with_the_panels_ui_id() {
    let mut ui = Ui::open("subtractive.ron");
    ui.lay_out_panels();
    let bounds = ui
        .runtime()
        .bounds(&super::canvas_id())
        .expect("the canvas is laid out");
    assert!(bounds.width > 0.0 && bounds.height > 0.0, "{bounds:?}");
    assert!(
        ui.runtime()
            .custom::<super::CanvasElement>(&super::canvas_id())
            .is_some(),
        "its state is the runtime's, by the panel's UI ID"
    );
}

/// The canvas's messages in `log`.
fn canvas_messages(log: &[Message]) -> Vec<&CanvasEvent> {
    log.iter()
        .filter_map(|message| match message {
            Message::Canvas(event) => Some(event),
            _ => None,
        })
        .collect()
}

/// A named change to the app, for the convergence test.
type Change = (&'static str, fn(&mut Ui));

/// The canvas syncs with the app after each update, as it did on Iced's
/// redraw. From each state below, the sync may send messages, and their
/// updates sync it again; a frame later, with no update between, a sync
/// sends nothing, so the extra ticks end (the convergence the sync relies
/// on). The harness gives up past a cap of turns, so a sync that never
/// ends fails here rather than hanging.
#[test]
fn the_canvas_sync_ends_from_every_state() {
    let mut ui = Ui::open("subtractive.ron");
    let changes: [Change; 5] = [
        ("a new frame key", |ui| ui.app_mut().frame_key += 1),
        ("the browser key", |ui| ui.send(Message::OpenBrowser)),
        ("a focus request", |ui| {
            ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
            ui.app_mut().a11y.request_control(Some((4, 0)));
        }),
        ("a selection change", |ui| {
            ui.send(Message::Canvas(CanvasEvent::Select(vec![3])))
        }),
        ("a pop-up holding input", |ui| {
            ui.send(Message::Preferences(crate::settings::Message::Open))
        }),
    ];
    for (what, change) in changes {
        change(&mut ui);
        let _ = ui.redraw();
        let again = ui.redraw();
        assert!(
            canvas_messages(&again).is_empty(),
            "{what}: a second sync with no update sends nothing: {again:?}"
        );
        ui.send(Message::Preferences(crate::settings::Message::Close));
        let _ = ui.redraw();
    }
}

/// A `test.panel` module added to an open patch and selected, with the
/// index of each named widget, for the focus-loss tests below.
fn panel_module(ui: &mut Ui) -> (u32, impl Fn(&str) -> usize + use<>) {
    let id = ui
        .app_mut()
        .session
        .controller
        .add_module("test.panel")
        .expect("the test modules are registered in tests")
        .0;
    ui.app_mut().commit();
    ui.send(Message::Canvas(CanvasEvent::Select(vec![id])));
    let _ = ui.redraw();
    let widgets: Vec<_> = ui
        .app()
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(id))
        .unwrap()
        .descriptor
        .panel
        .widgets
        .to_vec();
    let widget = move |name: &str| {
        widgets
            .iter()
            .position(|w| w.binds(name))
            .unwrap_or_else(|| panic!("no widget bound to {name}"))
    };
    (id, widget)
}

/// Gives the canvas control `widget` of `module` the keys, as Tab would.
fn focus_control(ui: &mut Ui, module: u32, widget: usize) {
    ui.app_mut().a11y.request_control(Some((module, widget)));
    let _ = ui.redraw();
    assert_eq!(
        canvas_input(ui, |input| input.focused_control()),
        Some((module, widget))
    );
}

/// Reads the canvas element's input state.
fn canvas_input<T>(ui: &Ui, read: impl FnOnce(&CanvasInput) -> T) -> T {
    read(
        ui.runtime()
            .custom::<super::CanvasElement>(&super::canvas_id())
            .expect("the canvas element")
            .input(),
    )
}

/// The window losing the keys, as the window's event gives it.
fn lose_the_window(ui: &mut Ui) -> Vec<Message> {
    ui.step(&[crate::ui_test::Input::unfocused()])
}

/// The window losing the keys while Space holds a canvas button releases
/// it: no key-up comes after the window loses focus. The app counts the
/// loss, and the canvas hears it on its next sync, in the same turn. The
/// later key-up releases nothing again.
#[test]
fn losing_the_window_releases_a_held_space() {
    let mut ui = Ui::open("sine.ron");
    let (id, widget) = panel_module(&mut ui);
    let gate = widget("gate");
    focus_control(&mut ui, id, gate);
    let pressed = ui.key_down(loom_ui_core::Key::Space, false);
    assert!(
        canvas_messages(&pressed).contains(&&CanvasEvent::ButtonPressed {
            module: id,
            widget: gate
        }),
        "{pressed:?}"
    );
    let lost = lose_the_window(&mut ui);
    assert!(
        canvas_messages(&lost).contains(&&CanvasEvent::ButtonReleased {
            module: id,
            widget: gate
        }),
        "{lost:?}"
    );
    let up = ui.key_up(loom_ui_core::Key::Space);
    assert!(
        !canvas_messages(&up)
            .iter()
            .any(|event| matches!(event, CanvasEvent::ButtonReleased { .. })),
        "{up:?}"
    );
}

/// The window losing the keys closes an open select list and ends a typed
/// value field, with no edit from either.
#[test]
fn losing_the_window_closes_a_list_and_ends_a_typed_field() {
    use loom_ui_core::{Key, Modifiers};
    let mut ui = Ui::open("sine.ron");
    let (id, widget) = panel_module(&mut ui);
    let steps = ui.app().session.edits.history.len();

    focus_control(&mut ui, id, widget("long"));
    ui.tap(Key::Enter, Modifiers::NONE);
    assert!(canvas_input(&ui, CanvasInput::list_open), "Enter opens the list");
    let _ = lose_the_window(&mut ui);
    assert!(!canvas_input(&ui, CanvasInput::list_open), "the loss closed it");

    focus_control(&mut ui, id, widget("cutoff"));
    ui.step(&[crate::ui_test::key_press(
        Key::Character('5'),
        Modifiers::NONE,
        Some("5"),
        false,
    )]);
    assert!(ui.places().typed.is_some(), "a digit opens the field");
    let _ = lose_the_window(&mut ui);
    assert!(ui.places().typed.is_none(), "the loss ended it");
    assert_eq!(ui.app().session.edits.history.len(), steps, "and nothing was edited");
}

/// A group of arrow-key steps closes in the update after the canvas says
/// it is due: the canvas's deadline wakes it once the group's time has
/// passed, it sends `EditGroupDue`, and that update closes the group as
/// one undo step. The canvas does not wake again for it.
#[test]
fn the_edit_group_closes_in_the_update_after_it_is_due() {
    use crate::edits::ArrowGroup;
    use loom_ui_core::{Key, Modifiers};
    let mut ui = Ui::open("sine.ron");
    let (id, widget) = panel_module(&mut ui);
    focus_control(&mut ui, id, widget("cutoff"));
    let steps = ui.app().session.edits.history.len();
    ui.tap(Key::Up, Modifiers::NONE);
    assert!(
        matches!(ui.app().session.edits.arrow_group(), ArrowGroup::Released { .. }),
        "the group waits for another step"
    );
    let woken = ui.wake_after(crate::edits::ARROW_GROUP + std::time::Duration::from_millis(50));
    assert!(
        canvas_messages(&woken)
            .iter()
            .any(|event| matches!(event, CanvasEvent::EditGroupDue { .. })),
        "{woken:?}"
    );
    assert_eq!(ui.app().session.edits.arrow_group(), ArrowGroup::Closed);
    assert_eq!(ui.app().session.edits.history.len(), steps + 1, "one undo step");
    let again = ui.wake_after(crate::edits::ARROW_GROUP);
    assert!(
        !canvas_messages(&again)
            .iter()
            .any(|event| matches!(event, CanvasEvent::EditGroupDue { .. })),
        "no second wake for a closed group: {again:?}"
    );
}

/// Opening a sub-patch frames it at the size the canvas is shown at. The
/// same update shows the breadcrumb, which takes room from the canvas, and
/// the canvas's sync runs before the layout that resizes it; the placement
/// after that layout frames it again, as Iced's draw after the layout did.
#[test]
fn opening_a_sub_patch_frames_it_at_the_size_it_is_shown() {
    let mut ui = Ui::open("nested-subtractive.ron");
    let before = ui.canvas().bounds;
    ui.send(Message::OpenLevel(loom_patch::ModulePath::top(2)));
    let _ = ui.redraw();
    let view = ui.canvas();
    assert!(view.bounds.height < before.height, "the breadcrumb took room");
    let mut framed = loom_canvas::Camera::default();
    framed.frame_readable(
        ui.app().session.scene.bounds().expect("a level with tiles"),
        view.bounds.size(),
        loom_canvas::READABLE_ZOOM,
    );
    assert_eq!(view.camera, framed);
}

/// An update draws a frame, even when it changes nothing a panel shows:
/// the canvas's lights, meters, and traces change with no panel, and Iced
/// drew after every update. A wake that runs no update asks for none.
#[test]
fn an_update_draws_a_frame_for_the_canvas() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let before = ui.draws_asked();
    let _ = ui.wake_after(std::time::Duration::ZERO);
    assert_eq!(ui.draws_asked(), before, "no update, no frame");
    ui.send(Message::Tick);
    assert!(ui.draws_asked() > before, "the update asked for a frame");
}

/// The measuring animation moves the canvas's camera on animation frames
/// and sends the app nothing until it ends, so `--bench` measures drawing:
/// no frame and no redraw while it runs sends a message, and the camera
/// moves. Turning it off puts the camera back, and the canvas reports what
/// is on screen then.
#[test]
fn the_measuring_animation_sends_nothing_until_it_ends() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let base = ui.canvas().camera;
    ui.send(Message::Animate(true));
    let start = ui.now();
    for step in 1..=10 {
        let at = start + std::time::Duration::from_millis(16 * step);
        let sent = ui.animation_frame(at);
        assert!(canvas_messages(&sent).is_empty(), "frame {step}: {sent:?}");
        let drawn = ui.redraw_at(at);
        assert!(canvas_messages(&drawn).is_empty(), "redraw {step}: {drawn:?}");
    }
    assert_ne!(ui.canvas().camera, base, "the camera moved");
    ui.send(Message::Animate(false));
    let _ = ui.redraw();
    assert_eq!(ui.canvas().camera, base, "the end put the camera back");
}

/// Hover text shows on an idle patch: the pointer rests on a control, no
/// update runs, and the canvas's own deadline wakes it after the delay.
/// That tick asks for a frame, so the text is drawn.
#[test]
fn hover_text_shows_on_an_idle_patch() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let at = ui.control(4, "cutoff");
    ui.move_to(at);
    let _ = ui.redraw();
    assert_eq!(canvas_input(&ui, CanvasInput::hover_text_shown), None, "not yet");
    let before = ui.draws_asked();
    let _ = ui.wake_after(loom_canvas::HOVER_DELAY + std::time::Duration::from_millis(10));
    assert!(
        canvas_input(&ui, CanvasInput::hover_text_shown).is_some(),
        "the text shows"
    );
    assert!(ui.draws_asked() > before, "and its tick asked for a frame");
}

/// The toolbar's New button with the focus, and the pointer over the
/// middle of the canvas.
fn focused_on_new() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let new = crate::toolbar::toolbar_id().with("new");
    ui.app_mut().effect(loom_ui::Effects::focus(new.clone()));
    ui.send(Message::Tick);
    assert_eq!(ui.runtime().focused(), Some(&new));
    let centre = ui.canvas().bounds.center();
    ui.move_to(centre);
    ui
}

/// The canvas's window-wide shortcuts work while a part of the runtime's
/// has the focus, as they did on Iced, where the canvas heard every key:
/// Cmd+V pastes at the pointer, Cmd+0 resets the zoom, and Cmd+A selects
/// every tile, with the toolbar's New button focused.
#[test]
fn the_canvas_shortcuts_work_with_the_focus_elsewhere() {
    let press = |ui: &mut Ui, c: char| {
        ui.step(&[crate::ui_test::key_press(
            loom_ui_core::Key::Character(c),
            loom_ui_core::Modifiers::platform_shortcut(),
            Some(&c.to_string()),
            false,
        )])
    };
    let mut ui = focused_on_new();
    let pasted = press(&mut ui, 'v');
    assert!(
        canvas_messages(&pasted)
            .iter()
            .any(|event| matches!(event, CanvasEvent::Paste { .. })),
        "Cmd+V pastes: {pasted:?}"
    );

    let mut ui = focused_on_new();
    assert_ne!(ui.canvas().camera.zoom, 1.0, "the patch opens at another zoom");
    let _ = press(&mut ui, '0');
    assert_eq!(ui.canvas().camera.zoom, 1.0, "Cmd+0 resets the zoom");

    let mut ui = focused_on_new();
    let _ = press(&mut ui, 'a');
    let mut all: Vec<u32> = ui.app().session.scene.modules().iter().map(|tile| tile.id).collect();
    all.sort_unstable();
    let mut selected = ui.app().ui_snapshot().selection;
    selected.sort_unstable();
    assert_eq!(selected, all, "Cmd+A selects every tile");
    assert_eq!(
        ui.runtime().focused(),
        Some(&crate::toolbar::toolbar_id().with("new")),
        "and the focus stays where it was"
    );
}

/// Help opens from the canvas's typed value field, which takes every
/// other key: F1, and Cmd or Ctrl with /, are heard first, whoever has the
/// keys (`App::always_key`).
#[test]
fn help_opens_while_typing_a_tile_value() {
    use loom_ui_core::{Key, Modifiers};
    for (key, modifiers, text) in [
        (Key::F1, Modifiers::NONE, None),
        (Key::Character('/'), Modifiers::platform_shortcut(), Some("/")),
    ] {
        let mut ui = Ui::open("subtractive.ron");
        let _ = ui.redraw();
        focus_control(&mut ui, 4, 0);
        ui.step(&[crate::ui_test::key_press(
            Key::Character('5'),
            Modifiers::NONE,
            Some("5"),
            false,
        )]);
        assert!(ui.places().typed.is_some(), "the typed field is open");
        ui.step(&[crate::ui_test::key_press(key, modifiers, text, false)]);
        assert!(ui.app().overlays.help.is_some(), "{key:?} opened help");
    }
}

/// A scripted pointer step can aim at the canvas's open typed field by its
/// UI ID, as `locate` can: the step finds it, with no error.
#[test]
fn a_scripted_step_reaches_the_typed_field() {
    use crate::pointer::{Aim, Step};
    use loom_ui_core::{Key, Modifiers};
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    let cutoff = ui
        .app()
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(4))
        .unwrap()
        .descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.binds("cutoff"))
        .expect("the filter's cutoff knob");
    focus_control(&mut ui, 4, cutoff);
    ui.step(&[crate::ui_test::key_press(
        Key::Character('5'),
        Modifiers::NONE,
        Some("5"),
        false,
    )]);
    let typed = ui.places().typed.expect("the typed field is open");
    let serial = ui.app().pointer.push(vec![Step::MoveTo(Aim::Target(
        "canvas/4/param/cutoff/field".to_string(),
    ))]);
    for _ in 0..4 {
        let _ = ui.redraw();
    }
    assert_eq!(ui.app().pointer.take_error(serial), None, "the step found the field");
    let at = ui.app().pointer.position().expect("the script moved the pointer");
    assert!(typed.bounds.contains(at), "{at:?} is in {:?}", typed.bounds);
}

/// A press outside the canvas takes the keys from a canvas control, as on
/// Iced, where the canvas heard every press: in the press's own step the
/// app counts it, then the canvas lets go (`FocusChanged(None)`), before
/// the release. A digit typed after it opens no typed field on the
/// control left behind, and a later sync replays nothing.
#[test]
fn a_press_in_the_inspector_takes_the_keys_from_a_canvas_control() {
    use loom_ui_core::{Key, Modifiers};
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.redraw();
    ui.send(Message::Inspector(crate::inspector::Message::SelectModule(4)));
    let _ = ui.redraw();
    focus_control(&mut ui, 4, 0);
    let row = ui.text("Cutoff").expect("the inspector's Cutoff row").center();
    let _ = ui.move_to(row);
    let pressed = ui.press();
    let at = |wanted: fn(&Message) -> bool| pressed.iter().position(wanted);
    let counted = at(|m| matches!(m, Message::PressedElsewhere)).expect("the press was counted");
    let let_go = at(|m| matches!(m, Message::Canvas(CanvasEvent::FocusChanged(None)))).expect("the canvas let go");
    assert!(counted < let_go, "{pressed:?}");
    let _ = ui.release();
    assert_eq!(canvas_input(&ui, CanvasInput::focused_control), None);

    ui.step(&[crate::ui_test::key_press(
        Key::Character('5'),
        Modifiers::NONE,
        Some("5"),
        false,
    )]);
    assert!(ui.places().typed.is_none(), "no typed field opened");
    let again = ui.redraw();
    assert!(
        !canvas_messages(&again)
            .iter()
            .any(|event| matches!(event, CanvasEvent::FocusChanged(_))),
        "nothing replayed: {again:?}"
    );
}
