use super::*;
use crate::App;
use crate::a11y::items::Item;
use crate::devices::fake::{self, Fake};
use crate::preferences::DeviceChoice;
use std::rc::Rc;

/// An app on a fake host with `devices` and the MIDI inputs `midi`, with
/// its preferences file in a folder of its own and Preferences open.
fn open(name: &str, fake: Fake) -> (App, Rc<Fake>) {
    let dir = std::env::temp_dir().join(format!("loom-prefs-access-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
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
    let mut app = App::boot_with(args, &mut crate::recovery::Dialogs, Rc::new(fake::Shared(fake.clone())));
    app.preferences(Message::Open);
    (app, fake)
}

/// The window's items.
fn dialog(app: &App) -> Item {
    items(&app.preferences_model().expect("Preferences is open"))
}

/// The part of `window` named by `id`.
fn part<'a>(window: &'a Item, id: &UiId) -> &'a Item {
    window
        .walk()
        .into_iter()
        .find(|item| item.id == *id)
        .unwrap_or_else(|| panic!("no part {id}"))
}

/// Sends the step up of the list `key`, as a screen reader's Increment.
fn step_up(app: &mut App, key: &str) {
    let window = dialog(app);
    let up = part(&window, &preferences_id().with(key)).step.0.clone();
    let _ = app.update(up.unwrap_or_else(|| panic!("{key} steps up")));
}

/// The window is a dialog named "Preferences", with its parts in screen
/// order, each named, and each list saying its chosen option.
#[test]
fn preferences_is_a_dialog_of_named_parts() {
    let (app, _) = open(
        "dialog",
        Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &["Keys", "Pads"]),
    );
    let window = dialog(&app);
    assert_eq!(window.kind, Kind::Dialog);
    assert_eq!(window.label.as_deref(), Some("Preferences"));
    for item in window.walk() {
        let acts = item.click.is_some() || item.step.0.is_some() || item.set_text.is_some();
        if acts {
            assert!(
                item.label.as_ref().is_some_and(|l| !l.is_empty()),
                "{} is named",
                item.id
            );
        }
    }
    let safety = part(&window, &preferences_id().with("safety"));
    assert_eq!(safety.kind, Kind::Options);
    assert_eq!(safety.children.len(), 3);
    assert_eq!(
        safety.children.iter().filter(|o| o.on == Some(true)).count(),
        1,
        "one mode is chosen"
    );
    let model = app.preferences_model().unwrap();
    let Some(Part::Picks(device)) = model.audio.first() else {
        panic!("the device list comes first");
    };
    let shown = part(&window, &preferences_id().with("device"));
    assert_eq!(shown.kind, Kind::Choice);
    assert_eq!(shown.value.as_deref(), device.chosen_text());
    for name in ["Keys", "Pads"] {
        let input = part(&window, &preferences_id().with("midi").with(name));
        assert_eq!(input.kind, Kind::Check);
        assert_eq!(input.label.as_deref(), Some(name));
    }
    let clear = part(&window, &preferences_id().with("panic").with("clear"));
    assert!(!clear.enabled, "no trigger to clear");
    let author = part(&window, &preferences_id().with("author"));
    assert_eq!(author.kind, Kind::Field);
    assert!(author.focus.is_some(), "a screen reader can focus it");
}

/// Two MIDI inputs with one name get two switches with two IDs.
#[test]
fn midi_inputs_with_one_name_have_their_own_ids() {
    let (app, _) = open("same-name", Fake::new(&[fake::device("Fake A")], &["Keys", "Keys"]));
    let window = dialog(&app);
    let midi = preferences_id().with("midi");
    let ids: Vec<&UiId> = window
        .children
        .iter()
        .filter(|item| item.id.is_under(&midi))
        .map(|item| &item.id)
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
}

/// Increment on the device list picks the next device, as picking it
/// from the list does, and plays on it.
#[test]
fn a_step_on_the_device_list_plays_the_next_device() {
    let (mut app, _) = open(
        "step",
        Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]),
    );
    step_up(&mut app, "device");
    step_up(&mut app, "device");
    assert_eq!(app.rig.shown_audio().device, DeviceChoice::Named("Fake B".into()));
    assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake B");
    let window = dialog(&app);
    let device = part(&window, &preferences_id().with("device"));
    assert_eq!(device.value.as_deref(), Some("Fake B"));
    assert!(device.step.0.is_none(), "the last device has no step up");
}

/// A device that cannot be used is said under the lists, where a screen
/// reader in the window hears it, and the next step goes on past it, so
/// the user is never stuck on it.
#[test]
fn a_device_that_fails_is_said_and_stepped_past() {
    let fake = Fake::new(
        &[fake::device("Fake A"), fake::device("Fake B"), fake::device("Fake C")],
        &[],
    );
    fake.broken.borrow_mut().push("Fake B".into());
    let (mut app, _) = open("broken", fake);
    step_up(&mut app, "device");
    assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake A");
    step_up(&mut app, "device");
    assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake A", "Fake B is busy");
    let window = dialog(&app);
    let error = part(&window, &preferences_id().with("note").with("audio-error"));
    assert!(error.value.as_deref().is_some_and(|text| text.contains("Fake B")));
    let device = part(&window, &preferences_id().with("device"));
    assert_eq!(device.value.as_deref(), Some("Fake A"), "the device in use");
    step_up(&mut app, "device");
    assert_eq!(app.rig.playing.as_ref().unwrap().device, "Fake C");
    let window = dialog(&app);
    assert!(
        !window
            .walk()
            .iter()
            .any(|item| item.id == preferences_id().with("note").with("audio-error")),
        "the error goes once a device works"
    );
}

/// A screen reader's new text for **Your name** is typed and saved, as
/// typing and Enter are.
#[test]
fn the_name_field_saves_what_a_screen_reader_sets() {
    let (mut app, _) = open("author", Fake::new(&[fake::device("Fake A")], &[]));
    for message in SetText::PreferencesAuthor.messages("Ada") {
        let _ = app.update(message);
    }
    assert_eq!(app.rig.store.saved().author, "Ada");
    assert_eq!(
        dialog(&app)
            .walk()
            .iter()
            .find(|i| i.kind == Kind::Field)
            .unwrap()
            .value
            .as_deref(),
        Some("Ada")
    );
}

/// With audio playing and Preferences open, the audio check finds nothing
/// new, so the poll key stays and the tree is not built again. A device
/// change changes what the window shows, so the key changes and the tree
/// is built. A build with nothing changed sends nothing.
#[test]
fn preferences_with_audio_playing_rebuilds_only_on_a_change() {
    let (mut app, _) = open(
        "poll",
        Fake::new(&[fake::device("Fake A"), fake::device("Fake B")], &[]),
    );
    assert!(app.session.engine.stream.is_playing());
    let _ = app.a11y_event(crate::a11y::Event::Activated);
    let places = crate::places::Places::default();
    let _ = app.a11y_changes(&places);
    let key = app.a11y_poll_key();
    app.poll_audio();
    app.check_audio(std::time::Instant::now());
    assert_eq!(app.a11y_poll_key(), key, "a poll that finds nothing new");
    let (_, again) = app.a11y_changes(&places);
    assert!(again.is_none(), "nothing changed, so nothing is sent");
    step_up(&mut app, "device");
    assert_ne!(app.a11y_poll_key(), key, "the window shows another device");
}
