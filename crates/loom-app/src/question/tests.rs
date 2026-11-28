use super::*;
use crate::files::fake::{self, Asked};
use crate::recovery::tests::TempDir;
use crate::tests::{app, app_args};
use crate::ui_test::Ui;
use crate::{App, FileAction, Message as AppMessage};
use loom_ui_core::{Key, Modifiers};

/// A window on a new patch with unsaved changes and no file.
fn changed() -> Ui {
    let mut app = app();
    app.edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    assert!(app.unsaved());
    let _ = fake::asked();
    Ui::with_app(app)
}

/// What the open question shows.
fn model(ui: &Ui) -> Model {
    ui.app().ui_snapshot().question.expect("a question is open")
}

/// Gives an answer, as its button does.
fn answer(ui: &mut Ui, answer: Answer) {
    ui.send(AppMessage::Question(Message::Answer(answer)));
}

/// How many modules the patch has.
fn modules(app: &App) -> usize {
    app.session.controller.graph().nodes().count()
}

/// Selects the patch's first module, so that F1 has help to open.
fn select_a_module(ui: &mut Ui) {
    let (id, _) = ui.app().session.controller.graph().nodes().next().expect("a module");
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
    let _ = app.send(AppMessage::File(FileAction::Close));
    assert!(app.overlays.question.is_none());
    assert!(app.quit);
}

