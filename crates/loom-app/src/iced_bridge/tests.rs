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

/// A capture becomes one Iced task, which asks the window for its latest
/// frame; nothing is answered before Iced runs it.
#[test]
fn a_capture_becomes_an_iced_task() {
    let (sender, inbox) = std::sync::mpsc::channel();
    let waker = Waker::new(sender, std::sync::Arc::new(|| {}));
    let mut driver = IcedDriver::default();
    let reply = loom_ui::effect::Reply::new(|_image: Result<Image, NoGpu>| Message::BenchDone);
    loom_ui::Driver::<Message>::capture(&mut driver, reply, &waker);
    assert_eq!(driver.tasks.len(), 1);
    assert!(inbox.try_recv().is_err(), "the answer waits for Iced");
}

/// The scroll is one action: walking the window finds the part in the
/// second of two scrolling areas, and `finish` chains the scroll of that
/// area alone, by the least move that shows the part. A part already in
/// view chains nothing.
#[test]
fn a_reveal_finds_its_part_and_chains_one_scroll() {
    use iced::advanced::widget::Operation;
    use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, RelativeOffset};
    use iced::advanced::widget::operation::{Outcome, Scrollable};
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
    let rect = |y: f32, height: f32| iced::Rectangle::new(iced::Point::new(0.0, y), iced::Size::new(100.0, height));
    let row = UiId::panel("browser").with("module").with("loom.osc.sine");
    // Two lists, each 100 tall; the row is 40 below the second one's view.
    let walk = |operation: &mut dyn Operation<()>, row_y: f32, areas: &mut [Area; 2]| {
        let [first, second] = areas;
        operation.scrollable(None, rect(0.0, 100.0), rect(0.0, 400.0), iced::Vector::ZERO, first);
        operation.traverse(&mut |_| {});
        operation.scrollable(None, rect(200.0, 100.0), rect(200.0, 400.0), iced::Vector::ZERO, second);
        operation.traverse(&mut |inside| {
            inside.container(Some(&crate::places::widget_id(&row)), rect(row_y, 20.0));
            inside.traverse(&mut |_| {});
        });
    };
    let mut areas = [Area::default(), Area::default()];
    let mut reveal = Reveal {
        target: row.clone(),
        parts: crate::places::Collect::default(),
    };
    walk(&mut reveal, 320.0, &mut areas);
    let Outcome::Chain(mut scroll) = reveal.finish() else {
        panic!("a part out of view chains a scroll");
    };
    walk(scroll.as_mut(), 320.0, &mut areas);
    assert!(areas[0].0.is_empty(), "the other list stays");
    assert_eq!(areas[1].0, [(0.0, 40.0)], "its bottom meets the view's");

    let mut reveal = Reveal {
        target: row.clone(),
        parts: crate::places::Collect::default(),
    };
    walk(&mut reveal, 230.0, &mut [Area::default(), Area::default()]);
    assert!(matches!(reveal.finish(), Outcome::None), "in view: nothing to chain");
}

/// A message already queued when a focus answer comes runs first, under the
/// focus it was sent under, and the app's copy never drifts from the
/// target's: the check at the top of `update` would panic, in a test or a
/// debug build of the app, if it did.
#[test]
fn a_queued_message_runs_before_a_focus_answer() {
    let (mut bridge, _) = Bridge::start(app(), Task::none());
    let waker = bridge.runtime.waker();
    std::thread::spawn(move || waker.send(Message::ShowHelp))
        .join()
        .unwrap();
    let field = crate::browser::search_id();
    let answer = FocusAnswer {
        any: true,
        widget: Some(crate::places::field_widget_id(&field)),
    };
    let _ = bridge.update(Bridged::Focus(answer));
    assert!(bridge.app().overlays.help.is_some(), "the queued message ran");
    assert_eq!(bridge.app().focus, Some(field.clone()), "the app heard of the focus");
    assert_eq!(bridge.runtime.target().focused, Some(field));
}
