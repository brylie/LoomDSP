//! Tests for dialogs in two turns: the question is an effect, and its
//! answer is the next turn's message.

use super::*;
use crate::FileAction;
use crate::files::fake;
use crate::recovery::tests::TempDir;
use crate::tests::app;
use loom_ui::effect::FileAction as DialogAction;
use loom_ui::{Context, Effect, Waker, WindowState};
use std::sync::{Arc, mpsc};

/// One update as the runtime runs it, with no driver: its effects come
/// back unrun, so a test sees the dialog as a value.
fn update(app: &mut App, message: Message) -> Vec<Effect<Message>> {
    let (sender, _queue) = mpsc::channel();
    let waker = Waker::new(sender, Arc::new(|| {}));
    let window = WindowState::new(loom_ui_core::Size::new(800.0, 600.0));
    let cx = Context::new(Instant::now(), &waker, None, window);
    app.update(message, &cx).into_iter().collect()
}

/// A patch file in `dir`, with one module more than an empty patch.
fn patch_in(dir: &TempDir) -> PathBuf {
    let path = dir.0.join("other.ron");
    std::fs::write(
        &path,
        r#"Patch(version: 1, metadata: Metadata(name: "Other"), modules: [PatchModule(id: 1, kind: "loom.osc.sine@1")])"#,
    )
    .unwrap();
    path
}

/// Open asks with a dialog and opens nothing in the same turn; the patch
/// opens on the turn its answer arrives.
#[test]
fn open_shows_a_dialog_and_opens_on_its_answer() {
    let dir = TempDir::new("dialogs-open");
    let path = patch_in(&dir);
    let mut app = app();
    let effects = update(&mut app, Message::File(FileAction::Open));
    assert!(
        matches!(
            effects.as_slice(),
            [Effect::FileDialog { request, .. }] if request.action == DialogAction::Open && request.title == "Open patch"
        ),
        "{effects:?}"
    );
    assert!(matches!(app.waiting(), Some(Waiting::Open)));
    assert!(app.session.document.file().is_none(), "nothing opens before the answer");

    let effects = update(&mut app, Message::Dialog(DialogAnswer::File(Some(path.clone()))));
    assert!(effects.is_empty(), "{effects:?}");
    assert_eq!(app.session.document.file(), Some(path.as_path()));
    assert!(app.waiting.is_none());
    let _ = fake::asked();
}

/// A cancelled dialog changes nothing, and lets the next one be asked.
#[test]
fn a_cancelled_dialog_changes_nothing() {
    let mut app = app();
    let _ = update(&mut app, Message::File(FileAction::Open));
    let _ = update(&mut app, Message::Dialog(DialogAnswer::File(None)));
    assert!(app.session.document.file().is_none());
    assert!(app.waiting.is_none());
    let effects = update(&mut app, Message::File(FileAction::Open));
    assert!(matches!(effects.as_slice(), [Effect::FileDialog { .. }]));
    let _ = fake::asked();
}

/// Save As keeps what was asked for after the save, here Close, and does
/// it once the dialog answers and the patch is written.
#[test]
fn save_as_carries_what_comes_after_it() {
    let dir = TempDir::new("dialogs-save-as");
    let path = dir.0.join("mine.ron");
    let mut app = app();
    assert!(!app.save_as_then(Some(AfterDiscard::Close)), "not written yet");
    assert!(matches!(
        app.waiting(),
        Some(Waiting::SaveAs {
            next: Some(AfterDiscard::Close)
        })
    ));
    assert!(!app.quit);
    let effects = update(&mut app, Message::Dialog(DialogAnswer::File(Some(path.clone()))));
    assert!(path.is_file());
    assert!(app.quit, "Close ran after the save");
    assert!(
        effects.iter().any(|effect| matches!(effect, Effect::Quit)),
        "{effects:?}"
    );
    let _ = fake::asked();
}

/// While one dialog waits, a second is not asked: its request is dropped,
/// and what waits stays.
#[test]
fn a_second_dialog_waits_for_none() {
    let mut app = app();
    let _ = update(&mut app, Message::File(FileAction::Open));
    let effects = update(&mut app, Message::File(FileAction::SaveAs));
    assert!(
        !effects.iter().any(|effect| matches!(effect, Effect::FileDialog { .. })),
        "{effects:?}"
    );
    assert!(matches!(app.waiting(), Some(Waiting::Open)));
    let _ = fake::asked();
}

/// An answer with nothing waiting for it changes nothing.
#[test]
fn an_answer_with_nothing_waiting_changes_nothing() {
    let dir = TempDir::new("dialogs-stray");
    let path = patch_in(&dir);
    let mut app = app();
    let effects = update(&mut app, Message::Dialog(DialogAnswer::File(Some(path))));
    assert!(effects.is_empty());
    assert!(app.session.document.file().is_none());
}

