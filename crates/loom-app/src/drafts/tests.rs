//! Drafts at a save (`docs/design/files.md`, Asking before losing work, Text
//! still being typed; #897), tested through the window: real presses and
//! keys on the subtractive example, whose module 4 is a filter with cutoff
//! at index 0.

use crate::files::fake::{self, Asked};
use crate::question::{Answer, Message as QuestionMessage};
use crate::ui_test::Ui;
use crate::{FileAction, Message};
use iced::keyboard::{Key, Modifiers, key::Named};
use loom_core::{MidiControl, ModuleId};
use loom_ui_core::UiId;
use std::path::Path;

/// The filter in the subtractive example.
const FILTER: u32 = 4;
/// Its cutoff parameter.
const CUTOFF: usize = 0;

/// The three kinds of draft.
#[derive(Clone, Copy, Debug)]
enum Draft {
    Tag,
    Value,
    Range,
}

/// Every kind of draft.
const DRAFTS: [Draft; 3] = [Draft::Tag, Draft::Value, Draft::Range];

/// Text that commits, or text that cannot.
#[derive(Clone, Copy, Debug)]
enum Text {
    Good,
    Bad,
}

/// The subtractive example, clean and saved, with the filter's cutoff
/// mapped to a MIDI control and saved that way, so a range draft has a
/// mapping to edit.
fn clean() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut()
        .session
        .controller
        .map_midi(
            MidiControl {
                channel: 0,
                controller: 21,
            },
            ModuleId(FILTER),
            CUTOFF,
        )
        .unwrap();
    ui.app_mut().commit();
    ui.send(Message::File(FileAction::Save));
    assert!(!ui.app().unsaved(), "clean to start");
    let _ = fake::asked();
    ui
}

/// The patch's file.
fn file(ui: &Ui) -> std::path::PathBuf {
    ui.app()
        .session
        .document
        .file()
        .expect("the example has a file")
        .to_path_buf()
}

/// Types `text` into `kind`'s field, without committing it, and returns
/// the field. A bad tag is a 21st: the patch is given 20 tags first.
fn type_draft(ui: &mut Ui, kind: Draft, text: Text) -> UiId {
    match kind {
        Draft::Tag => {
            if matches!(text, Text::Bad) {
                ui.app_mut().session.document.edit_info(|info| {
                    info.tags = (0..loom_patch::metadata::MAX_TAGS)
                        .map(|n| format!("tag-{n}"))
                        .collect();
                });
            }
            ui.send(Message::Canvas(loom_canvas::CanvasEvent::Select(Vec::new())));
            let _ = ui.redraw();
            let part = crate::patch_info::tags_part();
            ui.focus(&part);
            let _ = ui.typewrite("drafted");
            assert_eq!(ui.app().session.patch_info.tag_draft, "drafted");
            part
        }
        Draft::Value => {
            ui.send(Message::Inspector(crate::inspector::Message::SelectModule(FILTER)));
            let at = ui.value_field(FILTER, CUTOFF);
            let _ = ui.move_to(at);
            let _ = ui.press();
            let _ = ui.release();
            let _ = ui.typewrite(match text {
                Text::Good => "1234",
                Text::Bad => "abc",
            });
            assert!(ui.app().session.value_draft.is_some());
            ui.app().value_field_part(FILTER, CUTOFF).unwrap()
        }
        Draft::Range => {
            ui.send(Message::Inspector(crate::inspector::Message::SelectModule(FILTER)));
            ui.send(Message::Learn(crate::learn::Message::MidiOptionsRows {
                module: FILTER,
                index: CUTOFF,
            }));
            let [low, _] = ui.app().midi_range_fields().unwrap();
            let at = ui.field(&low);
            let _ = ui.move_to(at);
            let _ = ui.press();
            let _ = ui.release();
            // Low must be below High (midi-learn.md).
            let (low_text, high_text) = match text {
                Text::Good => ("0.2", "0.8"),
                Text::Bad => ("0.9", "0.1"),
            };
            ui.send(Message::Learn(crate::learn::Message::MidiRangeText {
                module: FILTER,
                index: CUTOFF,
                low: low_text.into(),
                high: high_text.into(),
            }));
            assert!(ui.app().session.learn.range_draft_param().is_some());
            low
        }
    }
}

