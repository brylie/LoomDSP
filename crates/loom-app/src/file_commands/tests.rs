use crate::Source;
use crate::files::fake::{self, Asked};
use crate::recovery::tests::TempDir;
use crate::tests::app;
use crate::ui_test::Ui;
use iced::keyboard::{Key, Modifiers};

/// A patch that does not load opens an error dialog, and the status line
/// says the same. The running patch stays.
#[test]
fn a_failed_open_shows_the_reason_in_a_dialog() {
    let tmp = TempDir::new("open-fails");
    let mut app = app();
    let _ = fake::asked();
    let generation = app.generation;

    assert!(!app.replace(Source::File(&tmp.0.join("missing.ron"))));

    let status = app.alerts.status().to_string();
    assert!(status.starts_with("Could not open the patch"), "{status}");
    assert!(status.ends_with("The current patch is unchanged."), "{status}");
    assert_eq!(fake::asked(), vec![Asked::Error(status)]);
    assert_eq!(app.generation, generation, "the engine was not rebuilt");
}

/// A save that cannot be written opens an error dialog after Save As,
/// and the patch stays untitled.
#[test]
fn a_failed_save_shows_the_reason_in_a_dialog() {
    let tmp = TempDir::new("save-fails");
    // A file where the folder should be: the save cannot be written.
    std::fs::write(tmp.0.join("blocked"), "").unwrap();
    fake::answer_save(Some(tmp.0.join("blocked").join("patch.ron")));
    let mut ui = Ui::with_app(app());
    let _ = fake::asked();

    ui.tap(Key::Character("s".into()), Modifiers::COMMAND);

    let status = ui.app.alerts.status().to_string();
    assert!(status.starts_with("Could not save \"patch.ron\""), "{status}");
    assert!(status.ends_with("Your changes are not saved."), "{status}");
    let asked = fake::asked();
    assert!(
        matches!(asked.as_slice(), [Asked::Save(_), Asked::Error(_)]),
        "{asked:?}"
    );
    assert_eq!(asked[1], Asked::Error(status));
    assert_eq!(ui.app.document.file(), None);
}
