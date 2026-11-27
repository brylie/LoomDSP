use super::*;
use crate::devices::fake::{self, Fake};
use crate::input_owner::InputOwner;
use crate::preferences::DeviceChoice;
use crate::recovery::tests::TempDir;
use crate::settings::Message;
use crate::ui_test::Ui;
use accesskit::{Action, Role as KitRole, Toggled};
use iced::keyboard::{Key, Modifiers, key::Named};
use loom_core::OutputSafety;
use std::rc::Rc;

/// An app on a fake host with `fake`'s devices, with its preferences file
/// in a folder of its own. No real device's name is ever used.
/// The folder comes first, so a caller that binds the tuple drops it
/// last, after the app.
fn app(name: &str, fake: Fake) -> (TempDir, App, Rc<Fake>) {
    let dir = TempDir::new(&format!("prefs-panel-{name}"));
    let fake = Rc::new(fake);
    let args = crate::Args {
        no_audio: false,
        no_recovery: true,
        devices: crate::devices::Options {
            preferences: Some(dir.join(crate::preferences::FILE_NAME)),
            ..crate::devices::Options::default()
        },
        ..crate::tests::app_args()
    };
    let app = App::boot_with(args, Rc::new(fake::Shared(fake.clone())));
    (dir, app, fake)
}

/// Two output devices and two MIDI inputs.
fn two_of_each() -> Fake {
    Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &["Keys", "Pads"])
}

/// The app in the harness with Preferences opened by Cmd+, as a keyboard
/// user opens it. The folder comes first, as in [`app`].
fn open(name: &str, fake: Fake) -> (TempDir, Ui, Rc<Fake>) {
    let (dir, app, fake) = app(name, fake);
    let mut ui = Ui::with_app(app);
    ui.tap(Key::Character(",".into()), Modifiers::COMMAND);
    let _ = ui.redraw();
    assert!(ui.app().preferences_model().is_some(), "Cmd+, opens Preferences");
    (dir, ui, fake)
}

/// Presses `key` and lays out what it changed.
fn tap(ui: &mut Ui, key: Key, modifiers: Modifiers) {
    ui.tap(key, modifiers);
    let _ = ui.redraw();
}

/// Tabs until `id` has the focus.
fn tab_to(ui: &mut Ui, id: &UiId) {
    for _ in 0..64 {
        if ui.runtime().focused() == Some(id) {
            return;
        }
        tap(ui, Key::Named(Named::Tab), Modifiers::default());
    }
    panic!("Tab never reached {id}: the focus is on {:?}", ui.runtime().focused());
}

/// A screen reader's `action` on the part `id`, with `data`, sent as the
/// shell sends it: to the runtime first, which carries out the actions of
/// its own selects and fields, then to the app.
fn act(ui: &mut Ui, id: &UiId, action: Action, data: Option<accesskit::ActionData>) {
    let node = ui.a11y_tree().node_by_id(id).node_id();
    let _ = ui.accessibility_turn(loom_ui::AccessEvent::Action(accesskit::ActionRequest {
        action,
        target_tree: accesskit::TreeId::ROOT,
        target_node: node,
        data,
    }));
    let _ = ui.redraw();
}

/// Every part's UI ID under `outer` in the tree, in tree order.
fn ids_under(ui: &mut Ui, outer: &UiId) -> Vec<UiId> {
    fn walk(node: crate::ui_test::A11yNode<'_>, outer: &UiId, out: &mut Vec<UiId>) {
        if let Some(id) = node.ui_id().filter(|id| id.is_under(outer) && *id != outer) {
            out.push(id.clone());
        }
        for child in node.children() {
            walk(child, outer, out);
        }
    }
    let tree = ui.a11y_tree();
    let mut out = Vec::new();
    walk(tree.root(), outer, &mut out);
    out
}

/// The output safety mode chosen now.
fn safety(ui: &Ui) -> OutputSafety {
    let model = ui.app().preferences_model().expect("Preferences is open");
    model
        .safety
        .iter()
        .find(|mode| mode.chosen)
        .expect("a mode is chosen")
        .mode
}

