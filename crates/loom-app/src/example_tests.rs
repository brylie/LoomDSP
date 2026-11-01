//! Examples open as untitled copies (`docs/design/files.md`, Defaults for
//! the dialogs, issue #182). Each test uses a temporary bundled examples
//! folder, and fake dialogs that record what they were asked.

use super::*;
use crate::files::fake::{self, Asked};
use crate::recovery::tests::TempDir;
use crate::ui_test::Ui;
use iced::keyboard::{Key, Modifiers};
use std::fs;

/// A temporary bundled examples folder holding `subtractive.ron`, which
/// starts with comments.
struct Examples {
    tmp: TempDir,
}

impl Examples {
    fn new(label: &str) -> Self {
        let tmp = TempDir::new(label);
        fs::create_dir_all(tmp.0.join("examples")).unwrap();
        let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
        fs::copy(source, tmp.0.join("examples/subtractive.ron")).unwrap();
        fake::use_examples(Some(tmp.0.join("examples")));
        Self { tmp }
    }

    /// The example file.
    fn example(&self) -> PathBuf {
        self.tmp.0.join("examples/subtractive.ron")
    }

    /// The example as it is on disk now.
    fn example_text(&self) -> String {
        fs::read_to_string(self.example()).unwrap()
    }

    /// Every file in the examples folder.
    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.tmp.0.join("examples"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

/// An app started with `patch` on the command line.
fn app_with(patch: PathBuf) -> App {
    App::new(Args {
        patch: Some(patch),
        ..tests::app_args()
    })
}

/// Presses Cmd/Ctrl+S.
fn press_save(ui: &mut Ui) {
    ui.tap(Key::Character("s".into()), Modifiers::COMMAND);
}

/// Turns the filter's cutoff, as a slider would: one undo step.
fn turn_knob(app: &mut App) {
    let _ = app.update(Message::Param {
        module: 4,
        index: 0,
        position: 0.8,
    });
    let _ = app.update(Message::SliderReleased);
}

/// An example given on the command line opens as an untitled copy, clean
/// until edited, so closing the window asks nothing.
#[test]
fn an_example_opens_as_a_clean_untitled_copy() {
    let examples = Examples::new("example-copy");
    let mut app = app_with(examples.example());
    assert_eq!(app.ui_snapshot().title, "subtractive (example) - LoomDSP");
    assert!(app.document.file().is_none());
    assert!(!app.unsaved());

    let _ = app.update(Message::File(crate::FileAction::Close));
    assert!(app.quit, "a clean copy closes");
    assert_eq!(fake::asked(), Vec::new(), "and asks nothing");
}

/// An example opened from a release folder is still a copy when macOS runs
/// the app from a translocated location and cannot find that folder.
#[test]
fn a_marked_release_example_is_a_copy_without_a_known_folder() {
    let tmp = TempDir::new("translocated-example");
    let patches = tmp.0.join("examples/patches");
    fs::create_dir_all(&patches).unwrap();
    fs::write(patches.join(".loomdsp-bundled-examples"), "").unwrap();
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
    let example = patches.join("subtractive.ron");
    fs::copy(source, &example).unwrap();
    fake::use_examples(None);

    let app = app_with(example);
    assert!(app.document.file().is_none());
    assert_eq!(app.ui_snapshot().title, "subtractive (example) - LoomDSP");
    assert!(!app.unsaved());
}

/// Save on an example copy asks for a place, suggesting the example's
/// file name in the Documents folder, and never writes the example. The
/// place chosen becomes the current file. No `*.saved.ron` file appears.
#[test]
fn saving_an_example_asks_for_a_place_and_keeps_the_example() {
    let examples = Examples::new("example-save");
    let before = examples.example_text();
    let mut ui = Ui::with_app(app_with(examples.example()));
    turn_knob(&mut ui.app);
    assert_eq!(ui.app.ui_snapshot().title, "* subtractive (example) - LoomDSP");

    // Cancelled: nothing is written, and the mark stays.
    press_save(&mut ui);
    assert_eq!(fake::asked(), vec![Asked::Save(Some(PathBuf::from("subtractive.ron")))]);
    assert!(ui.app.unsaved());
    assert_eq!(examples.example_text(), before);

    let mine = examples.tmp.0.join("mine.ron");
    fake::answer_save(Some(mine.clone()));
    press_save(&mut ui);
    assert_eq!(fake::asked().len(), 1);
    assert!(mine.is_file());
    assert_eq!(ui.app.document.file(), Some(mine.as_path()));
    assert_eq!(ui.app.ui_snapshot().title, "mine.ron - LoomDSP");
    assert_eq!(examples.example_text(), before, "the example keeps its comments");
    assert_eq!(examples.files(), vec!["subtractive.ron".to_string()]);

    // From now on Save writes the chosen file, with no dialog.
    turn_knob(&mut ui.app);
    press_save(&mut ui);
    assert_eq!(fake::asked(), Vec::new());
    assert!(!ui.app.unsaved());
}

/// File > Open on an example, after its dialog, gives a copy too. The
/// unsaved changes question names the copy as the title does.
#[test]
fn opening_an_example_from_a_file_gives_a_copy() {
    let examples = Examples::new("example-open");
    let mut app = tests::app();
    app.open_path(&examples.example());
    assert!(app.document.file().is_none());
    assert_eq!(app.ui_snapshot().title, "subtractive (example) - LoomDSP");

    turn_knob(&mut app);
    let _ = app.update(Message::File(crate::FileAction::Close));
    assert_eq!(fake::asked(), vec![Asked::Discard("subtractive (example)".to_string())]);
    assert!(!app.quit, "the fake answers Cancel");
}

/// A folder that is only named `examples`, outside the bundled one, holds
/// the player's own patches: they open as themselves, and Save writes them
/// in place with no dialog.
#[test]
fn a_folder_named_examples_elsewhere_is_not_a_copy() {
    let examples = Examples::new("example-elsewhere");
    let own = examples.tmp.0.join("music/examples");
    fs::create_dir_all(&own).unwrap();
    let set = own.join("set.ron");
    fs::copy(examples.example(), &set).unwrap();

    let mut ui = Ui::with_app(app_with(set.clone()));
    assert_eq!(ui.app.document.file(), Some(set.as_path()));
    assert_eq!(ui.app.ui_snapshot().title, "set.ron - LoomDSP");
    turn_knob(&mut ui.app);
    press_save(&mut ui);
    assert_eq!(fake::asked(), Vec::new());
    assert!(!ui.app.unsaved());
    assert_ne!(
        fs::read_to_string(&set).unwrap(),
        examples.example_text(),
        "saved in place"
    );
}
