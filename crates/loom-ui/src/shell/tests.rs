use super::*;
use std::sync::mpsc;
use std::time::Duration;

/// A waker whose messages land in the returned receiver.
fn waker() -> (Waker<u32>, mpsc::Receiver<u32>) {
    let (sender, inbox) = mpsc::channel();
    (Waker::new(sender, Arc::new(|| {})), inbox)
}

/// An open dialog with nothing set.
fn open_request() -> FileRequest {
    FileRequest {
        action: crate::effect::FileAction::Open,
        title: "Open".into(),
        filters: Vec::new(),
        folder: None,
        file_name: None,
    }
}

/// A driver with no window over `NoServices`.
fn driver() -> ShellDriver<u32, NoServices> {
    ShellDriver {
        services: NoServices,
        window: None,
        captures: Vec::new(),
        exit: false,
    }
}

/// With no services, the clipboard reads empty and each dialog is
/// dismissed, each answer through the waker.
#[test]
fn no_services_answers_empty_and_dismissed() {
    let (waker, inbox) = waker();
    let mut services = NoServices;
    Services::<u32>::clipboard_write(&mut services, "kept nowhere".into());
    services.clipboard_read(Reply::new(|text: Option<String>| u32::from(text.is_some())), &waker);
    services.file_dialog(
        open_request(),
        Reply::new(|path: Option<PathBuf>| 10 + u32::from(path.is_some())),
        &waker,
    );
    services.ask(
        Question {
            level: crate::effect::Level::Info,
            title: "Title".into(),
            text: "Text".into(),
            buttons: vec!["OK".into()],
        },
        Reply::new(|choice: Choice| 20 + u32::from(choice.is_some())),
        &waker,
    );
    assert_eq!(inbox.try_iter().collect::<Vec<_>>(), [0, 10, 20]);
}

/// A capture waits for the end of the turn, when the shell has its layers
/// to draw with; nothing is answered before.
#[test]
fn a_capture_waits_for_the_end_of_the_turn() {
    let (waker, inbox) = waker();
    let mut driver = driver();
    driver.capture(Reply::new(|_image: Result<Image, NoGpu>| 1), &waker);
    assert_eq!(driver.captures.len(), 1);
    assert!(inbox.try_recv().is_err());
}

/// Quitting sets a flag the loop acts on after the turn, since a driver
/// call has no event loop in hand.
#[test]
fn quitting_sets_a_flag() {
    let mut driver = driver();
    Driver::<u32>::exit(&mut driver);
    assert!(driver.exit);
}

/// Work runs on another thread and answers through the waker.
#[test]
fn work_runs_on_another_thread() {
    let (waker, inbox) = waker();
    let mut driver = driver();
    let here = thread::current().id();
    driver.work(
        Job::new(move |waker: &Waker<u32>| {
            waker.send(u32::from(thread::current().id() != here));
        }),
        &waker,
    );
    assert_eq!(inbox.recv_timeout(Duration::from_secs(5)), Ok(1));
}

/// The clipboard and dialogs go to the services; the title and limits go
/// nowhere while there is no window.
#[test]
fn the_rest_goes_to_the_services() {
    let (waker, inbox) = waker();
    let mut driver = driver();
    driver.window(&WindowSpec {
        title: "No window yet".into(),
        min_size: None,
    });
    driver.clipboard_write("text".into());
    driver.clipboard_read(Reply::new(|_: Option<String>| 3), &waker);
    driver.file_dialog(open_request(), Reply::new(|_: Option<PathBuf>| 4), &waker);
    driver.ask(
        Question {
            level: crate::effect::Level::Info,
            title: String::new(),
            text: String::new(),
            buttons: Vec::new(),
        },
        Reply::new(|_: Choice| 5),
        &waker,
    );
    assert_eq!(inbox.try_iter().collect::<Vec<_>>(), [3, 4, 5]);
}
