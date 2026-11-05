use super::*;
use crate::files::fake::{self, Asked};
use crate::question::{Answer, Message as QuestionMessage};
use crate::recovery::tests::TempDir;
use crate::tests::{app, app_args};
use crate::ui_test::Ui;
use crate::{App, FileAction, Message as AppMessage};
use iced::keyboard::{Key, Modifiers, key::Named};

/// `count` patch files in `dir`, as `0.ron`, `1.ron`, and so on.
fn patches(dir: &Path, count: usize) -> Vec<PathBuf> {
    (0..count)
        .map(|i| {
            let path = dir.join(format!("{i}.ron"));
            std::fs::write(&path, format!(r#"Patch(version: 1, name: "Patch {i}")"#)).unwrap();
            path
        })
        .collect()
}

/// An app that makes one edit, so that dropping the patch asks.
fn changed(app: &mut App) {
    app.edit(|app| {
        app.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    assert!(app.unsaved());
}

/// The list holds 10, newest first, each once, and survives a restart.
#[test]
fn the_list_holds_ten_newest_first_and_survives_a_restart() {
    let dir = TempDir::new("recent-ten");
    let preferences = dir.0.join("preferences.ron");
    let (mut list, note) = RecentList::open(Some(&preferences));
    assert_eq!(note, None, "no file yet is no problem");
    let files = patches(&dir.0, 12);
    for file in &files {
        list.remember(file).unwrap();
    }
    assert_eq!(list.files().len(), 10);
    assert_eq!(list.files()[0], files[11]);
    assert_eq!(list.files()[9], files[2]);

    list.remember(&files[5]).unwrap();
    assert_eq!(list.files()[0], files[5]);
    assert_eq!(list.files().len(), 10, "listed once");

    let (again, note) = RecentList::open(Some(&preferences));
    assert_eq!(note, None);
    assert_eq!(again.files(), list.files());
    assert!(dir.0.join(FILE_NAME).is_file(), "beside the preferences file");
}

/// A file that does not read gives an empty list and a note, not a failure.
#[test]
fn a_corrupt_file_gives_an_empty_list() {
    let dir = TempDir::new("recent-corrupt");
    std::fs::write(dir.0.join(FILE_NAME), "not ron at all (").unwrap();
    let (list, note) = RecentList::open(Some(&dir.0.join("preferences.ron")));
    assert!(list.files().is_empty());
    assert!(note.unwrap().starts_with("Could not read the recent patches"));
}

/// A failed write never stops a save: the list is only a convenience.
#[test]
fn a_failed_write_never_stops_a_save() {
    let dir = TempDir::new("recent-unwritable");
    // A file where the settings folder should be.
    std::fs::write(dir.0.join("blocked"), "").unwrap();
    let mut app = app();
    app.rig.recent = RecentList::open(Some(&dir.0.join("blocked").join("preferences.ron"))).0;
    let saved = dir.0.join("mine.ron");
    assert!(app.write_to(saved.clone()), "the save still works");
    assert!(saved.is_file());
    assert!(
        app.alerts.status().starts_with("Could not keep the recent patches"),
        "{}",
        app.alerts.status()
    );
}

/// Opening and saving put a patch on the list. A failed open does not.
#[test]
fn opening_and_saving_are_listed() {
    let dir = TempDir::new("recent-listed");
    let files = patches(&dir.0, 2);
    let mut app = app();
    assert!(app.open_path(&files[0]));
    assert_eq!(
        app.rig.recent.files(),
        [files[0].clone()],
        "the list outlives the engine"
    );
    assert!(app.write_to(files[1].clone()));
    assert_eq!(app.rig.recent.files(), [files[1].clone(), files[0].clone()]);
    assert!(!app.open_path(&dir.0.join("missing.ron")));
    assert_eq!(app.rig.recent.files().len(), 2, "a failed open is not listed");
}

/// A copy of an example is not listed, however it opens.
#[test]
fn an_example_copy_is_not_listed() {
    let dir = TempDir::new("recent-example");
    let examples = dir.0.join("examples");
    std::fs::create_dir_all(&examples).unwrap();
    let example = patches(&examples, 1).remove(0);
    fake::use_examples(Some(examples));
    let mut app = app();
    fake::use_examples(None);
    assert!(app.open_path(&example));
    assert_eq!(app.document.file(), None);
    assert!(app.rig.recent.files().is_empty());
}

/// Nothing a script does changes the player's list, opening or saving.
#[test]
fn a_script_does_not_change_the_list() {
    let dir = TempDir::new("recent-script");
    let files = patches(&dir.0, 2);
    let mut app = App::new(app_args());
    app.control
        .push(crate::control::Listener::Script(PathBuf::from("unused")));
    assert!(app.replace(crate::Source::File(&files[0])));
    assert!(app.write_to(files[1].clone()));
    assert!(app.open_path(&files[0]));
    assert!(app.rig.recent.files().is_empty());
}

/// A path on the command line is listed absolute, in a normal run. A run that
/// measures does not touch the list.
#[test]
fn the_command_line_is_listed_only_in_a_normal_run() {
    let dir = TempDir::new("recent-args");
    let mine = patches(&dir.0, 1).remove(0);
    let normal = crate::Args {
        patch: Some(mine.clone()),
        ..app_args()
    };
    let mut app = App::new(normal.clone());
    app.remember_opened_at_start(&normal);
    assert_eq!(app.rig.recent.files(), std::slice::from_ref(&mine));

    let mut measuring = normal.clone();
    measuring.bench.measure = true;
    let mut app = App::new(measuring.clone());
    app.remember_opened_at_start(&measuring);
    assert!(app.rig.recent.files().is_empty());

    // Listed once, whichever way the path was spelled.
    let mut app = crate::tests::app();
    let cwd = std::env::current_dir().unwrap();
    let from_here = pathdiff(&mine, &cwd);
    assert!(app.open_path(&from_here));
    assert!(app.rig.recent.files()[0].is_absolute(), "{:?}", app.rig.recent.files());
}

/// `path` written relative to `from`, by climbing to the root and back.
fn pathdiff(path: &Path, from: &Path) -> PathBuf {
    let up = from.components().skip(1).map(|_| "..").collect::<PathBuf>();
    up.join(path.strip_prefix("/").unwrap_or(path))
}

/// The toolbar's button is disabled with an empty list. It opens a list with
/// file names and folders. Escape closes it.
#[test]
fn the_button_opens_a_list_of_names() {
    let dir = TempDir::new("recent-button");
    let files = patches(&dir.0, 1);
    let mut ui = Ui::with_app(app());
    assert!(ui.text("Recent").is_some());
    assert_eq!(ui.app.ui_snapshot().recent, None);
    ui.send(AppMessage::OpenRecent);
    assert_eq!(ui.app.ui_snapshot().recent, None, "nothing to list");

    assert!(ui.app.open_path(&files[0]));
    ui.redraw();
    ui.send(AppMessage::OpenRecent);
    ui.redraw();
    let rows = ui.app.ui_snapshot().recent.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "0.ron");
    assert_eq!(rows[0].folder, dir.0.display().to_string());
    assert!(!rows[0].missing);
    assert!(ui.text("0.ron").is_some());

    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(ui.app.ui_snapshot().recent, None);
}

/// Choosing a row asks about unsaved changes first, as Open does.
#[test]
fn choosing_a_row_asks_first() {
    let dir = TempDir::new("recent-asks");
    let files = patches(&dir.0, 1);
    let mut ui = Ui::with_app(app());
    ui.app.rig.recent.remember(&files[0]).unwrap();
    changed(&mut ui.app);
    ui.send(AppMessage::OpenRecent);
    ui.send(AppMessage::Recent(Message::Pick(0)));
    assert!(ui.app.overlays.question.is_some());
    assert_eq!(ui.app.document.file(), None);
    ui.send(AppMessage::Question(QuestionMessage::Answer(Answer::DontSave)));
    assert_eq!(ui.app.document.file(), Some(files[0].as_path()));
}

/// A missing file is dimmed. Choosing it takes it off the list, with a status
/// message and no question, however much is unsaved.
#[test]
fn a_missing_file_is_dimmed_and_removed_when_chosen() {
    let dir = TempDir::new("recent-missing");
    let files = patches(&dir.0, 2);
    let mut ui = Ui::with_app(app());
    ui.app.rig.recent.remember(&files[1]).unwrap();
    ui.app.rig.recent.remember(&files[0]).unwrap();
    std::fs::remove_file(&files[0]).unwrap();
    changed(&mut ui.app);
    ui.send(AppMessage::OpenRecent);
    let rows = ui.app.ui_snapshot().recent.unwrap();
    assert!(rows[0].missing);
    assert!(!rows[1].missing);
    ui.send(AppMessage::Recent(Message::Pick(0)));
    assert!(ui.app.overlays.question.is_none(), "nothing to ask about");
    assert_eq!(ui.app.rig.recent.files(), &files[1..]);
    assert!(ui.app.alerts.status().contains("0.ron"), "{}", ui.app.alerts.status());
}

/// A file that goes after the list opened is looked up again when chosen.
#[test]
fn a_file_is_looked_up_again_when_chosen() {
    let dir = TempDir::new("recent-gone");
    let files = patches(&dir.0, 1);
    let mut ui = Ui::with_app(app());
    ui.app.rig.recent.remember(&files[0]).unwrap();
    ui.send(AppMessage::OpenRecent);
    assert!(!ui.app.ui_snapshot().recent.unwrap()[0].missing);
    std::fs::remove_file(&files[0]).unwrap();
    ui.send(AppMessage::Recent(Message::Pick(0)));
    assert!(ui.app.rig.recent.files().is_empty());
    assert_eq!(ui.app.document.file(), None);
}

/// An app with its library at `library`, on an untitled patch.
fn app_with_library(library: &Path) -> App {
    let mut app = App::new(app_args());
    let mut preferences = app.rig.store.saved().clone();
    preferences.library = Some(library.to_path_buf());
    let _ = app.rig.store.save(preferences);
    let _ = fake::asked();
    app
}

/// Open and Save As on an untitled patch start in the library folder, and then
/// in the last folder used, which a new patch keeps. This is what a test of
/// Open alone would miss: Save As names a file in its folder.
#[test]
fn both_dialogs_start_in_the_remembered_folder() {
    let dir = TempDir::new("recent-folder");
    let library = dir.0.join("library");
    let project = dir.0.join("project");
    std::fs::create_dir_all(&library).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let mine = patches(&project, 1).remove(0);
    let mut app = app_with_library(&library);

    // First, the library.
    let _ = app.update(AppMessage::File(FileAction::Open));
    assert_eq!(fake::asked(), [Asked::Open(Some(library.clone()))]);
    assert!(!app.save_as());
    assert_eq!(fake::asked(), [Asked::Save(Some(library.join("Untitled.ron")))]);

    // A file picked in a dialog makes its folder the last one.
    fake::answer_open(Some(mine.clone()));
    let _ = app.update(AppMessage::File(FileAction::Open));
    fake::answer_open(None);
    let _ = fake::asked();
    assert_eq!(app.document.file(), Some(mine.as_path()));

    // With a current file, its folder.
    let _ = app.update(AppMessage::File(FileAction::Open));
    assert_eq!(fake::asked(), [Asked::Open(Some(project.clone()))]);

    // A new patch has none, so the last folder, for both.
    let _ = app.update(AppMessage::File(FileAction::New));
    let _ = app.update(AppMessage::File(FileAction::Open));
    assert_eq!(fake::asked(), [Asked::Open(Some(project.clone()))]);
    assert!(!app.save_as());
    assert_eq!(fake::asked(), [Asked::Save(Some(project.join("Untitled.ron")))]);

    // Once that folder is gone, the library again.
    std::fs::remove_dir_all(&project).unwrap();
    assert!(!app.save_as());
    assert_eq!(fake::asked(), [Asked::Save(Some(library.join("Untitled.ron")))]);
}

/// A patch saved with Save As makes its folder the last one too.
#[test]
fn save_as_remembers_its_folder() {
    let dir = TempDir::new("recent-save-as");
    let project = dir.0.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut app = app_with_library(&dir.0);
    fake::answer_save(Some(project.join("mine.ron")));
    assert!(app.save_as());
    fake::answer_save(None);
    assert_eq!(app.rig.recent.last_folder(), Some(project.as_path()));
}

/// The first patch among the files opens, whatever comes before it.
#[test]
fn a_drop_opens_the_first_patch() {
    let (ron, wav) = (PathBuf::from("/a/b.ron"), PathBuf::from("/a/c.wav"));
    assert_eq!(
        dropped(std::slice::from_ref(&ron)),
        Dropped::Open {
            path: ron.clone(),
            several: false
        }
    );
    assert_eq!(
        dropped(&[wav.clone(), ron.clone(), PathBuf::from("/a/d.ron")]),
        Dropped::Open {
            path: ron,
            several: true
        }
    );
    assert_eq!(
        dropped(&[PathBuf::from("/a/E.RON")]),
        Dropped::Open {
            path: PathBuf::from("/a/E.RON"),
            several: false
        }
    );
    assert_eq!(dropped(&[wav, PathBuf::from("/a/no-extension")]), Dropped::NoPatch);
}

/// The window's report of a dropped file becomes a message, and nothing else
/// does.
#[test]
fn a_dropped_file_becomes_a_message() {
    let path = PathBuf::from("/a/b.ron");
    let status = iced::event::Status::Ignored;
    let window = iced::window::Id::unique();
    let event = iced::Event::Window(iced::window::Event::FileDropped(path.clone()));
    assert!(matches!(
        drop_event(event, status, window),
        Some(AppMessage::FileDropped(found)) if found == path
    ));
    let event = iced::Event::Window(iced::window::Event::CloseRequested);
    assert!(drop_event(event, status, window).is_none());
}

/// Drops `files` on the window, as the window reports them, and lets the
/// moment for collecting them pass.
fn drop_files(app: &mut App, files: &[PathBuf]) {
    for file in files {
        let _ = app.update(AppMessage::FileDropped(file.clone()));
    }
    let _ = app.update(AppMessage::DropsDone);
}

/// One patch opens, after the question.
#[test]
fn dropping_one_patch_opens_it_after_the_question() {
    let dir = TempDir::new("recent-drop");
    let files = patches(&dir.0, 1);
    let mut app = app();
    changed(&mut app);
    drop_files(&mut app, &files);
    assert!(app.overlays.question.is_some());
    assert_eq!(app.document.file(), None);
    let _ = app.update(AppMessage::Question(QuestionMessage::Answer(Answer::DontSave)));
    assert_eq!(app.document.file(), Some(files[0].as_path()));
    assert_eq!(app.rig.recent.files(), &files[..1]);
    assert!(app.alerts.status().starts_with("opened"), "{}", app.alerts.status());
}

/// Several files open the first and say so.
#[test]
fn dropping_several_opens_the_first_and_says_so() {
    let dir = TempDir::new("recent-several");
    let files = patches(&dir.0, 2);
    let mut app = app();
    drop_files(&mut app, &files);
    assert_eq!(app.document.file(), Some(files[0].as_path()));
    assert!(
        app.alerts
            .status()
            .starts_with("Opened \"0.ron\". Drop one patch at a time."),
        "{}",
        app.alerts.status()
    );
}

/// That sentence must not lose what loaded differently.
#[test]
fn dropping_several_keeps_the_load_notes() {
    let dir = TempDir::new("recent-notes");
    // A patch with no Transport gets one, and a note says so.
    let files = patches(&dir.0, 1);
    let mut app = app();
    drop_files(&mut app, &[files[0].clone(), dir.0.join("other.wav")]);
    let note = app.document.load_notes().first().expect("a load note").clone();
    let status = app.alerts.status();
    assert!(
        status.starts_with("Opened \"0.ron\". Drop one patch at a time."),
        "{status}"
    );
    assert!(status.contains(&note), "{status}");
}

/// Anything else says so, and changes nothing.
#[test]
fn dropping_something_else_shows_the_message() {
    let mut app = app();
    changed(&mut app);
    drop_files(&mut app, &[PathBuf::from("/a/b.wav")]);
    assert_eq!(app.alerts.status(), "Only LoomDSP patches (.ron) can be opened here.");
    assert!(app.overlays.question.is_none());
    assert!(app.unsaved());
}

/// A drop while a question is open is ignored, with a status line, and leaves
/// nothing behind to act on later.
#[test]
fn a_drop_during_a_question_is_ignored() {
    let dir = TempDir::new("recent-busy");
    let files = patches(&dir.0, 1);
    let mut app = app();
    changed(&mut app);
    let _ = app.update(AppMessage::File(FileAction::Close));
    let before = app.ui_snapshot().question;
    drop_files(&mut app, &files);
    assert_eq!(app.ui_snapshot().question, before);
    assert!(
        app.alerts.status().contains("answer the question first"),
        "{}",
        app.alerts.status()
    );
    assert!(app.drops.is_empty());
    let _ = app.update(AppMessage::Question(QuestionMessage::Escape));
    assert_eq!(app.document.file(), None);
}

/// Overlays close only once the patch has opened, so Cancel changes nothing.
#[test]
fn overlays_stay_open_until_the_patch_opens() {
    let dir = TempDir::new("recent-overlay");
    let files = patches(&dir.0, 1);
    let mut app = app();
    let _ = app.update(AppMessage::Preferences(crate::settings::Message::Open));
    assert!(app.overlays.preferences().is_some());
    changed(&mut app);
    drop_files(&mut app, &files);
    let _ = app.update(AppMessage::Question(QuestionMessage::Answer(Answer::Cancel)));
    assert!(app.overlays.preferences().is_some(), "Cancel changes nothing");
    drop_files(&mut app, &files);
    let _ = app.update(AppMessage::Question(QuestionMessage::Answer(Answer::DontSave)));
    assert!(app.overlays.preferences().is_none());
}

/// With the list open, the clipboard shortcuts reach nothing behind it. Cut
/// would remove the selected modules.
#[test]
fn clipboard_keys_do_nothing_behind_the_list() {
    let dir = TempDir::new("recent-clipboard");
    let files = patches(&dir.0, 1);
    let mut ui = Ui::with_app(app());
    ui.app.rig.recent.remember(&files[0]).unwrap();
    let (id, _) = ui.app.controller.graph().nodes().next().expect("a module");
    ui.send(AppMessage::Canvas(loom_canvas::CanvasEvent::Select(vec![id.0])));
    ui.redraw();
    let count = ui.app.controller.graph().nodes().count();
    ui.send(AppMessage::OpenRecent);
    ui.redraw();
    for key in ["x", "d"] {
        ui.tap(Key::Character(key.into()), Modifiers::COMMAND);
        assert_eq!(ui.app.controller.graph().nodes().count(), count, "{key}");
    }
    assert!(ui.app.overlays.recent_rows().is_some(), "the list stays open");
}

/// Opening the list takes the keys from a focused matrix, so Delete removes
/// no assignment behind it, and one Escape closes the list.
#[test]
fn the_list_takes_the_keys_from_a_focused_matrix() {
    let dir = TempDir::new("recent-matrix");
    let files = patches(&dir.0, 1);
    let mut ui = Ui::with_app(app());
    ui.app.rig.recent.remember(&files[0]).unwrap();
    ui.app.matrix.tab = crate::matrix::Tab::Matrix;
    ui.app.matrix.focused = true;
    ui.redraw();
    ui.send(AppMessage::OpenRecent);
    ui.redraw();
    assert!(!ui.app.matrix.focused);
    let messages = ui.tap(Key::Named(Named::Delete), Modifiers::empty());
    assert!(
        !messages.iter().any(|m| matches!(m, AppMessage::MatrixKey(_))),
        "{messages:?}"
    );
    ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert!(ui.app.overlays.recent_rows().is_none(), "one Escape closes it");
}

/// Open with a current file starts in its folder, and does not make the
/// library folder it does not need.
#[test]
fn open_makes_no_library_folder_it_does_not_need() {
    let dir = TempDir::new("recent-no-library");
    let library = dir.0.join("library");
    let mine = patches(&dir.0, 1).remove(0);
    let mut app = app_with_library(&library);
    assert!(app.open_path(&mine));
    let _ = fake::asked();
    let _ = app.update(AppMessage::File(FileAction::Open));
    assert_eq!(fake::asked(), [Asked::Open(Some(dir.0.clone()))]);
    assert!(!library.exists());
}