/// The filter's cutoff in the patch.
fn cutoff(ui: &Ui) -> f32 {
    ui.app()
        .session
        .controller
        .graph()
        .node(ModuleId(FILTER))
        .unwrap()
        .params[CUTOFF]
}

/// True if the file at `path` holds `kind`'s good draft.
fn holds(path: &Path, kind: Draft) -> bool {
    let saved = crate::patch_file::load(path).expect("the file loads");
    match kind {
        Draft::Tag => saved.metadata.tags.iter().any(|tag| tag == "drafted"),
        Draft::Value => saved
            .modules
            .iter()
            .find(|module| module.id == FILTER)
            .and_then(|module| module.params.get("cutoff").copied())
            .is_some_and(|hz| (hz - 1234.0).abs() < 1.0),
        Draft::Range => saved
            .midi
            .iter()
            .any(|m| m.target.0 == FILTER && (m.range.0 - 0.2).abs() < 1e-6 && (m.range.1 - 0.8).abs() < 1e-6),
    }
}

/// Answers the open question, as its button does.
fn answer(ui: &mut Ui, answer: Answer) {
    ui.send(Message::Question(QuestionMessage::Answer(answer)));
}

/// True if nothing asked where to save.
fn no_save_dialog() -> bool {
    fake::asked().iter().all(|asked| !matches!(asked, Asked::Save(_)))
}

/// After a save stopped on `field`'s draft: the focus is on the field, its
/// node gives the reason, the status line says "Not saved", and the draft
/// is kept.
fn assert_refused(ui: &mut Ui, field: &UiId, kind: Draft) {
    let _ = ui.redraw();
    assert_eq!(
        ui.focused_field().as_ref(),
        Some(field),
        "{kind:?}: the focus is on the field"
    );
    let tree = ui.a11y_tree();
    let reason = tree.node_by_id(field).description().map(str::to_string);
    assert!(
        reason.is_some_and(|r| !r.is_empty()),
        "{kind:?}: its node gives the reason"
    );
    let status = ui.app().alerts.status().to_string();
    assert!(status.starts_with("Not saved: "), "{kind:?}: {status}");
    assert!(ui.app().has_draft(), "{kind:?}: the draft is kept");
}

/// The question's Save commits each kind of draft, as Enter would, so the
/// file holds what was typed, and then the close goes on. Before #897 the
/// file lacked the draft.
#[test]
fn the_questions_save_writes_each_draft() {
    for kind in DRAFTS {
        let mut ui = clean();
        let path = file(&ui);
        let _ = type_draft(&mut ui, kind, Text::Good);
        ui.send(Message::File(FileAction::Close));
        assert!(
            ui.app().overlays.question.is_some(),
            "{kind:?}: the draft makes Close ask"
        );
        answer(&mut ui, Answer::Save);
        assert!(holds(&path, kind), "{kind:?} is in the file");
        assert!(ui.app().quit, "{kind:?}: the close went on");
    }
}

/// A draft that cannot commit stops the question's Save: nothing is
/// written, the app stays, the question is gone, and the field has the
/// focus with its reason.
#[test]
fn a_bad_draft_stops_the_questions_save() {
    for kind in DRAFTS {
        let mut ui = clean();
        let path = file(&ui);
        let before = std::fs::read(&path).unwrap();
        let field = type_draft(&mut ui, kind, Text::Bad);
        ui.send(Message::File(FileAction::Close));
        answer(&mut ui, Answer::Save);
        assert_eq!(std::fs::read(&path).unwrap(), before, "{kind:?}: the file is unchanged");
        assert!(!ui.app().quit, "{kind:?}: the app stays");
        assert!(ui.app().overlays.question.is_none(), "{kind:?}: the question closed");
        assert_refused(&mut ui, &field, kind);
    }
}