/// The window is a dialog named "Preferences", its parts under its body,
/// each named, with output safety a radio group, each list a combo box
/// saying its chosen option, each MIDI input a check box named by its
/// port, Clear turned off with no trigger, and **Your name** a text input
/// (ui-layer.md, section 7, rules 29 and 32).
#[test]
fn preferences_is_a_dialog_of_named_parts() {
    let (_dir, mut ui, _) = open("dialog", two_of_each());
    let tree = ui.a11y_tree();
    let window = tree.node_by_id(&preferences_id());
    assert_eq!(window.role(), KitRole::Dialog);
    assert_eq!(window.name(), Some("Preferences"));
    let group = tree.node_by_id(&part_id("safety"));
    assert_eq!(group.role(), KitRole::RadioGroup);
    let radios: Vec<_> = group
        .children()
        .into_iter()
        .filter(|node| node.role() == KitRole::RadioButton)
        .collect();
    assert_eq!(radios.len(), 3);
    assert_eq!(
        radios
            .iter()
            .filter(|radio| radio.toggled() == Some(Toggled::True))
            .count(),
        1,
        "one mode is chosen"
    );
    assert!(
        radios.iter().all(|radio| radio.description().is_some()),
        "each has its line"
    );
    let device = tree.node_by_id(&part_id("device"));
    assert_eq!(device.role(), KitRole::ComboBox);
    assert!(device.value().is_some_and(|value| value.starts_with("System default")));
    assert!(device.offers(Action::Expand));
    for name in ["Keys", "Pads"] {
        let input = tree.node_by_id(&part_id("midi").with(name));
        assert_eq!(input.role(), KitRole::CheckBox);
        assert_eq!(input.name(), Some(name));
    }
    assert!(
        tree.node_by_id(&part_id("panic").with("clear")).is_disabled(),
        "nothing to clear"
    );
    assert_eq!(tree.node_by_id(&author_part()).role(), KitRole::TextInput);
    assert_eq!(tree.node_by_id(&close_id()).role(), KitRole::Button);
}

/// Two MIDI inputs with one name get two check boxes with two IDs.
#[test]
fn midi_inputs_with_one_name_have_their_own_ids() {
    let (_dir, mut ui, _) = open("same-name", Fake::new(&[fake::device("Fake A")], &["Keys", "Keys"]));
    let boxes = ids_under(&mut ui, &part_id("midi"));
    assert_eq!(boxes.len(), 2);
    assert_ne!(boxes[0], boxes[1]);
}

/// Opened from the keyboard, Close has the focus with a ring; Down and
/// Page Down from Close scroll the body; Tab and Shift+Tab cycle the stops
/// in reading order and stay inside (preferences.md, Keys and focus).
#[test]
fn it_opens_on_close_and_tab_cycles_its_stops() {
    let (_dir, mut ui, _) = open("tab", two_of_each());
    assert_eq!(ui.runtime().focused(), Some(&close_id()));
    assert_eq!(ui.runtime().focus_ring().map(|(id, _)| id), Some(close_id()));
    let offset = |ui: &mut Ui| {
        let _ = ui.redraw();
        ui.runtime().scroll_offset(&body_id()).expect("the body scrolls").0
    };
    let start = offset(&mut ui);
    tap(&mut ui, Key::Named(Named::ArrowDown), Modifiers::default());
    let down = offset(&mut ui);
    assert!(down > start, "Down scrolls: {start} then {down}");
    tap(&mut ui, Key::Named(Named::PageDown), Modifiers::default());
    assert!(offset(&mut ui) > down, "Page Down scrolls further");
    assert_eq!(ui.runtime().focused(), Some(&close_id()), "Close keeps the focus");

    let stops = ui.app().preferences_stops();
    assert_eq!(stops[1], part_id("safety").with("low-latency"), "the group is one stop");
    assert!(!stops.contains(&part_id("panic").with("clear")), "Clear is off");
    let mut met = vec![close_id()];
    for _ in 1..stops.len() {
        tap(&mut ui, Key::Named(Named::Tab), Modifiers::default());
        met.push(ui.runtime().focused().cloned().expect("a stop has the focus"));
    }
    assert_eq!(met, stops, "in reading order");
    tap(&mut ui, Key::Named(Named::Tab), Modifiers::default());
    assert_eq!(ui.runtime().focused(), Some(&close_id()), "it wraps to Close");
    tap(&mut ui, Key::Named(Named::Tab), Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), stops.last(), "Shift+Tab wraps back");
}

