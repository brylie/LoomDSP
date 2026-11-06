use super::*;
use crate::files::fake::{self as dialogs, Asked};
use crate::inspector::Inspector;
use crate::preferences::FILE_NAME;
use crate::{Args, FileAction, Message as AppMessage};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A folder for one test, emptied first.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-patch-info-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Args for an app whose preferences file in `dir` holds `preferences`.
fn args_with(dir: &Path, preferences: &str) -> Args {
    std::fs::write(dir.join(FILE_NAME), preferences).unwrap();
    Args {
        no_recovery: true,
        devices: crate::devices::Options {
            preferences: Some(dir.join(FILE_NAME)),
            ..crate::devices::Options::default()
        },
        ..crate::tests::app_args()
    }
}

/// What patch info shows now.
fn rows(app: &App) -> PatchRows {
    match app.inspector_model() {
        Inspector::Patch(rows) => rows,
        other => panic!("patch info is shown with nothing selected, not {other:?}"),
    }
}

/// Sends a patch info edit through the app.
fn send(app: &mut App, message: Message) {
    let _ = app.update(AppMessage::PatchInfo(message));
}

/// patch-metadata.md, Patch info: with nothing selected, the inspector
/// shows the patch name and its metadata, "No licence stated" when empty.
#[test]
fn nothing_selected_shows_patch_info() {
    let app = App::new(crate::tests::app_args());
    let rows = rows(&app);
    assert_eq!(rows.name, "Untitled");
    assert!(rows.authors.is_empty() && rows.tags.is_empty());
    assert_eq!(rows.licence, "No licence stated");
    assert_eq!(rows.licence_summary, None);
    assert!(!app.unsaved());
}

/// Acceptance: ` Bass Drone ` is stored as `bass-drone`, a duplicate is
/// stored once, a comma adds a tag, and a 21st tag is refused with a
/// message. Edits mark the patch unsaved; a chip's × removes its tag.
#[test]
fn tags_are_typed_tidied_and_limited() {
    let mut app = App::new(crate::tests::app_args());
    send(&mut app, Message::TagDraft(" Bass Drone ".into()));
    send(&mut app, Message::TagSubmit);
    assert_eq!(app.session.document.info().tags, ["bass-drone"]);
    assert!(app.unsaved());
    send(&mut app, Message::TagDraft("drone, Drone,dr".into()));
    assert_eq!(app.session.document.info().tags, ["bass-drone", "drone"]);
    assert_eq!(rows(&app).tag_draft, "dr");
    for i in 0..18 {
        send(&mut app, Message::TagDraft(format!("t{i},")));
    }
    assert_eq!(app.session.document.info().tags.len(), 20);
    send(&mut app, Message::TagDraft("one-too-many".into()));
    send(&mut app, Message::TagSubmit);
    assert_eq!(app.session.document.info().tags.len(), 20);
    let note = rows(&app).tag_note.expect("a refused tag says why");
    assert!(note.contains("at most 20"), "{note}");
    send(&mut app, Message::RemoveTag(0));
    assert_eq!(app.session.document.info().tags[0], "drone");
    assert_eq!(rows(&app).tag_note, None);
    send(&mut app, Message::RemoveTag(99));
    assert_eq!(app.session.document.info().tags.len(), 19);
}

/// Patch info: the authors box takes one author per line; empty lines
/// and duplicates are dropped from what the patch stores.
#[test]
fn authors_are_one_per_line() {
    let mut app = App::new(crate::tests::app_args());
    let paste = text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(
        " Ada Lovelace\n\nSam Rivera\nada lovelace".to_string(),
    )));
    send(&mut app, Message::Authors(paste));
    assert_eq!(app.session.document.info().authors, ["Ada Lovelace", "Sam Rivera"]);
    assert_eq!(rows(&app).authors, ["Ada Lovelace", "Sam Rivera"]);
    // Moving the cursor is not an edit.
    send(
        &mut app,
        Message::Authors(text_editor::Action::Move(text_editor::Motion::Home)),
    );
    assert_eq!(app.session.document.info().authors.len(), 2);
}

/// Patch info: a licence shows its short name, summary, and page; one
/// that is not SPDX shows "Unrecognised licence" and the text as written.
#[test]
fn the_licence_is_described() {
    let mut app = App::new(crate::tests::app_args());
    app.session
        .document
        .edit_info(|info| info.license = Some("CC-BY-SA-4.0".into()));
    let shown = rows(&app);
    assert_eq!(shown.licence, "CC-BY-SA-4.0");
    assert!(shown.licence_summary.unwrap().contains("same licence"));
    assert_eq!(
        shown.licence_url.as_deref(),
        Some("https://creativecommons.org/licenses/by-sa/4.0/")
    );
    app.session
        .document
        .edit_info(|info| info.license = Some("my own terms".into()));
    let shown = rows(&app);
    assert_eq!(shown.licence, "Unrecognised licence");
    assert!(shown.licence_summary.unwrap().contains("my own terms"));
    send(&mut app, Message::ChooseLicence);
    assert!(app.overlays.chooser.is_some(), "Choose opens the licence chooser");
}

