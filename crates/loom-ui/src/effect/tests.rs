use super::*;
use std::sync::{Arc, mpsc};

/// A test message.
#[derive(Debug, PartialEq, Eq)]
enum Msg {
    Picked(Option<PathBuf>),
    Pressed(Choice),
    Captured(bool),
    Pasted(Option<String>),
    Done(u32),
    Drawn,
}

/// A save request for the tests.
fn save_request() -> FileRequest {
    FileRequest {
        action: FileAction::Save,
        title: "Save patch".into(),
        filters: vec![FileFilter {
            name: "Patch".into(),
            extensions: vec!["ron".into()],
        }],
        folder: None,
        file_name: Some("untitled.ron".into()),
    }
}

/// A question for the tests.
fn question() -> Question {
    Question {
        level: Level::Warning,
        title: "Unsaved changes".into(),
        text: "Save before closing?".into(),
        buttons: vec!["Save".into(), "Don't save".into(), "Cancel".into()],
    }
}

/// A waker that queues into a channel and wakes nothing.
fn waker() -> (Waker<Msg>, mpsc::Receiver<Msg>) {
    let (queue, receiver) = mpsc::channel();
    (Waker::new(queue, Arc::new(|| {})), receiver)
}

/// Section 2, rule 2: each constructor makes a value a test can read back
/// as its variant, with what it was given.
#[test]
fn effects_are_plain_values_a_test_can_read() {
    let field = UiId::panel("browser").with("search");
    let effects = Effects::focus(field.clone())
        .and(Effects::focus_and_select_all(field.clone()))
        .and(Effects::focus_step(false))
        .and(Effects::focus_step(true))
        .and(Effects::scroll_into_view(field.clone()))
        .and(Effects::clipboard_write("osc"))
        .and(Effects::clipboard_read(Msg::Pasted))
        .and(Effects::file_dialog(save_request(), Msg::Picked))
        .and(Effects::ask(question(), Msg::Pressed))
        .and(Effects::work(|| Msg::Done(1)))
        .and(Effects::capture(|image| Msg::Captured(image.is_ok())))
        .and(Effects::after_draw(Msg::Drawn))
        .and(Effects::redraw())
        .and(Effects::quit());
    let list: Vec<_> = effects.into_iter().collect();
    assert_eq!(list.len(), 14);
    assert!(
        matches!(&list[0], Effect::Focus { target: FocusTarget::Element(id), select: Selection::Keep } if *id == field)
    );
    assert!(matches!(
        &list[1],
        Effect::Focus {
            target: FocusTarget::Element(_),
            select: Selection::All
        }
    ));
    assert!(matches!(
        &list[2],
        Effect::Focus {
            target: FocusTarget::Next,
            ..
        }
    ));
    assert!(matches!(
        &list[3],
        Effect::Focus {
            target: FocusTarget::Previous,
            ..
        }
    ));
    assert!(matches!(&list[4], Effect::ScrollIntoView(id) if *id == field));
    assert!(matches!(&list[5], Effect::ClipboardWrite(text) if text == "osc"));
    assert!(matches!(&list[6], Effect::ClipboardRead(_)));
    assert!(matches!(&list[7], Effect::FileDialog { request, .. } if *request == save_request()));
    assert!(matches!(&list[8], Effect::Ask { question: asked, .. } if *asked == question()));
    assert!(matches!(&list[9], Effect::Work(_)));
    assert!(matches!(&list[10], Effect::Capture(_)));
    assert!(matches!(&list[11], Effect::AfterDraw(Msg::Drawn)));
    assert!(matches!(&list[12], Effect::Redraw));
    assert!(matches!(&list[13], Effect::Quit));
}

/// Each effect has its stable name, which is also its panic text in a
/// driver that cannot carry it out.
#[test]
fn every_effect_has_its_stable_name() {
    let effects = Effects::focus(UiId::panel("a"))
        .and(Effects::scroll_into_view(UiId::panel("a")))
        .and(Effects::clipboard_write(""))
        .and(Effects::clipboard_read(Msg::Pasted))
        .and(Effects::file_dialog(save_request(), Msg::Picked))
        .and(Effects::ask(question(), Msg::Pressed))
        .and(Effects::work(|| Msg::Drawn))
        .and(Effects::capture(|_| Msg::Drawn))
        .and(Effects::after_draw(Msg::Drawn))
        .and(Effects::redraw())
        .and(Effects::quit());
    let found: Vec<_> = effects.iter().map(Effect::name).collect();
    assert_eq!(
        found,
        [
            names::FOCUS,
            names::SCROLL_INTO_VIEW,
            names::CLIPBOARD_WRITE,
            names::CLIPBOARD_READ,
            names::FILE_DIALOG,
            names::ASK,
            names::WORK,
            names::CAPTURE,
            names::AFTER_DRAW,
            names::REDRAW,
            names::QUIT,
        ]
    );
    assert_eq!(
        found,
        [
            "focus",
            "scroll into view",
            "clipboard write",
            "clipboard read",
            "file dialog",
            "question",
            "background work",
            "window capture",
            "after draw",
            "redraw",
            "quit"
        ],
        "the names are part of the API: tests match on them"
    );
}