/// Down on the output safety group chooses the next mode, and Up on Low
/// latency wraps to Off: the WAI-ARIA radio group's default (Brylie's
/// decision, 2026-11-24).
#[test]
fn the_safety_arrows_choose_off_included() {
    let (_dir, mut ui, _) = open("safety", two_of_each());
    tab_to(&mut ui, &part_id("safety").with("low-latency"));
    tap(&mut ui, Key::Named(Named::ArrowDown), Modifiers::default());
    assert_eq!(safety(&ui), OutputSafety::Clean);
    assert_eq!(ui.app().rig.store.saved().output_safety, OutputSafety::Clean, "saved");
    assert_eq!(
        ui.app().session.engine.stream.output_safety(),
        OutputSafety::Clean,
        "the engine runs it"
    );
    tap(&mut ui, Key::Named(Named::ArrowUp), Modifiers::default());
    assert_eq!(safety(&ui), OutputSafety::LowLatency);
    tap(&mut ui, Key::Named(Named::ArrowUp), Modifiers::default());
    assert_eq!(safety(&ui), OutputSafety::Off, "it wraps to Off");
}

/// A list opened by keyboard chooses its highlighted option with Enter,
/// and plays on that device; Escape closes an open list with no choice,
/// and a second Escape closes the window.
#[test]
fn a_list_chooses_with_enter_and_escape_closes_it_first() {
    let (_dir, mut ui, _) = open("list", two_of_each());
    tab_to(&mut ui, &part_id("device"));
    tap(&mut ui, Key::Named(Named::ArrowDown), Modifiers::default());
    tap(&mut ui, Key::Named(Named::ArrowDown), Modifiers::default());
    tap(&mut ui, Key::Named(Named::Enter), Modifiers::default());
    assert_eq!(ui.app().rig.shown_audio().device, DeviceChoice::Named("Fake A".into()));
    assert_eq!(ui.app().rig.output.playing.as_ref().unwrap().device, "Fake A");

    tap(&mut ui, Key::Named(Named::ArrowDown), Modifiers::default());
    tap(&mut ui, Key::Named(Named::ArrowDown), Modifiers::default());
    tap(&mut ui, Key::Named(Named::Escape), Modifiers::default());
    assert!(
        ui.app().preferences_model().is_some(),
        "the first Escape closes the list"
    );
    assert_eq!(ui.app().rig.shown_audio().device, DeviceChoice::Named("Fake A".into()));
    tap(&mut ui, Key::Named(Named::Escape), Modifiers::default());
    assert!(ui.app().preferences_model().is_none(), "the second closes the window");
}

/// A screen reader's Increment on the device list plays the next device,
/// as choosing it from the list does, and a step at the last does nothing.
#[test]
fn a_step_on_the_device_list_plays_the_next_device() {
    let (_dir, mut ui, _) = open("step", two_of_each());
    let device = part_id("device");
    act(&mut ui, &device, Action::Increment, None);
    act(&mut ui, &device, Action::Increment, None);
    assert_eq!(ui.app().rig.output.playing.as_ref().unwrap().device, "Fake B");
    assert_eq!(ui.a11y_tree().node_by_id(&device).value(), Some("Fake B"));
    act(&mut ui, &device, Action::Increment, None);
    assert_eq!(
        ui.app().rig.output.playing.as_ref().unwrap().device,
        "Fake B",
        "the last"
    );
}

/// A device that cannot be used is said under the lists, where a screen
/// reader in the window hears it. The list then opens on it and steps on
/// from it, so it never blocks the ones after it (ui-layer.md, section 5,
/// rule 18, the step's start; section 7, rule 27).
#[test]
fn a_device_that_fails_is_said_and_stepped_past() {
    let fake = Fake::new(
        &[fake::device("Fake A"), fake::device("Fake B"), fake::device("Fake C")],
        &[],
    );
    fake.broken.borrow_mut().push("Fake B".into());
    let (_dir, mut ui, _) = open("broken", fake);
    let device = part_id("device");
    act(&mut ui, &device, Action::Increment, None);
    act(&mut ui, &device, Action::Increment, None);
    assert_eq!(
        ui.app().rig.output.playing.as_ref().unwrap().device,
        "Fake A",
        "Fake B is busy"
    );
    let tree = ui.a11y_tree();
    let error = tree.node_by_id(&part_id("audio-error"));
    assert!(error.value().is_some_and(|text| text.contains("Fake B")));
    assert_eq!(tree.node_by_id(&device).value(), Some("Fake A"), "the device in use");

    // Down opens on the failed device; one more Down and Enter take the
    // next, so the step goes on past it.
    tab_to(&mut ui, &device);
    tap(&mut ui, Key::Named(Named::ArrowDown), Modifiers::default());
    tap(&mut ui, Key::Named(Named::ArrowDown), Modifiers::default());
    tap(&mut ui, Key::Named(Named::Enter), Modifiers::default());
    assert_eq!(ui.app().rig.output.playing.as_ref().unwrap().device, "Fake C");
    assert!(
        !ui.a11y_tree().has_part_under(&part_id("audio-error")),
        "the error goes once a device works"
    );
}