/// Acceptance: with "Your name" and a default licence set, a new patch
/// starts with that author and licence, after New too, and is not unsaved.
#[test]
fn a_new_patch_takes_the_defaults() {
    let dir = folder("defaults");
    let mut app = App::new(args_with(
        &dir,
        r#"(author: "Ada Lovelace", default_license: Some("CC-BY-4.0"))"#,
    ));
    assert_eq!(app.session.document.info().authors, ["Ada Lovelace"]);
    assert_eq!(app.session.document.info().license.as_deref(), Some("CC-BY-4.0"));
    assert_eq!(rows(&app).authors, ["Ada Lovelace"]);
    assert!(!app.unsaved());
    let _ = app.update(AppMessage::File(FileAction::New));
    assert_eq!(app.session.document.info().authors, ["Ada Lovelace"]);
    assert_eq!(app.session.document.info().license.as_deref(), Some("CC-BY-4.0"));
    let _ = std::fs::remove_dir_all(dir);
}

/// Acceptance and rule 3: opening a patch by others and saving it keeps
/// every author, and offers once to add the user's name at the end.
#[test]
fn saving_someone_elses_patch_keeps_credit_and_offers_once() {
    let dir = folder("credit");
    let patch = dir.join("drone.ron");
    std::fs::write(
        &patch,
        r#"Patch(version: 1, metadata: Metadata(name: "Drone", authors: ["Ada Lovelace"], tags: ["drone"], license: "CC-BY-4.0"))"#,
    )
    .unwrap();
    let mut args = args_with(&dir, r#"(author: "Sam Rivera")"#);
    args.patch = Some(patch.clone());
    let mut app = App::new(args);
    assert_eq!(app.session.document.info().authors, ["Ada Lovelace"]);
    let _ = dialogs::asked();

    // Declined: every author stays, and the question is not asked again.
    dialogs::answer_add_author(Some(false));
    assert!(app.save());
    assert_eq!(dialogs::asked(), [Asked::AddAuthor("Sam Rivera".into())]);
    let saved = crate::patch_file::load(&patch).unwrap();
    assert_eq!(saved.metadata.authors, ["Ada Lovelace"]);
    assert_eq!(saved.metadata.tags, ["drone"]);
    assert_eq!(saved.metadata.license.as_deref(), Some("CC-BY-4.0"));
    assert!(app.save());
    assert_eq!(dialogs::asked(), []);

    // Opened again, it asks again; accepting adds the name at the end.
    app.open_path(&patch);
    dialogs::answer_add_author(Some(true));
    app.session.patch_info.tag_draft = "half-typ".into();
    assert!(app.save());
    assert_eq!(
        app.session.patch_info.tag_draft, "half-typ",
        "a tag being typed is kept"
    );
    assert_eq!(dialogs::asked(), [Asked::AddAuthor("Sam Rivera".into())]);
    assert_eq!(
        crate::patch_file::load(&patch).unwrap().metadata.authors,
        ["Ada Lovelace", "Sam Rivera"]
    );
    assert!(!app.unsaved());
    // The authors box shows the added name, and editing it keeps it.
    assert_eq!(
        app.session.patch_info.authors.text().lines().collect::<Vec<_>>(),
        ["Ada Lovelace", "Sam Rivera"]
    );
    send(
        &mut app,
        Message::Authors(text_editor::Action::Move(text_editor::Motion::DocumentEnd)),
    );
    send(
        &mut app,
        Message::Authors(text_editor::Action::Edit(text_editor::Edit::Paste(Arc::new(
            "\nLin".to_string(),
        )))),
    );
    assert_eq!(
        app.session.document.info().authors,
        ["Ada Lovelace", "Sam Rivera", "Lin"]
    );
    assert!(app.save());

    // With no dialog to ask, nothing is added, and the next Save asks.
    app.open_path(&patch);
    app.session
        .document
        .edit_info(|info| info.authors = vec!["Ada Lovelace".into()]);
    dialogs::answer_add_author(None);
    assert!(app.save());
    dialogs::answer_add_author(Some(false));
    assert!(app.save());
    assert_eq!(dialogs::asked().len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

/// Patch info edits are saved with the patch, and the description of an
/// opened patch is kept on save.
#[test]
fn metadata_edits_are_saved() {
    let dir = folder("save");
    let patch = dir.join("pad.ron");
    std::fs::write(
        &patch,
        r#"Patch(version: 1, metadata: Metadata(name: "Pad", description: "Soft."))"#,
    )
    .unwrap();
    let mut args = crate::tests::app_args();
    args.patch = Some(patch.clone());
    let mut app = App::new(args);
    send(&mut app, Message::TagDraft("ambient,".into()));
    assert!(app.unsaved());
    assert!(app.save());
    assert!(!app.unsaved());
    let saved = crate::patch_file::load(&patch).unwrap();
    assert_eq!(saved.metadata.description, "Soft.");
    assert_eq!(saved.metadata.tags, ["ambient"]);
    let _ = std::fs::remove_dir_all(dir);
}

/// In the window: patch info is in the inspector, the chooser asks the
/// commercial question only after the first answer, the Preferences window
/// has the new patch defaults, and Browse lists the library folder and
/// opens a patch with Enter (Keys).
#[test]
fn the_window_shows_patch_info_the_chooser_and_the_browser() {
    use crate::ui_test::Ui;
    use iced::keyboard::{Key, Modifiers, key::Named};
    let dir = folder("window");
    std::fs::write(
        dir.join("drone.ron"),
        r#"Patch(version: 1, metadata: Metadata(name: "Low Drone", authors: ["Ada Lovelace"], tags: ["drone"], license: "CC-BY-4.0"))"#,
    )
    .unwrap();
    let args = args_with(
        &dir,
        &format!(
            "(author: \"Sam Rivera\", library: Some({:?}))",
            dir.display().to_string()
        ),
    );
    let mut ui = Ui::with_app(App::new(args));
    assert!(ui.text("Authors, one per line").is_some());
    assert!(ui.text("No licence stated").is_some());

    ui.send(AppMessage::PatchInfo(Message::ChooseLicence));
    ui.redraw();
    assert!(ui.text("Choose a licence").is_some());
    assert!(ui.text("Allow commercial use?").is_none());
    ui.send(AppMessage::Licence(licence_chooser::Message::Answer(
        licence_chooser::Answer::Changes(loom_patch::metadata::Changes::ShareAlike),
    )));
    ui.send(AppMessage::Licence(licence_chooser::Message::Commercial(false)));
    ui.redraw();
    assert!(ui.text("Allow commercial use?").is_some());
    assert!(ui.text("CC-BY-NC-SA-4.0").is_some());
    ui.send(AppMessage::Licence(licence_chooser::Message::Use));
    ui.redraw();
    assert!(ui.text("CC-BY-NC-SA-4.0").is_some(), "patch info shows the new licence");

    ui.send(AppMessage::Preferences(crate::settings::Message::Open));
    ui.redraw();
    assert!(ui.text("New patches").is_some());
    assert!(ui.text("No default").is_some());
    ui.send(AppMessage::Preferences(crate::settings::Message::Author(
        "  Sam  ".into(),
    )));
    ui.send(AppMessage::Preferences(crate::settings::Message::Close));
    assert_eq!(ui.app.rig.store.saved().author, "Sam");

    ui.tap(Key::Character("o".into()), Modifiers::COMMAND | Modifiers::SHIFT);
    ui.redraw();
    assert!(ui.app.overlays.patches().is_some(), "Ctrl+Shift+O opens the browser");
    assert!(ui.text("Your patches").is_some());
    assert!(ui.text("Low Drone").is_some());
    ui.tap(Key::Named(Named::ArrowDown), Modifiers::empty());
    ui.typewrite("drone");
    ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    ui.redraw();
    assert!(ui.app.overlays.patches().is_none());
    // Unsaved changes are asked about first.
    ui.send(AppMessage::Question(crate::question::Message::Answer(
        crate::question::Answer::DontSave,
    )));
    assert_eq!(ui.app.session.document.name(), "Low Drone");
    let _ = std::fs::remove_dir_all(dir);
}

/// undo.md, Gestures: the undo shortcuts do nothing while a text field
/// has focus, because Iced's text fields have no undo of their own and
/// undo would change the patch behind the field. With the field left, they
/// work again.
#[test]
fn undo_keys_leave_a_focused_text_field_alone() {
    use crate::ui_test::Ui;
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut ui = Ui::with_app(App::new(crate::tests::app_args()));
    let sine = ui
        .app
        .modules
        .iter()
        .copied()
        .find(|d| d.id == "loom.osc.sine")
        .unwrap();
    ui.app.add_module(sine, iced::Point::ORIGIN);
    ui.app.session.selection.clear();
    ui.redraw();
    let count = ui.app.session.controller.graph().nodes().count();

    let z = || Key::Character("z".into());
    ui.focus(TAG_INPUT);
    ui.typewrite("drone");
    ui.tap(z(), Modifiers::COMMAND);
    assert_eq!(
        ui.app.session.controller.graph().nodes().count(),
        count,
        "typing keeps the canvas edit"
    );
    assert_eq!(ui.app.session.patch_info.tag_draft, "drone");

    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    ui.tap(z(), Modifiers::COMMAND);
    assert_eq!(
        ui.app.session.controller.graph().nodes().count(),
        count - 1,
        "out of the field, undo works"
    );

    // Redo waits for the field the same way.
    ui.focus(TAG_INPUT);
    ui.tap(z(), Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(
        ui.app.session.controller.graph().nodes().count(),
        count - 1,
        "typing keeps redo too"
    );
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    ui.tap(z(), Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(ui.app.session.controller.graph().nodes().count(), count);
}