/// With nothing to offer there is no deadline; the start patch kept for
/// the question is the one the command line names.
#[test]
fn no_offer_no_deadline_and_the_start_patch_follows_the_command_line() {
    let app = app();
    assert!(app.next_deadline().is_none());
    let mut args = crate::tests::app_args();
    assert_eq!(StartPatch::of(&args), StartPatch::Empty);
    args.generate = Some(3);
    assert_eq!(StartPatch::of(&args), StartPatch::Generated(3));
    args.generate = None;
    args.patch = Some(PathBuf::from("a.ron"));
    assert_eq!(StartPatch::of(&args), StartPatch::File(PathBuf::from("a.ron")));
}

/// While another dialog waits, the recovery question is not due, so the
/// loop does not wake for it again and again; once that dialog answers,
/// it is due again.
#[test]
fn the_recovery_question_is_not_due_while_another_dialog_waits() {
    let dir = TempDir::new("dialogs-offer-waits");
    let mut app = app();
    app.recovery_offer = Some(RecoveryOffer {
        orphan: recovery::tests::orphan_for(&dir.0, None),
        start: StartPatch::Empty,
        found: Instant::now(),
    });
    assert!(app.next_deadline().is_some(), "due at once");
    let _ = update(&mut app, Message::File(FileAction::Open));
    assert!(app.waiting.is_some());
    assert!(app.next_deadline().is_none(), "not due while the Open dialog waits");
    let _ = update(&mut app, Message::Dialog(DialogAnswer::File(None)));
    assert!(app.next_deadline().is_some(), "due again once it answers");
    let _ = fake::asked();
}

/// A Save As answer that arrives after the patch was replaced writes
/// nothing: it belonged to the patch it was asked about, and the new one
/// must not go where the old one was to be saved.
#[test]
fn an_answer_for_a_replaced_patch_is_not_used() {
    let dir = TempDir::new("dialogs-stale");
    let path = dir.0.join("mine.ron");
    let mut app = app();
    assert!(!app.save_as_then(Some(AfterDiscard::Close)));
    assert!(app.replace(Source::Empty), "New, by a script, while the dialog is open");
    let _ = update(&mut app, Message::Dialog(DialogAnswer::File(Some(path.clone()))));
    assert!(!path.exists(), "nothing was written");
    assert!(!app.quit, "and Close did not run");
    assert!(app.waiting.is_none());
    assert!(
        app.alerts.status().contains("the dialog was open"),
        "{}",
        app.alerts.status()
    );
    let _ = fake::asked();
}

/// An Open answer is used after a replace: what the player picked to open
/// does not depend on the patch that was showing. Unsaved changes in the
/// patch that came meanwhile are asked about first.
#[test]
fn an_open_answer_is_used_after_a_replace() {
    let dir = TempDir::new("dialogs-open-replaced");
    let path = patch_in(&dir);
    let mut app = app();
    let _ = update(&mut app, Message::File(FileAction::Open));
    assert!(app.replace(Source::Empty));
    app.edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    let _ = update(&mut app, Message::Dialog(DialogAnswer::File(Some(path.clone()))));
    assert!(
        app.overlays.question.is_some(),
        "the new patch's changes are asked about"
    );
    assert!(app.session.document.file().is_none(), "not opened over them yet");

    let mut clean = crate::tests::app();
    let _ = update(&mut clean, Message::File(FileAction::Open));
    assert!(clean.replace(Source::Empty));
    let _ = update(&mut clean, Message::Dialog(DialogAnswer::File(Some(path.clone()))));
    assert_eq!(clean.session.document.file(), Some(path.as_path()), "opened");
    let _ = fake::asked();
}

/// The recovery question's answer is the exception: it is what replaces
/// the patch, so it is used whatever the patch is meanwhile.
#[test]
fn a_recovery_answer_is_used_after_a_replace() {
    let dir = TempDir::new("dialogs-stale-recovery");
    let mut app = app();
    app.recovery_offer = Some(RecoveryOffer {
        orphan: recovery::tests::orphan_for(&dir.0, None),
        start: StartPatch::Generated(2),
        found: Instant::now(),
    });
    let _ = update(&mut app, Message::AskRecovery);
    assert!(app.replace(Source::Empty));
    let modules = app.session.controller.graph().nodes().count();
    let _ = update(&mut app, Message::Dialog(DialogAnswer::Choice(Some(2))));
    assert!(
        app.session.controller.graph().nodes().count() > modules,
        "Not Now opened the start patch"
    );
}
