use super::*;

/// A sink that counts operations and keeps a clipboard.
#[derive(Default)]
struct Fake {
    operations: usize,
    clipboard: Option<String>,
}

impl Sink for Fake {
    fn operate(&mut self, _operation: Box<dyn Operation>) {
        self.operations += 1;
    }

    fn clipboard_read(&mut self) -> Option<String> {
        self.clipboard.clone()
    }

    fn clipboard_write(&mut self, contents: String) {
        self.clipboard = Some(contents);
    }
}

/// A task's outputs come back in order, and a finished task is not stuck.
#[test]
fn outputs_come_back_in_order() {
    let task = Task::done(1).chain(Task::done(2));
    let ran = run(task, &mut Fake::default());
    assert_eq!(ran.outputs, [1, 2]);
    assert_eq!(ran.stuck, None);
}

/// A widget operation goes to the sink, which runs it on the view.
#[test]
fn a_widget_operation_goes_to_the_sink() {
    let id = loom_ui_core::UiId::panel("inspector").with("field");
    let mut sink = Fake::default();
    let ran = run(crate::iced_bridge::scroll_into_view::<()>(&id), &mut sink);
    assert_eq!(sink.operations, 1);
    assert_eq!(ran.stuck, None);
}

/// The clipboard is read from and written to the sink.
#[test]
fn the_clipboard_is_the_sinks() {
    let mut sink = Fake::default();
    let ran = run(iced::clipboard::write::<()>("copied".into()), &mut sink);
    assert_eq!(ran.stuck, None);
    assert_eq!(sink.clipboard.as_deref(), Some("copied"));
    let ran = run(iced::clipboard::read(), &mut sink);
    assert_eq!(ran.outputs, [Some("copied".to_string())]);
}

/// A task that waits on the outside world stops, and says so, keeping
/// what it gave before.
#[test]
fn a_task_that_waits_is_stuck() {
    let task = Task::done(1).chain(Task::future(std::future::pending::<i32>()));
    let ran = run(task, &mut Fake::default());
    assert_eq!(ran.outputs, [1]);
    assert_eq!(ran.stuck, Some(Stuck::Waits));
}

/// An action no sink carries out, such as exit, stops the task and is
/// named, so a caller can refuse it out loud.
#[test]
fn an_action_no_sink_runs_is_named() {
    let ran = run(iced::exit::<()>(), &mut Fake::default());
    assert!(
        matches!(&ran.stuck, Some(Stuck::Action(action)) if action.contains("Exit")),
        "{:?}",
        ran.stuck
    );
}

/// A long chain of tasks that are ready at once finishes, however many
/// times its parts yield and wake themselves on the way.
#[test]
fn a_long_chain_of_ready_tasks_finishes() {
    let mut task = Task::done(0);
    for _ in 0..32 {
        task = task.then(|n| Task::done(n + 1));
    }
    let ran = run(task, &mut Fake::default());
    assert_eq!(ran.outputs, [32]);
    assert_eq!(ran.stuck, None);
}

/// A future that wakes itself on every poll and never settles.
struct Spin;

impl std::future::Future for Spin {
    type Output = i32;

    fn poll(self: std::pin::Pin<&mut Self>, context: &mut std::task::Context<'_>) -> std::task::Poll<i32> {
        context.waker().wake_by_ref();
        std::task::Poll::Pending
    }
}

/// A task that keeps waking itself and never goes on stops as spinning,
/// keeping what it gave, so it cannot hold a turn forever.
#[test]
fn a_task_that_spins_is_stuck() {
    let task = Task::done(1).chain(Task::future(Spin));
    let ran = run(task, &mut Fake::default());
    assert_eq!(ran.outputs, [1]);
    assert_eq!(ran.stuck, Some(Stuck::Spins));
}