/// Every command that would drop the patch asks first, and goes on to do
/// what was asked with Don't Save.
#[test]
fn each_way_of_dropping_the_patch_asks() {
    let dir = TempDir::new("question-open");
    let other = dir.0.join("other.ron");
    std::fs::write(&other, r#"Patch(version: 1, metadata: Metadata(name: "Other"))"#).unwrap();

    // New.
    let mut ui = changed();
    let before = modules(ui.app());
    ui.send(AppMessage::File(FileAction::New));
    assert!(ui.app().overlays.question.is_some());
    assert_eq!(modules(ui.app()), before, "it waits for the answer");
    answer(&mut ui, Answer::DontSave);
    assert!(modules(ui.app()) < before);
    assert!(!ui.app().unsaved());
    assert_eq!(ui.app().alerts.status(), "new patch");

    // Open: the dialog comes after the answer, never before it.
    let mut ui = changed();
    fake::answer_open(Some(other.clone()));
    ui.send(AppMessage::File(FileAction::Open));
    assert_eq!(fake::asked(), Vec::new(), "no picker before the answer");
    answer(&mut ui, Answer::DontSave);
    assert!(matches!(fake::asked().as_slice(), [Asked::Open(_)]));
    assert_eq!(ui.app().session.document.file(), Some(other.as_path()));
    fake::answer_open(None);

    // Closing the window.
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    assert!(!ui.app().quit, "it waits for the answer");
    answer(&mut ui, Answer::DontSave);
    assert!(ui.app().quit);
}

/// Cancel stops the command and changes nothing, however it is given.
#[test]
fn cancel_changes_nothing() {
    for cancel in [Message::Answer(Answer::Cancel), Message::Escape] {
        let mut ui = changed();
        let before = modules(ui.app());
        ui.send(AppMessage::File(FileAction::Close));
        ui.send(AppMessage::Question(cancel));
        assert!(ui.app().overlays.question.is_none());
        assert!(!ui.app().quit);
        assert!(ui.app().unsaved());
        assert_eq!(modules(ui.app()), before);
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
    assert!(ui.app().quit);
}

/// A cancelled Save dialog stays, with nothing lost.
#[test]
fn a_cancelled_save_stays() {
    fake::answer_save(None);
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    answer(&mut ui, Answer::Save);
    assert!(!ui.app().quit);
    assert!(ui.app().unsaved());
    assert!(ui.app().overlays.question.is_none());
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
    assert!(!ui.app().quit);
    assert!(ui.app().unsaved());
    let error = model(&ui);
    assert!(error.text.starts_with("Could not save"), "{}", error.text);
    assert_eq!(error.buttons, ["ok"]);
    assert_eq!(error.highlight, "ok");
    assert_eq!(ui.app().alerts.status(), error.text, "the status bar repeats it");

    // OK closes it. Escape would too, and neither quits.
    ui.tap(Key::Enter, Modifiers::NONE);
    assert!(ui.app().overlays.question.is_none());
    assert!(!ui.app().quit);
}

/// A question that replaces another in the same update takes the focus
/// to its safe answer, with a ring, as one that opens where none was does:
/// here a save to the patch's own file fails while the Discard question is
/// answered. The focus is let go first, so only the new question's own
/// focus effect can put it back: a layout's fallback to the first stop
/// would not, and is not the safe answer for every question.
#[test]
fn a_question_that_replaces_another_takes_the_focus() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    // The patch's own file is now a folder, so the save cannot write it.
    let path = ui.app().session.document.file().expect("a file").to_path_buf();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    assert_eq!(model(&ui).highlight, "save");
    let _ = ui.unfocus_all();
    ui.redraw();
    assert_eq!(ui.runtime().focused(), None, "the focus is let go");
    answer(&mut ui, Answer::Save);
    ui.redraw();
    let error = model(&ui);
    assert!(error.text.starts_with("Could not save"), "{}", error.text);
    let ok = ui.app().overlays.question.as_ref().expect("the error").highlighted_id();
    assert_eq!(ui.runtime().focused(), Some(&ok), "the new question's answer");
    assert!(ui.runtime().focus_ring().is_some(), "with its ring");
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
    assert!(!ui.app().quit, "it waits for the answer");
    let question = model(&ui);
    assert_eq!(question.title, "Replace \"foo.ron\"?");
    assert_eq!(question.buttons, ["replace", "cancel"]);
    assert_eq!(question.highlight, "cancel", "a stray Enter must not overwrite");
    assert_eq!(std::fs::read_to_string(dir.0.join("foo.ron")).unwrap(), "theirs");

    answer(&mut ui, Answer::Replace);
    assert_ne!(std::fs::read_to_string(dir.0.join("foo.ron")).unwrap(), "theirs");
    assert!(ui.app().quit);
}

/// Cancel in Replace leaves the file, the unsaved mark, and the app.
#[test]
fn cancelling_replace_keeps_everything() {
    let dir = TempDir::new("question-keep");
    let mut ui = asked_to_replace(&dir);
    ui.tap(Key::Enter, Modifiers::NONE);
    assert!(ui.app().overlays.question.is_none());
    assert_eq!(std::fs::read_to_string(dir.0.join("foo.ron")).unwrap(), "theirs");
    assert!(ui.app().unsaved());
    assert!(!ui.app().quit);
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
    assert!(!ui.app().unsaved());
    assert!(!ui.app().quit);
    assert_eq!(ui.app().session.document.file(), Some(dir.0.join("foo.ron").as_path()));
}

/// Tab and the arrow keys move the highlight and wrap around, Enter gives it,
/// and it starts on Save.
#[test]
fn the_keys_move_and_give_the_highlight() {
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    assert_eq!(model(&ui).highlight, "save");
    ui.tap(Key::Tab, Modifiers::NONE);
    assert_eq!(model(&ui).highlight, "dont-save");
    ui.tap(Key::Right, Modifiers::NONE);
    assert_eq!(model(&ui).highlight, "cancel");
    ui.tap(Key::Tab, Modifiers::NONE);
    assert_eq!(model(&ui).highlight, "save", "it wraps");
    ui.tap(Key::Tab, Modifiers::SHIFT);
    assert_eq!(model(&ui).highlight, "cancel", "Shift goes back");
    ui.tap(Key::Left, Modifiers::NONE);
    assert_eq!(model(&ui).highlight, "dont-save");
    ui.tap(Key::Enter, Modifiers::NONE);
    assert!(ui.app().quit, "Enter gave Don't Save");
}

/// A held Enter repeats, and a repeat answers nothing, so one press cannot
/// both raise a question and answer it. The panic shortcut is heard over
/// the question, once per press; any other key means nothing to it.
#[test]
fn a_held_enter_answers_nothing() {
    use loom_ui_core::{Key as K, KeyEvent, Modifiers as M};
    let press = |key, modifiers, repeat| KeyEvent::Pressed {
        key,
        modifiers,
        repeat,
        text: None,
        code: loom_ui_core::Code::Other,
    };
    assert!(key_of(&press(K::Enter, M::NONE, true)).is_none());
    assert!(matches!(
        key_of(&press(K::Enter, M::NONE, false)),
        Some(AppMessage::Question(Message::Activate))
    ));
    let shortcut = if cfg!(target_os = "macos") {
        M { meta: true, ..M::NONE }
    } else {
        M {
            control: true,
            ..M::NONE
        }
    };
    assert!(matches!(
        key_of(&press(K::Character('.'), shortcut, false)),
        Some(AppMessage::Panic)
    ));
    assert!(key_of(&press(K::Character('.'), shortcut, true)).is_none());
    assert!(key_of(&press(K::Backspace, M::NONE, false)).is_none());
    assert!(key_of(&press(K::Character('d'), shortcut, false)).is_none());
}

/// With a text field focused behind the question, typing reaches neither the
/// field nor the patch, and Enter answers the question. *Why:* a native
/// dialog took these keys by stopping the app. This one has to take them.
#[test]
fn a_focused_field_gets_nothing() {
    let mut ui = changed();
    ui.focus(&crate::patch_info::tags_part());
    ui.typewrite("a");
    let draft = ui.app().session.patch_info.tag_draft.clone();
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    ui.typewrite("drone");
    assert_eq!(ui.app().session.patch_info.tag_draft, draft);
    assert!(ui.app().overlays.question.is_some());
    ui.tap(Key::Enter, Modifiers::NONE);
    assert!(ui.app().overlays.question.is_none(), "Enter answered the question");
    // Enter gave Save, the safe answer, which adds the tag typed before the
    // question opened and nothing typed behind it (files.md, Text still
    // being typed).
    assert_eq!(
        ui.app().session.document.info().tags,
        [draft],
        "only the earlier draft is added"
    );
}

/// What would change the patch or open something behind the question does
/// nothing: undo, New, Run, and help.
#[test]
fn shortcuts_do_nothing_behind_the_question() {
    let mut ui = changed();
    select_a_module(&mut ui);
    let count = modules(ui.app());
    let steps = ui.app().session.edits.history.len();
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    let before = model(&ui);
    ui.tap(Key::Character('z'), Modifiers::platform_shortcut());
    assert_eq!(modules(ui.app()), count, "undo did nothing");
    assert_eq!(ui.app().session.edits.history.len(), steps);
    ui.tap(Key::Character('n'), Modifiers::platform_shortcut());
    assert_eq!(modules(ui.app()), count, "New did nothing");
    ui.tap(Key::Backspace, Modifiers::NONE);
    assert_eq!(modules(ui.app()), count, "Delete did nothing");
    ui.tap(Key::F1, Modifiers::NONE);
    assert!(ui.app().overlays.help.is_none(), "help stays closed");
    assert_eq!(model(&ui), before, "the question stays as it was");
}

/// Escape cancels the question and leaves help open under it. Help listens to
/// Escape whatever has focus, so the gate must stop it.
#[test]
fn escape_leaves_help_open_under_the_question() {
    let mut ui = changed();
    select_a_module(&mut ui);
    ui.tap(Key::F1, Modifiers::NONE);
    assert!(ui.app().overlays.help.is_some());
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    ui.tap(Key::Escape, Modifiers::NONE);
    assert!(ui.app().overlays.question.is_none());
    assert!(ui.app().overlays.help.is_some(), "help is still open");
}

/// A second close request is ignored, and the question stays as it was.
#[test]
fn a_second_close_is_ignored() {
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    ui.tap(Key::Tab, Modifiers::NONE);
    ui.send(AppMessage::File(FileAction::Close));
    assert_eq!(model(&ui).highlight, "dont-save");
    assert!(!ui.app().quit);
}

/// Space held on an answer, then Escape: one Escape cancels the held press
/// and closes the question with the safe answer, and the Space release
/// afterwards sends nothing (#926; the WAI-ARIA alertdialog pattern has
/// Escape close the dialog). Before, the first Escape only cancelled the
/// press and a second one was needed.
#[test]
fn escape_with_space_held_on_an_answer_closes_the_question_once() {
    let mut ui = changed();
    let before = modules(ui.app());
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    // Tab moves the focus off the safe answer, so the held Space is on an
    // answer that would act: the release must not click it.
    ui.tap(Key::Tab, Modifiers::NONE);
    let armed_on = model(&ui).highlight;
    assert_ne!(armed_on, "cancel", "Space is held on another answer");
    ui.key_down(Key::Space, false);
    ui.tap(Key::Escape, Modifiers::NONE);
    assert!(ui.app().overlays.question.is_none(), "one Escape closed it");
    let released = ui.key_up(Key::Space);
    assert!(
        !released.iter().any(|m| matches!(m, AppMessage::Question(_))),
        "the release clicks nothing: {released:?}"
    );
    assert!(!ui.app().quit, "the window stays");
    assert!(ui.app().unsaved(), "nothing was saved or dropped");
    assert_eq!(modules(ui.app()), before);
}

/// The Panic key works over the question, which stays open.
#[test]
fn the_panic_key_works_over_the_question() {
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    ui.tap(Key::Character('.'), Modifiers::platform_shortcut());
    assert!(
        ui.app().alerts.status().starts_with("Panic"),
        "{}",
        ui.app().alerts.status()
    );
    assert!(ui.app().overlays.question.is_some());
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
    std::fs::write(
        dir.0.join("a.ron"),
        r#"Patch(version: 1, metadata: Metadata(name: "A"))"#,
    )
    .unwrap();
    let mut app = App::new(app_args());
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(dir.0.clone());
    let _ = app.rig.store.save(preferences);
    let _ = app.send(AppMessage::File(FileAction::Browse));
    app.edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    let _ = app.send(AppMessage::File(FileAction::Close));
    assert!(app.overlays.question.is_some());
    // The reader ran to its end as the browser opened (work runs inline in
    // a test). A row it finds later still arrives under the question.
    std::fs::write(
        dir.0.join("b.ron"),
        r#"Patch(version: 1, metadata: Metadata(name: "B"))"#,
    )
    .unwrap();
    let rows = vec![crate::patches::Row::read(&dir.0.join("b.ron"))];
    let open = app.patches_open;
    let _ = app.send(AppMessage::Patches(crate::patches::Message::Found { open, rows }));
    let _ = app.send(AppMessage::Question(Message::Escape));
    let browser = app.overlays.patches().expect("the browser is still open");
    assert_eq!(browser.shown().count(), 2);
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

/// The question has every key (ui-layer.md, section 6, rule 6, step 2;
/// plan 3.2, row 3a): with a module selected under it, Backspace, Delete,
/// and the shortcuts on the selection (Cmd+D, Cmd+X, Cmd+C, Cmd+Z) never
/// reach the app as a key at all, since the runtime keeps each press from
/// Iced, and the module, the selection, the history, and the clipboard
/// stay as they were.
#[test]
fn keys_under_the_question_never_reach_the_app() {
    let mut ui = changed();
    select_a_module(&mut ui);
    let count = modules(ui.app());
    let selection = ui.app().session.selection.clone();
    let steps = ui.app().session.edits.history.len();
    let clipboard = ui.clipboard().map(str::to_string);
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    use loom_ui_core::{Key as K, Modifiers as M};
    let keys = [
        (K::Backspace, M::NONE),
        (K::Delete, M::NONE),
        (K::Character('d'), M::platform_shortcut()),
        (K::Character('x'), M::platform_shortcut()),
        (K::Character('c'), M::platform_shortcut()),
        (K::Character('z'), M::platform_shortcut()),
    ];
    for (key, modifiers) in keys {
        let sent = ui.tap(key, modifiers);
        assert!(
            !sent
                .iter()
                .any(|message| matches!(message, AppMessage::Key(loom_ui_core::KeyEvent::Pressed { .. }))),
            "{key:?}: no key reached the app: {sent:?}"
        );
    }
    assert!(ui.app().overlays.question.is_some(), "the question is still open");
    assert_eq!(modules(ui.app()), count, "no module went or came");
    assert_eq!(ui.app().session.selection, selection, "the selection is the same");
    assert_eq!(ui.app().session.edits.history.len(), steps, "no history step");
    assert_eq!(
        ui.clipboard().map(str::to_string),
        clipboard,
        "the clipboard is the same"
    );
}

/// Space clicks the focused answer when it comes up, as on any button
/// (ui-layer.md, section 3, rule 4; row 3b): the question opens with its
/// safe answer focused, so Space gives that answer.
#[test]
fn space_gives_the_focused_answer() {
    let mut ui = changed();
    ui.send(AppMessage::File(FileAction::Close));
    ui.redraw();
    assert_eq!(model(&ui).highlight, "save", "the safe answer has the focus");
    ui.tap(Key::Tab, Modifiers::NONE);
    assert_eq!(model(&ui).highlight, "dont-save", "Tab moved it");
    ui.tap(Key::Space, Modifiers::NONE);
    assert!(ui.app().quit, "Space gave Don't Save");
}

/// A draft being typed when the unsaved-changes question opens is in the
/// file its Save writes: a tag in patch info's Tags field, and a value in
/// an inspector value field. The question takes the focus without the
/// field being left (ui-layer.md, section 6, rule 4), so the draft stays
/// until Save commits it, as Enter would (files.md, Text still being
/// typed; #897). `drafts/tests.rs` covers the MIDI range and the refusals.
#[test]
fn a_draft_is_in_the_file_the_questions_save_writes() {
    let dir = TempDir::new("question-save-draft");
    let path = dir.0.join("mine.ron");
    fake::answer_save(Some(path.clone()));
    let mut ui = changed();
    ui.send(AppMessage::Canvas(loom_canvas::CanvasEvent::Select(Vec::new())));
    let _ = ui.redraw();
    ui.focus(&crate::patch_info::tags_part());
    let _ = ui.typewrite("drafted");
    assert_eq!(ui.app().session.patch_info.tag_draft, "drafted");
    ui.send(AppMessage::File(FileAction::Close));
    answer(&mut ui, Answer::Save);
    fake::answer_save(None);
    let saved = crate::patch_file::load(&path).expect("the file was written");
    assert_eq!(saved.metadata.tags, ["drafted"], "the tag draft is saved");
    assert!(ui.app().quit);

    // A value field's draft, on a module.
    let path = dir.0.join("value.ron");
    fake::answer_save(Some(path.clone()));
    let mut ui = changed();
    let mut filter = loom_core::ModuleId(0);
    ui.app_mut().edit(|app| {
        filter = app.session.controller.add_module("loom.filter.svf").unwrap();
        app.commit();
    });
    let module = filter.0;
    ui.send(AppMessage::Canvas(loom_canvas::CanvasEvent::Select(vec![module])));
    let _ = ui.redraw();
    let index = {
        let node = ui.app().session.controller.graph().node(filter).unwrap();
        loom_core::ModuleNames::parameter_index(node, "cutoff").unwrap()
    };
    let at = ui.value_field(module, index);
    let _ = ui.move_to(at);
    let _ = ui.press();
    let _ = ui.release();
    let _ = ui.typewrite("1234");
    assert!(ui.app().session.value_draft.is_some());
    ui.send(AppMessage::File(FileAction::Close));
    answer(&mut ui, Answer::Save);
    fake::answer_save(None);
    let saved = crate::patch_file::load(&path).expect("the file was written");
    let cutoff = saved
        .modules
        .iter()
        .find(|saved| saved.id == module)
        .expect("the module is saved")
        .params
        .get("cutoff")
        .copied()
        .expect("a typed cutoff is saved");
    assert!((cutoff - 1234.0).abs() < 1.0, "the value draft is saved: {cutoff}");
    assert!(ui.app().quit, "the close went on");
}

/// An open question holds every press below it: a click on the toolbar's
/// Open and on a canvas tile does nothing, and the question stays. Once it
/// closes, the same click on the tile selects it (ui-layer.md, section 5,
/// rule 1).
#[test]
fn a_question_holds_presses_on_the_toolbar_and_the_canvas() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    ui.send(AppMessage::File(FileAction::Close));
    let _ = ui.redraw();
    assert!(ui.app().overlays.question.is_some());
    ui.lay_out_panels();
    // The question's panel is the whole window, a scrim with the dialog
    // centred in it; a press on the scrim answers nothing.
    let window = ui
        .runtime()
        .bounds(&crate::overlays::access::question_id())
        .expect("the question is laid out");
    let half = crate::style::QUESTION_WIDTH / 2.0;
    let centre = window.x + window.width / 2.0;
    let dialog = |at: loom_ui_core::Point| (at.x - centre).abs() <= half;
    // A point on a tile's title, beside the dialog.
    let canvas = ui.canvas();
    let (module, at) = ui
        .app()
        .session
        .scene
        .modules()
        .iter()
        .map(|tile| {
            let bounds = tile.bounds();
            (
                tile.id,
                canvas.to_window(loom_ui_core::Point::new(bounds.x + bounds.width / 2.0, bounds.y + 6.0)),
            )
        })
        .find(|(_, at)| !dialog(*at) && canvas.bounds.contains(*at))
        .expect("a tile outside the question");
    let selection = ui.app().ui_snapshot().selection;

    let mut heard = ui.click_text("Open");
    heard.extend(ui.move_to(at));
    heard.extend(ui.press());
    heard.extend(ui.release());
    assert!(
        !heard
            .iter()
            .any(|m| matches!(m, AppMessage::File(_) | AppMessage::Canvas(_))),
        "nothing below the question heard the presses: {heard:?}"
    );
    assert_eq!(ui.app().ui_snapshot().selection, selection);
    assert!(ui.app().overlays.question.is_some(), "the question stays");

    ui.tap(Key::Escape, Modifiers::NONE);
    assert!(ui.app().overlays.question.is_none());
    ui.move_to(at);
    ui.press();
    ui.release();
    assert_eq!(ui.app().ui_snapshot().selection, vec![module], "the tile selects now");
}
