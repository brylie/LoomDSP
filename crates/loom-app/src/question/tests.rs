use super::*;
use crate::files::fake::{self, Asked};
use crate::recovery::tests::TempDir;
use crate::tests::{app, app_args};
use crate::ui_test::Ui;
use crate::{App, FileAction, Message as AppMessage};
use iced::keyboard::{Key, Modifiers, key::Named};

/// A window on a new patch with unsaved changes and no file.
fn changed() -> Ui {
    let mut app = app();
    app.edit(|app| {
        app.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    assert!(app.unsaved());
    let _ = fake::asked();
    Ui::with_app(app)
}

/// What the open question shows.
fn model(ui: &Ui) -> Model {
    ui.app.ui_snapshot().question.expect("a question is open")
}

/// Gives an answer, as its button does.
fn answer(ui: &mut Ui, answer: Answer) {
    ui.send(AppMessage::Question(Message::Answer(answer)));
}

/// How many modules the patch has.
fn modules(app: &App) -> usize {
    app.controller.graph().nodes().count()
}

/// Selects the patch's first module, so that F1 has help to open.
fn select_a_module(ui: &mut Ui) {
    let (id, _) = ui.app.controller.graph().nodes().next().expect("a module");
    ui.send(AppMessage::Canvas(loom_canvas::CanvasEvent::Select(vec![id.0])));
    ui.redraw();
}

/// The question's words come from the spec, and it is drawn in the window
/// with its buttons. So it needs no other program.
#[test]
fn the_question_is_drawn_in_the_window() {
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::New));
    ui.redraw();
    assert_eq!(model(&ui).title, "Save changes to \"Untitled\"?");
    assert!(ui.text("Save changes to \"Untitled\"?").is_some());
    assert!(ui.text("Your changes will be lost if you do not save them.").is_some());
    for label in ["Save", "Don't Save", "Cancel"] {
        assert!(ui.text(label).is_some(), "{label}");
    }
    assert_eq!(fake::asked(), Vec::new(), "no native dialog");
}

/// With nothing unsaved, nothing asks.
#[test]
fn a_clean_patch_is_not_asked_about() {
    let mut app = app();
    assert!(!app.unsaved());
    let _ = app.update(AppMessage::File(FileAction::Close));
    assert!(app.overlays.question.is_none());
    assert!(app.quit);
}