/// A reply turns each kind of answer into the app's message.
#[test]
fn a_reply_makes_the_message_for_its_answer() {
    let mut effects = Effects::file_dialog(save_request(), Msg::Picked)
        .and(Effects::ask(question(), Msg::Pressed))
        .and(Effects::capture(|image| Msg::Captured(image.is_ok())))
        .and(Effects::clipboard_read(Msg::Pasted))
        .into_iter();
    let Some(Effect::FileDialog { reply, .. }) = effects.next() else {
        panic!("a file dialog comes first");
    };
    assert_eq!(reply.give(Some("a.ron".into())), Msg::Picked(Some("a.ron".into())));
    let Some(Effect::Ask { reply, .. }) = effects.next() else {
        panic!("a question comes second");
    };
    assert_eq!(reply.give(Some(2)), Msg::Pressed(Some(2)));
    let Some(Effect::Capture(reply)) = effects.next() else {
        panic!("a capture comes third");
    };
    assert_eq!(reply.give(Err(NoGpu)), Msg::Captured(false));
    let Some(Effect::ClipboardRead(reply)) = effects.next() else {
        panic!("a clipboard read comes fourth");
    };
    assert_eq!(reply.give(Some("osc".into())), Msg::Pasted(Some("osc".into())));
}

/// Section 2, rule 4's shape: work sends its result through the waker,
/// and a job may send several messages, in order.
#[test]
fn work_sends_one_message_and_a_job_sends_many_in_order() {
    let (waker, receiver) = waker();
    let effects = Effects::work(|| Msg::Done(1)).and(Effects::job(|waker| {
        for n in 2..=4 {
            waker.send(Msg::Done(n));
        }
    }));
    for effect in effects {
        let Effect::Work(job) = effect else {
            panic!("only work here");
        };
        job.run(&waker);
    }
    assert_eq!(
        receiver.try_iter().collect::<Vec<_>>(),
        [Msg::Done(1), Msg::Done(2), Msg::Done(3), Msg::Done(4)]
    );
}

/// The list keeps its order whichever way it is built, and reports its
/// size.
#[test]
fn effects_keep_their_order_however_they_are_built() {
    let none: Effects<Msg> = Effects::default();
    assert!(none.is_empty());
    assert_eq!(none.len(), 0);

    let mut built = Effects::from(Effect::Redraw);
    built.push(Effect::Quit);
    built.extend([Effect::AfterDraw(Msg::Drawn)]);
    let names: Vec<_> = (&built).into_iter().map(Effect::name).collect();
    assert_eq!(names, [names::REDRAW, names::QUIT, names::AFTER_DRAW]);
    assert_eq!(built.len(), 3);

    let collected: Effects<Msg> = [Effect::Quit, Effect::Redraw].into_iter().collect();
    let joined: Effects<Msg> = [Effects::redraw(), Effects::none(), Effects::quit()]
        .into_iter()
        .collect();
    assert_eq!(
        collected.iter().map(Effect::name).collect::<Vec<_>>(),
        [names::QUIT, names::REDRAW]
    );
    assert_eq!(
        joined.iter().map(Effect::name).collect::<Vec<_>>(),
        [names::REDRAW, names::QUIT]
    );
}

/// Debug output names each effect and shows its data, but no closure.
#[test]
fn debug_output_names_each_effect() {
    let effects = Effects::focus(UiId::panel("a"))
        .and(Effects::scroll_into_view(UiId::panel("b")))
        .and(Effects::clipboard_write("x"))
        .and(Effects::clipboard_read(Msg::Pasted))
        .and(Effects::file_dialog(save_request(), Msg::Picked))
        .and(Effects::ask(question(), Msg::Pressed))
        .and(Effects::work(|| Msg::Drawn))
        .and(Effects::capture(|_| Msg::Drawn))
        .and(Effects::after_draw(Msg::Drawn))
        .and(Effects::redraw())
        .and(Effects::quit());
    let text = format!("{effects:?}");
    for part in [
        "Focus",
        "ScrollIntoView",
        "ClipboardWrite(\"x\")",
        "ClipboardRead",
        "FileDialog",
        "Save patch",
        "Ask",
        "Unsaved changes",
        "Work",
        "Capture",
        "AfterDraw",
        "Redraw",
        "Quit",
    ] {
        assert!(text.contains(part), "{part} in {text}");
    }
    assert_eq!(format!("{:?}", Reply::<u32, u32>::new(|n| n)), "Reply");
    assert_eq!(format!("{:?}", Job::<u32>::new(|_| {})), "Job");
}

/// An image holds exactly four bytes per pixel, and gives them back.
#[test]
fn an_image_holds_four_bytes_per_pixel() {
    let image = Image::new(2, 1, 2.0, vec![1, 2, 3, 4, 5, 6, 7, 8]).expect("eight bytes for two pixels");
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(image.scale(), 2.0, "the scale it was drawn at");
    assert_eq!(image.rgba(), [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(Image::new(2, 1, 1.0, vec![0; 7]), None);
    assert_eq!(
        Image::new(0, 0, 1.0, Vec::new()).map(|image| image.rgba().len()),
        Some(0)
    );
}

/// Taking the focus from every element is a focus effect to nothing, which
/// keeps the selection as it is.
#[test]
fn unfocus_moves_the_focus_to_nothing() {
    let list: Vec<Effect<Msg>> = Effects::unfocus().into_iter().collect();
    assert!(matches!(
        list.as_slice(),
        [Effect::Focus {
            target: FocusTarget::Nothing,
            select: Selection::Keep
        }]
    ));
    assert_eq!(list[0].name(), names::FOCUS);
}
