//! Tests for the bridge to Iced: one turn of our runtime per Iced message.

use super::*;
use crate::tests::app;
use loom_ui::Target as _;

/// An Iced message becomes one turn with one update. The bridge's own
/// hand-off to the queue wakes nothing, so Iced gets no second message
/// for it.
#[test]
fn a_message_from_iced_is_one_turn_with_no_wake_up() {
    let (mut bridge, _) = Bridge::start(app(), Task::none());
    let before = bridge.runtime.counts();
    let _ = bridge.update(Bridged::App(Message::ShowHelp));
    let after = bridge.runtime.counts();
    assert_eq!(after.turns - before.turns, 1);
    assert_eq!(after.updates - before.updates, 1);
    assert!(bridge.app().overlays.help.is_some(), "the update ran");
    let mut wakes = bridge
        .wakes
        .0
        .lock()
        .unwrap()
        .take()
        .expect("the receiver, not yet taken");
    assert!(wakes.try_recv().is_err(), "no wake-up was sent");
}

/// A waker used from another thread wakes Iced, and the next turn takes
/// its message.
#[test]
fn a_waker_wakes_iced_and_the_next_turn_takes_its_message() {
    let (mut bridge, _) = Bridge::start(app(), Task::none());
    let waker = bridge.runtime.waker();
    std::thread::spawn(move || waker.send(Message::ShowHelp))
        .join()
        .unwrap();
    let mut wakes = bridge.wakes.0.lock().unwrap().take().expect("the receiver");
    assert!(wakes.try_recv().is_ok(), "one wake-up");
    let _ = bridge.update(Bridged::Woken);
    assert!(bridge.app().overlays.help.is_some());
}

/// The first turn reads the first deadline. Here nothing is due, so
/// nothing more runs.
#[test]
fn the_first_turn_reads_the_deadline() {
    let (bridge, _) = Bridge::start(app(), Task::none());
    assert_eq!(bridge.runtime.counts().turns, 1);
    assert!(bridge.app().next_deadline().is_none());
}

/// The outbox keeps tasks that do something, and drops empty ones.
#[test]
fn the_outbox_keeps_only_tasks_with_work() {
    let outbox = Outbox::default();
    outbox.push(Task::none());
    assert_eq!(outbox.len(), 0);
    outbox.push(Task::done(Message::ShowHelp));
    assert_eq!(outbox.len(), 1);
    assert_eq!(outbox.take().len(), 1);
    assert_eq!(outbox.len(), 0);
}

/// A focus effect becomes an Iced widget operation; until the focus pull
/// request, nothing reads focus back.
#[test]
fn focus_becomes_a_widget_task() {
    let mut target = IcedTarget::default();
    let field = UiId::panel("inspector").with("field");
    loom_ui::Target::<Message>::focus(&mut target, &FocusTarget::Element(field), Selection::All);
    loom_ui::Target::<Message>::focus(&mut target, &FocusTarget::Nothing, Selection::Keep);
    assert_eq!(target.tasks.len(), 2);
    assert!(loom_ui::Target::<Message>::focused(&target).is_none());
    assert!(target.needs_redraw(1), "Iced draws after an update");
    assert!(!target.needs_redraw(0), "and not after a turn with none");
}

/// An element's Iced widget ID is the text of its UI ID.
#[test]
fn a_widget_id_is_the_ui_id_text() {
    let id = UiId::panel("inspector").with("4").with("field");
    assert_eq!(widget_id(&id), iced::advanced::widget::Id::from(id.to_string()));
}

/// A dialog's result names its button by label, or by place where the
/// platform reports a standard one.
#[test]
fn a_dialog_result_names_its_button() {
    use rfd::MessageDialogResult as R;
    let three = ["Restore".to_string(), "Discard".to_string(), "Not Now".to_string()];
    assert_eq!(choice(&three, R::Custom("Discard".into())), Some(1));
    assert_eq!(choice(&three, R::Custom("Not Now".into())), Some(2));
    assert_eq!(choice(&three, R::Yes), Some(0));
    assert_eq!(choice(&three, R::No), Some(1));
    assert_eq!(choice(&three, R::Cancel), None);
    let two = ["Add my name".to_string(), "Not for this patch".to_string()];
    assert_eq!(choice(&two, R::Ok), Some(0));
    assert_eq!(choice(&two, R::No), None);
    assert_eq!(choice(&two, R::Custom("Something else".into())), None);
    assert_eq!(choice(&[], R::Ok), None, "no button, no index");
}

/// The hosted app's window spec carries the title Iced shows.
#[test]
fn the_hosted_app_describes_its_window() {
    let hosted = Hosted(app());
    let spec = loom_ui::App::window(&hosted);
    assert_eq!(spec.title, hosted.0.title());
}