/// Every command that would drop the patch asks first, and goes on to do
/// what was asked with Don't Save.
#[test]
fn each_way_of_dropping_the_patch_asks() {
    let dir = TempDir::new("question-open");
    let other = dir.0.join("other.ron");
    std::fs::write(&other, r#"Patch(version: 1, name: "Other")"#).unwrap();

    // New.
    let mut ui = changed();
    let before = modules(&ui.app);
    ui.send(AppMessage::File(FileAction::New));
    assert!(ui.app.overlays.question.is_some());
    assert_eq!(modules(&ui.app), before, "it waits for the answer");
    answer(&mut ui, Answer::DontSave);
    assert!(modules(&ui.app) < before);
    assert!(!ui.app.unsaved());
    assert_eq!(ui.app.alerts.status(), "new patch");

    // Open: the dialog comes after the answer, never before it.
    let mut ui = changed();
    fake::answer_open(Some(other.clone()));
    ui.send(AppMessage::File(FileAction::Open));
    assert_eq!(fake::asked(), Vec::new(), "no picker before the answer");
    answer(&mut ui, Answer::DontSave);
    assert!(matches!(fake::asked().as_slice(), [Asked::Open(_)]));
    assert_eq!(ui.app.document.file(), Some(other.as_path()));
    fake::answer_open(None);

    // Closing the window.
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    assert!(!ui.app.quit, "it waits for the answer");
    answer(&mut ui, Answer::DontSave);
    assert!(ui.app.quit);
}

/// Cancel stops the command and changes nothing, however it is given.
#[test]
fn cancel_changes_nothing() {
    for cancel in [Message::Answer(Answer::Cancel), Message::Escape] {
        let mut ui = changed();
        let before = modules(&ui.app);
        ui.send(AppMessage::File(FileAction::Close));
        ui.send(AppMessage::Question(cancel));
        assert!(ui.app.overlays.question.is_none());
        assert!(!ui.app.quit);
        assert!(ui.app.unsaved());
        assert_eq!(modules(&ui.app), before);
    }
}

/// Save writes the patch, then goes on with what was asked for.
#[test]
fn save_writes_then_continues() {
    let dir = TempDir::new("question-save");
    let path = dir.0.join("mine.ron");
    fake::answer_save(Some(path.clone()));
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    answer(&mut ui, Answer::Save);
    fake::answer_save(None);
    assert!(path.is_file());
    assert!(ui.app.quit);
}

/// A cancelled Save dialog stays, with nothing lost.
#[test]
fn a_cancelled_save_stays() {
    fake::answer_save(None);
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    answer(&mut ui, Answer::Save);
    assert!(!ui.app.quit);
    assert!(ui.app.unsaved());
    assert!(ui.app.overlays.question.is_none());
}

/// A failed save stays and says why, with nothing lost.
#[test]
fn a_failed_save_stays_and_says_why() {
    let dir = TempDir::new("question-fail");
    // A file where the folder should be: the save cannot be written.
    std::fs::write(dir.0.join("blocked"), "").unwrap();
    fake::answer_save(Some(dir.0.join("blocked").join("patch.ron")));
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    answer(&mut ui, Answer::Save);
    fake::answer_save(None);
    assert!(!ui.app.quit);
    assert!(ui.app.unsaved());
    let error = model(&ui);
    assert!(error.text.starts_with("Could not save"), "{}", error.text);
    assert_eq!(error.buttons, ["ok"]);
    assert_eq!(error.highlight, "ok");
    assert_eq!(ui.app.alerts.status(), error.text, "the status bar repeats it");

    // OK closes it. Escape would too, and neither quits.
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    assert!(ui.app.overlays.question.is_none());
    assert!(!ui.app.quit);
}

/// A window on an untitled patch with changes, closing, having answered Save
/// and typed `foo` where `foo.ron` already exists.
fn asked_to_replace(dir: &TempDir) -> Ui {
    std::fs::write(dir.0.join("foo.ron"), "theirs").unwrap();
    fake::answer_save(Some(dir.0.join("foo")));
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    answer(&mut ui, Answer::Save);
    fake::answer_save(None);
    ui
}

/// The Save, then Replace, then continue chain: the app stays open while it
/// asks, and the highlight starts on the safe answer.
#[test]
fn replace_writes_then_continues() {
    let dir = TempDir::new("question-replace");
    let mut ui = asked_to_replace(&dir);
    assert!(!ui.app.quit, "it waits for the answer");
    let question = model(&ui);
    assert_eq!(question.title, "Replace \"foo.ron\"?");
    assert_eq!(question.buttons, ["replace", "cancel"]);
    assert_eq!(question.highlight, "cancel", "a stray Enter must not overwrite");
    assert_eq!(std::fs::read_to_string(dir.0.join("foo.ron")).unwrap(), "theirs");

    answer(&mut ui, Answer::Replace);
    assert_ne!(std::fs::read_to_string(dir.0.join("foo.ron")).unwrap(), "theirs");
    assert!(ui.app.quit);
}

/// Cancel in Replace leaves the file, the unsaved mark, and the app.
#[test]
fn cancelling_replace_keeps_everything() {
    let dir = TempDir::new("question-keep");
    let mut ui = asked_to_replace(&dir);
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    assert!(ui.app.overlays.question.is_none());
    assert_eq!(std::fs::read_to_string(dir.0.join("foo.ron")).unwrap(), "theirs");
    assert!(ui.app.unsaved());
    assert!(!ui.app.quit);
}

/// The toolbar's Save As asks about Replace too, with nothing to go on to.
#[test]
fn save_as_asks_about_replace_and_goes_no_further() {
    let dir = TempDir::new("question-save-as");
    std::fs::write(dir.0.join("foo.ron"), "theirs").unwrap();
    fake::answer_save(Some(dir.0.join("foo")));
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::SaveAs));
    fake::answer_save(None);
    answer(&mut ui, Answer::Replace);
    assert!(!ui.app.unsaved());
    assert!(!ui.app.quit);
    assert_eq!(ui.app.document.file(), Some(dir.0.join("foo.ron").as_path()));
}