/// A screen reader's new text for **Your name** is kept as typed text is,
/// and saved when the window closes; Enter saves it too.
#[test]
fn the_name_field_saves_on_enter_and_on_close() {
    let (_dir, mut ui, _) = open("author", Fake::new(&[fake::device("Fake A")], &[]));
    act(
        &mut ui,
        &author_part(),
        Action::SetValue,
        Some(accesskit::ActionData::Value("Ada".into())),
    );
    assert_eq!(ui.a11y_tree().node_by_id(&author_part()).value(), Some("Ada"));
    tap(&mut ui, Key::Named(Named::Escape), Modifiers::default());
    assert_eq!(ui.app().rig.store.saved().author, "Ada", "closing saves it");

    tap(&mut ui, Key::Character(",".into()), Modifiers::COMMAND);
    tab_to(&mut ui, &author_part());
    ui.typewrite(" L");
    tap(&mut ui, Key::Named(Named::Enter), Modifiers::default());
    assert_eq!(ui.app().rig.store.saved().author, "Ada L", "Enter saves it");
    assert!(ui.app().preferences_model().is_some(), "Enter keeps the window open");
}

/// Escape and Cmd+, from **Your name** close the window and save the
/// name; the focus goes back to what had it before it opened.
#[test]
fn closing_from_the_name_field_saves_and_gives_the_focus_back() {
    let (_dir, app, _) = app("close", Fake::new(&[fake::device("Fake A")], &[]));
    let mut ui = Ui::with_app(app);
    tap(&mut ui, Key::Named(Named::Tab), Modifiers::SHIFT);
    let before = ui
        .runtime()
        .focused()
        .cloned()
        .expect("a stop of the bar has the focus");
    tap(&mut ui, Key::Character(",".into()), Modifiers::COMMAND);
    tab_to(&mut ui, &author_part());
    ui.typewrite("Ada");
    tap(&mut ui, Key::Character(",".into()), Modifiers::COMMAND);
    assert!(ui.app().preferences_model().is_none(), "Cmd+, closes it from the field");
    assert_eq!(ui.app().rig.store.saved().author, "Ada");
    assert_eq!(ui.runtime().focused(), Some(&before), "the focus came back");
}

/// A press on the backdrop leaves the window open (preferences.md, Keys and
/// focus); Clear moves the focus to Learn when it turns itself off.
#[test]
fn the_backdrop_does_nothing_and_clear_gives_the_focus_to_learn() {
    let (_dir, mut ui, _) = open("backdrop", two_of_each());
    tab_to(&mut ui, &part_id("activity"));
    ui.move_to(loom_ui_core::Point::new(4.0, 300.0));
    ui.press();
    ui.release();
    let _ = ui.redraw();
    assert!(ui.app().preferences_model().is_some(), "a press beside it does nothing");
    assert_eq!(
        ui.runtime().focused(),
        Some(&part_id("activity")),
        "the focus stays where it was (section 6, rule 3)"
    );

    ui.app_mut()
        .set_panic_pref(Some(crate::preferences::PanicPref::Note(9, 36)));
    let _ = ui.redraw();
    let clear = part_id("panic").with("clear");
    tab_to(&mut ui, &clear);
    tap(&mut ui, Key::Named(Named::Space), Modifiers::default());
    assert!(ui.app().rig.store.saved().panic.is_none(), "cleared");
    assert_eq!(ui.runtime().focused(), Some(&learn_id()));
}