/// Don't Save goes on without the drafts, even one that cannot commit:
/// only Save can be stopped.
#[test]
fn dont_save_drops_the_drafts() {
    for text in [Text::Good, Text::Bad] {
        for kind in DRAFTS {
            let mut ui = clean();
            let path = file(&ui);
            let before = std::fs::read(&path).unwrap();
            let _ = type_draft(&mut ui, kind, text);
            ui.send(Message::File(FileAction::Close));
            answer(&mut ui, Answer::DontSave);
            assert!(ui.app().quit, "{kind:?} {text:?}: Don't Save quits");
            assert_eq!(
                std::fs::read(&path).unwrap(),
                before,
                "{kind:?} {text:?}: nothing written"
            );
            assert!(no_save_dialog());
        }
    }
}

/// Cancel gives the field the focus back with its draft, and the patch did
/// not change under the question.
#[test]
fn cancel_keeps_the_draft_and_its_focus() {
    for kind in DRAFTS {
        let mut ui = clean();
        let field = type_draft(&mut ui, kind, Text::Good);
        let before = ui.app().current_patch();
        ui.send(Message::File(FileAction::Close));
        answer(&mut ui, Answer::Cancel);
        let _ = ui.redraw();
        assert_eq!(ui.focused_field().as_ref(), Some(&field), "{kind:?}: the focus is back");
        assert!(ui.app().has_draft(), "{kind:?}: the draft is there");
        assert_eq!(ui.app().current_patch(), before, "{kind:?}: the patch did not change");
    }
}

/// Cmd+S with a draft in a focused field writes the draft, and the field
/// keeps the focus with the committed text.
#[test]
fn save_commits_the_focused_fields_draft() {
    for kind in DRAFTS {
        let mut ui = clean();
        let path = file(&ui);
        let field = type_draft(&mut ui, kind, Text::Good);
        ui.tap(Key::Character("s".into()), Modifiers::COMMAND);
        assert!(holds(&path, kind), "{kind:?} is in the file");
        let _ = ui.redraw();
        assert_eq!(
            ui.focused_field().as_ref(),
            Some(&field),
            "{kind:?}: the field keeps the focus"
        );
        assert!(!ui.app().has_draft(), "{kind:?}: the draft committed");
        assert!(!ui.app().unsaved(), "{kind:?}: saved");
    }
}

/// A press on the toolbar's Save button is not leaving the field: a draft
/// that cannot commit stays, and stops the save with its reason, rather
/// than being dropped by the field's leave rule. A toolbar button takes no
/// focus from a press, so the field keeps it and its leave rule never runs;
/// this guards that.
#[test]
fn a_press_on_save_keeps_a_bad_draft() {
    for kind in DRAFTS {
        let mut ui = clean();
        let path = file(&ui);
        let before = std::fs::read(&path).unwrap();
        let field = type_draft(&mut ui, kind, Text::Bad);
        let save = crate::toolbar::toolbar_id().with("save");
        let at = ui.field(&save);
        let _ = ui.move_to(at);
        let _ = ui.press();
        let _ = ui.release();
        assert_eq!(std::fs::read(&path).unwrap(), before, "{kind:?}: nothing written");
        assert_refused(&mut ui, &field, kind);
    }
}

/// A press on the toolbar's Save button writes a good draft.
#[test]
fn a_press_on_save_writes_the_draft() {
    for kind in DRAFTS {
        let mut ui = clean();
        let path = file(&ui);
        let _ = type_draft(&mut ui, kind, Text::Good);
        let save = crate::toolbar::toolbar_id().with("save");
        let at = ui.field(&save);
        let _ = ui.move_to(at);
        let _ = ui.press();
        let _ = ui.release();
        assert!(holds(&path, kind), "{kind:?} is in the file");
        assert!(!ui.app().unsaved(), "{kind:?}: saved");
    }
}

