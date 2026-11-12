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

/// A scroll moves only the scrolling area the walk meets in its place, by
/// the move it was given, and leaves every other area alone.
#[test]
fn a_scroll_moves_only_its_own_area() {
    use iced::advanced::widget::Operation;
    use iced::advanced::widget::operation::Scrollable;
    use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, RelativeOffset};
    /// Records each move it is given.
    #[derive(Default)]
    struct Area(Vec<(f32, f32)>);
    impl Scrollable for Area {
        fn snap_to(&mut self, _: RelativeOffset<Option<f32>>) {}
        fn scroll_to(&mut self, _: AbsoluteOffset<Option<f32>>) {}
        fn scroll_by(&mut self, by: AbsoluteOffset, _: iced::Rectangle, _: iced::Rectangle) {
            self.0.push((by.x, by.y));
        }
    }
    let mut scroll = ScrollBy {
        area: 1,
        met: 0,
        by: loom_ui_core::Vector::new(0.0, 26.0),
    };
    let (mut first, mut second, mut third) = (Area::default(), Area::default(), Area::default());
    let bounds = iced::Rectangle::new(iced::Point::ORIGIN, iced::Size::new(100.0, 100.0));
    for area in [&mut first, &mut second, &mut third] {
        scroll.scrollable(None, bounds, bounds, iced::Vector::ZERO, area);
    }
    assert!(first.0.is_empty() && third.0.is_empty(), "the others stay");
    assert_eq!(second.0, [(0.0, 26.0)]);
}

/// A scroll to a part the window does not show scrolls nothing, and
/// asks for nothing more.
#[test]
fn a_scroll_to_a_missing_part_does_nothing() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.send(Message::OpenBrowser);
    let _ = ui.redraw();
    let before = ui.scroll_translation("module-browser-list");
    let missing = UiId::panel("browser").with("module").with("nothing-by-this-name");
    let messages = ui.run_task(scroll_into_view(&missing));
    assert!(messages.is_empty());
    assert_eq!(ui.scroll_translation("module-browser-list"), before);
}

/// Two scrolls in one turn become one task, the second after the first,
/// so the second finds its part where the first moved it. Batched, both
/// would work from the old offsets and their moves would add up.
#[test]
fn a_turns_scrolls_run_one_after_another() {
    let mut target = IcedTarget::default();
    let card = UiId::panel("matrix").with(3u32);
    loom_ui::Target::<Message>::scroll_into_view(&mut target, &card);
    loom_ui::Target::<Message>::scroll_into_view(&mut target, &card.clone().with("depth"));
    assert!(target.tasks.is_empty(), "no scroll is batched on its own");
    assert_eq!(target.take_tasks().len(), 1, "one chained task");
    assert!(target.take_tasks().is_empty(), "and it is taken once");
}