/// Tab and the arrow keys move the highlight and wrap around, Enter gives it,
/// and it starts on Save.
#[test]
fn the_keys_move_and_give_the_highlight() {
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    assert_eq!(model(&ui).highlight, "save");
    ui.tap(Key::Named(Named::Tab), Modifiers::empty());
    assert_eq!(model(&ui).highlight, "dont-save");
    ui.tap(Key::Named(Named::ArrowRight), Modifiers::empty());
    assert_eq!(model(&ui).highlight, "cancel");
    ui.tap(Key::Named(Named::Tab), Modifiers::empty());
    assert_eq!(model(&ui).highlight, "save", "it wraps");
    ui.tap(Key::Named(Named::Tab), Modifiers::SHIFT);
    assert_eq!(model(&ui).highlight, "cancel", "Shift goes back");
    ui.tap(Key::Named(Named::ArrowLeft), Modifiers::empty());
    assert_eq!(model(&ui).highlight, "dont-save");
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    assert!(ui.app.quit, "Enter gave Don't Save");
}

/// A held Enter repeats, and a repeat answers nothing, so one press cannot
/// both raise a question and answer it.
#[test]
fn a_held_enter_answers_nothing() {
    let press = |repeat| {
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key: Key::Named(Named::Enter),
            modified_key: Key::Named(Named::Enter),
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Enter),
            location: iced::keyboard::Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat,
        })
    };
    let status = iced::event::Status::Ignored;
    let window = iced::window::Id::unique();
    assert!(key(press(true), status, window).is_none());
    assert!(matches!(
        key(press(false), status, window),
        Some(AppMessage::Question(Message::Activate))
    ));
}

/// With a text field focused behind the question, typing reaches neither the
/// field nor the patch, and Enter answers the question. *Why:* a native
/// dialog took these keys by stopping the app. This one has to take them.
#[test]
fn a_focused_field_gets_nothing() {
    let mut ui = changed();
    ui.focus(crate::patch_info::TAG_INPUT);
    ui.typewrite("a");
    let draft = ui.app.patch_info.tag_draft.clone();
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    ui.typewrite("drone");
    assert_eq!(ui.app.patch_info.tag_draft, draft);
    assert!(ui.app.overlays.question.is_some());
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    assert!(ui.app.overlays.question.is_none(), "Enter answered the question");
    assert!(ui.app.document.info().tags.is_empty(), "and added no tag");
}

/// What would change the patch or open something behind the question does
/// nothing: undo, New, Run, and help.
#[test]
fn shortcuts_do_nothing_behind_the_question() {
    let mut ui = changed();
    select_a_module(&mut ui);
    let count = modules(&ui.app);
    let steps = ui.app.edits.history.len();
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    let before = model(&ui);
    ui.tap(Key::Character("z".into()), Modifiers::COMMAND);
    assert_eq!(modules(&ui.app), count, "undo did nothing");
    assert_eq!(ui.app.edits.history.len(), steps);
    ui.tap(Key::Character("n".into()), Modifiers::COMMAND);
    assert_eq!(modules(&ui.app), count, "New did nothing");
    ui.tap(Key::Named(Named::Backspace), Modifiers::empty());
    assert_eq!(modules(&ui.app), count, "Delete did nothing");
    ui.tap(Key::Named(Named::Space), Modifiers::empty());
    ui.tap(Key::Named(Named::F1), Modifiers::empty());
    assert!(ui.app.overlays.help.is_none(), "help stays closed");
    assert_eq!(model(&ui), before, "the question stays as it was");
}