/// Save As stops on a draft that cannot commit before its dialog opens.
#[test]
fn a_bad_draft_stops_save_as() {
    for kind in DRAFTS {
        let mut ui = clean();
        let field = type_draft(&mut ui, kind, Text::Bad);
        ui.send(Message::File(FileAction::SaveAs));
        assert!(no_save_dialog(), "{kind:?}: no dialog");
        assert_refused(&mut ui, &field, kind);
    }
}

/// Undo after a save takes back a committed value in one step.
#[test]
fn undo_after_save_takes_back_the_value() {
    let mut ui = clean();
    let before = cutoff(&ui);
    let _ = type_draft(&mut ui, Draft::Value, Text::Good);
    ui.send(Message::File(FileAction::Save));
    assert!((cutoff(&ui) - 1234.0).abs() < 1.0, "the value committed");
    ui.send(Message::Undo);
    assert_eq!(cutoff(&ui), before, "one step takes it back");
}

/// A draft on a clean patch marks the title and makes Close ask, with no
/// undo step; Don't Save then quits without writing.
#[test]
fn a_draft_alone_marks_the_title_and_asks() {
    for kind in DRAFTS {
        let mut ui = clean();
        let path = file(&ui);
        let before = std::fs::read(&path).unwrap();
        assert!(!ui.app().title().starts_with("* "), "{kind:?}: clean to start");
        let _ = type_draft(&mut ui, kind, Text::Good);
        assert!(ui.app().title().starts_with("* "), "{kind:?}: {}", ui.app().title());
        assert!(ui.app().session.edits.history.is_saved(), "{kind:?}: no undo step");
        ui.send(Message::File(FileAction::Close));
        assert!(ui.app().overlays.question.is_some(), "{kind:?}: Close asks");
        answer(&mut ui, Answer::DontSave);
        assert!(ui.app().quit);
        assert_eq!(std::fs::read(&path).unwrap(), before, "{kind:?}: nothing written");
    }
}

/// New asks too when a draft is the only unsaved work.
#[test]
fn a_draft_alone_makes_new_ask() {
    let mut ui = clean();
    let _ = type_draft(&mut ui, Draft::Value, Text::Good);
    ui.send(Message::File(FileAction::New));
    assert!(ui.app().overlays.question.is_some(), "New asks");
}

/// Escape drops the draft, and on a patch with nothing else unsaved the
/// mark clears.
#[test]
fn escape_clears_the_mark_on_a_clean_patch() {
    for kind in DRAFTS {
        let mut ui = clean();
        let _ = type_draft(&mut ui, kind, Text::Good);
        ui.tap(Key::Named(Named::Escape), Modifiers::empty());
        assert!(!ui.app().has_draft(), "{kind:?}: dropped");
        assert!(!ui.app().title().starts_with("* "), "{kind:?}: {}", ui.app().title());
    }
}

/// `commit_drafts` keeps a refused tag in the field with its note, where
/// Enter clears the field.
#[test]
fn a_refused_tag_stays_a_draft_at_a_save() {
    let mut ui = clean();
    let _ = type_draft(&mut ui, Draft::Tag, Text::Bad);
    let refused = ui.app_mut().commit_drafts().expect_err("a 21st tag is refused");
    assert_eq!(refused.label, "Tags");
    assert_eq!(ui.app().session.patch_info.tag_draft, "drafted");
    assert!(ui.app().session.patch_info.tag_note.is_some());
}

/// With no draft, `commit_drafts` changes nothing.
#[test]
fn no_draft_commits_nothing() {
    let mut ui = clean();
    let before = ui.app().current_patch();
    assert_eq!(ui.app_mut().commit_drafts(), Ok(()));
    assert_eq!(ui.app().current_patch(), before);
    assert!(!ui.app().unsaved());
}
