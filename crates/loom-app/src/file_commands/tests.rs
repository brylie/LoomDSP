use crate::Source;
use crate::files::fake::{self, Asked};
use crate::recovery::tests::TempDir;
use crate::tests::app;
use crate::ui_test::Ui;
use iced::keyboard::{Key, Modifiers};

/// A patch that does not load shows the reason in the window, and the status
/// line says the same. The running patch stays.
#[test]
fn a_failed_open_shows_the_reason_in_the_window() {
    let tmp = TempDir::new("open-fails");
    let mut app = app();
    let _ = fake::asked();
    let generation = app.generation;

    assert!(!app.replace(Source::File(&tmp.0.join("missing.ron"))));

    let status = app.alerts.status().to_string();
    assert!(status.starts_with("Could not open the patch"), "{status}");
    assert!(status.ends_with("The current patch is unchanged."), "{status}");
    assert_eq!(app.ui_snapshot().question.unwrap().text, status);
    assert_eq!(app.generation, generation, "the engine was not rebuilt");
}

/// A save that cannot be written shows the reason in the window after Save As,
/// and the patch stays untitled.
#[test]
fn a_failed_save_shows_the_reason_in_the_window() {
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
    assert!(matches!(asked.as_slice(), [Asked::Save(_)]), "{asked:?}");
    assert_eq!(ui.app.ui_snapshot().question.unwrap().text, status);
    assert_eq!(ui.app.document.file(), None);
}