/// A screen reader can choose an external clock source and switch clock
/// sending on.
#[test]
fn midi_clock_preferences_are_operable_with_a_screen_reader() {
    let (_dir, mut ui, fake) = open("midi-clock", Fake::new(&[fake::device("Fake A")], &["Drums"]));
    fake.midi_outputs.borrow_mut().push("Synth".into());
    ui.app_mut().poll_midi();
    let _ = ui.redraw();
    let source = part_id("clock-source");
    assert_eq!(ui.a11y_tree().node_by_id(&source).value(), Some("Internal"));
    act(&mut ui, &source, Action::Increment, None);
    assert_eq!(ui.a11y_tree().node_by_id(&source).value(), Some("Drums"));
    let output = send_clock_id("Synth");
    let tree = ui.a11y_tree();
    let switch = tree.node_by_id(&output);
    assert_eq!(switch.name(), Some("Send clock to Synth"));
    assert_eq!(switch.toggled(), Some(Toggled::False));
    ui.a11y_action(&output, Action::Click);
    assert_eq!(ui.a11y_tree().node_by_id(&output).toggled(), Some(Toggled::True));
}

/// The overlay the tree reports is the one that owns the input, by the
/// same answer the pointer reads: help over Preferences, and the question
/// over Preferences (section 7, rule 27).
#[test]
fn the_tree_reports_the_overlay_that_owns_the_input() {
    let (_dir, mut app, _) = app("owner", two_of_each());
    app.preferences(Message::Open);
    assert_eq!(app.input_owner(), InputOwner::Preferences);
    assert_eq!(app.ours_overlay().map(|(_, _, id)| id), Some(preferences_id()));
    app.overlays.help = Some(crate::help::Help::shortcuts());
    assert_eq!(app.input_owner(), InputOwner::Help);
    assert_eq!(
        app.ours_overlay().map(|(_, _, id)| id),
        Some(crate::overlays::access::help_id())
    );
    app.overlays.help = None;
    app.overlays.question = Some(crate::question::Question::error("No".to_string()));
    assert_eq!(app.input_owner(), InputOwner::Question);
    assert_eq!(
        app.ours_overlay().map(|(_, _, id)| id),
        Some(crate::overlays::access::question_id())
    );
}

/// With audio playing and Preferences open, the audio check finds nothing
/// new, so the poll key stays and the tree is not built again. A device
/// change changes what the window shows, so the key changes.
#[test]
fn preferences_with_audio_playing_rebuilds_only_on_a_change() {
    let (_dir, mut app, _) = app("poll", two_of_each());
    app.preferences(Message::Open);
    assert!(app.session.engine.stream.is_playing());
    app.a11y_event(crate::a11y::Event::Activated);
    let places = crate::places::Places::default();
    let _ = app.a11y_changes(&places);
    let key = app.a11y_poll_key();
    app.poll_audio();
    app.check_audio(std::time::Instant::now());
    assert_eq!(app.a11y_poll_key(), key, "a poll that finds nothing new");
    let (_, again) = app.a11y_changes(&places);
    assert!(again.is_none(), "nothing changed, so nothing is sent");
    let choose = app
        .preferences_chose(&part_id("device"), 1)
        .expect("the device list chooses");
    let _ = app.send(choose);
    assert_ne!(app.a11y_poll_key(), key, "the window shows another device");
}

/// F1 over Preferences opens help; closing it gives the focus back to the
/// part that had it, which is a stop again once Preferences owns the input
/// (help-pages.md; ui-layer.md, section 6, rule 4).
#[test]
fn help_over_preferences_gives_the_focus_back_when_it_closes() {
    let (_dir, mut ui, _) = open("help-back", two_of_each());
    let activity = part_id("activity");
    tab_to(&mut ui, &activity);
    tap(&mut ui, Key::Named(Named::F1), Modifiers::default());
    assert!(ui.app().overlays.help.is_some(), "F1 opened help");
    assert!(ui.app().preferences_stops().is_empty(), "Preferences is covered");
    tap(&mut ui, Key::Named(Named::Escape), Modifiers::default());
    assert!(ui.app().overlays.help.is_none(), "Escape closed help");
    assert!(ui.app().preferences_model().is_some(), "Preferences stays open");
    assert_eq!(ui.runtime().focused(), Some(&activity));
}