/// Escape cancels the question and leaves help open under it. Help listens to
/// Escape whatever has focus, so the gate must stop it.
#[test]
fn escape_leaves_help_open_under_the_question() {
    let mut ui = changed();
    select_a_module(&mut ui);
    ui.tap(Key::Named(Named::F1), Modifiers::empty());
    assert!(ui.app.overlays.help.is_some());
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert!(ui.app.overlays.question.is_none());
    assert!(ui.app.overlays.help.is_some(), "help is still open");
}

/// A second close request is ignored, and the question stays as it was.
#[test]
fn a_second_close_is_ignored() {
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    ui.tap(Key::Named(Named::Tab), Modifiers::empty());
    ui.send(AppMessage::File(FileAction::Close));
    assert_eq!(model(&ui).highlight, "dont-save");
    assert!(!ui.app.quit);
}

/// The Panic key works over the question, which stays open.
#[test]
fn the_panic_key_works_over_the_question() {
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    ui.tap(Key::Character(".".into()), Modifiers::COMMAND);
    assert!(
        ui.app.alerts.status().starts_with("Panic"),
        "{}",
        ui.app.alerts.status()
    );
    assert!(ui.app.overlays.question.is_some());
}

/// The gate lets through what keeps the music going, and what the canvas sends
/// as it lets go, and stops what would change the patch.
#[test]
fn the_gate_keeps_the_music_going() {
    use loom_canvas::CanvasEvent;
    for message in [
        AppMessage::Midi,
        AppMessage::Panic,
        AppMessage::Autosave,
        AppMessage::Lights(Vec::new()),
        AppMessage::Canvas(CanvasEvent::GestureEnd),
        AppMessage::Canvas(CanvasEvent::ButtonReleased { module: 1, widget: 0 }),
    ] {
        assert!(message.allowed_during_question(), "{message:?}");
    }
    for message in [
        AppMessage::Undo,
        AppMessage::File(FileAction::Save),
        AppMessage::Canvas(CanvasEvent::Delete(vec![1])),
        AppMessage::Canvas(CanvasEvent::ButtonPressed { module: 1, widget: 0 }),
    ] {
        assert!(!message.allowed_during_question(), "{message:?}");
    }
}

/// Closing asks over the browser, and Cancel returns to it, so rows read
/// meanwhile must not go missing.
#[test]
fn rows_still_reach_a_browser_under_the_question() {
    let dir = TempDir::new("question-rows");
    std::fs::write(dir.0.join("a.ron"), r#"Patch(version: 1, name: "A")"#).unwrap();
    let mut app = App::new(app_args());
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(dir.0.clone());
    let _ = app.rig.store.save(preferences);
    let _ = app.update(AppMessage::File(FileAction::Browse));
    app.edit(|app| {
        app.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    let _ = app.update(AppMessage::File(FileAction::Close));
    assert!(app.overlays.question.is_some());
    let rows = vec![crate::patches::Row::read(&dir.0.join("a.ron"))];
    let open = app.patches_open;
    let _ = app.update(AppMessage::Patches(crate::patches::Message::Found { open, rows }));
    let _ = app.update(AppMessage::Question(Message::Escape));
    let browser = app.overlays.patches().expect("the browser is still open");
    assert_eq!(browser.shown().count(), 1);
}

/// The strongest colour is on the safe answer, where Enter starts, and never
/// on overwriting a file.
#[test]
fn the_primary_style_is_on_the_safe_answer() {
    let path = PathBuf::from("foo.ron");
    assert_eq!(Question::discard("a".into(), AfterDiscard::New).model().primary, "save");
    let replace = Question::replace(path, None).model();
    assert_eq!(replace.primary, "cancel");
    assert_eq!(replace.primary, replace.highlight);
    assert_eq!(Question::error("no".into()).model().primary, "ok");
}
