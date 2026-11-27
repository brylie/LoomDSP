use super::*;
use crate::question::{Answer, Message as QuestionMessage};
use crate::{CanvasEvent, Message, files, tests, ui_test::Ui};
use iced::mouse;
use loom_ui_core::{Key, Modifiers, Point};

/// Every built-in example parses and loads with the running registry.
#[test]
fn bundled_help_examples_load() {
    let app = tests::app();
    for descriptor in &app.modules {
        let help = Help::new(descriptor);
        assert!(!help.model.notes.is_empty(), "{} has no notes", descriptor.id);
        let example = help.model.example.as_ref().expect(descriptor.id);
        let patch = loom_patch::Patch::from_ron(example).expect(descriptor.id);
        let session = crate::Loaded::read(
            loom_core::EngineConfig::default(),
            false,
            crate::Source::Recovered(Box::new(patch)),
            &loom_patch::NoLibrary,
        );
        assert!(
            session.is_ok(),
            "{}: {}",
            descriptor.id,
            session.err().unwrap_or_default()
        );
    }
}

/// Every bundled help example and every example patch names its patch in
/// `metadata`. `Patch` skips unknown fields, so a `name` left at the top
/// level after the format change (#652) would load as an empty name and
/// no other test would notice.
#[test]
fn bundled_and_example_patches_name_themselves_in_metadata() {
    let app = tests::app();
    for descriptor in &app.modules {
        let example = Help::new(descriptor).model.example.expect(descriptor.id);
        let patch = loom_patch::Patch::from_ron(&example).expect(descriptor.id);
        assert!(
            !patch.metadata.name.is_empty(),
            "{}: example has no name",
            descriptor.id
        );
    }
    let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for folder in ["patches", "dev"] {
        for entry in std::fs::read_dir(examples.join(folder)).expect("examples folder") {
            let path = entry.expect("entry").path();
            if path.extension().is_some_and(|e| e == "ron") {
                let text = std::fs::read_to_string(&path).expect("example");
                let patch = loom_patch::Patch::from_ron(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(!patch.metadata.name.is_empty(), "{} has no name", path.display());
            }
        }
    }
}

/// A registry module with no bundled notes has no example action.
#[test]
fn modules_without_notes_have_no_example() {
    let app = crate::App::new(crate::Args {
        test_modules: true,
        ..tests::app_args()
    });
    let descriptor = app
        .modules
        .iter()
        .find(|d| d.id.starts_with("test."))
        .expect("test module");
    let help = Help::new(descriptor);
    assert!(help.model.notes.is_empty());
    assert!(help.model.example.is_none());
}

/// F1 reaches the focused browser, and Escape returns to the same search.
#[test]
fn help_from_focused_browser_preserves_search() {
    let mut ui = Ui::open("subtractive.ron");
    let canvas = ui.canvas();
    ui.move_to(Point::new(canvas.bounds.x + 8.0, canvas.bounds.y + 8.0));
    ui.tap(Key::Character('a'), Modifiers::NONE);
    ui.typewrite("svf");
    ui.tap(Key::F1, Modifiers::NONE);
    let help = ui.app().ui_snapshot().help.expect("help is open");
    assert_eq!(help.id, "loom.filter.svf");
    assert!(!help.inputs.is_empty());
    assert!(help.notes.contains("Example patch"));
    let example = crate::overlays::access::help_id().with("body").with("example");
    assert!(ui.app().help_stops().contains(&example), "Open example is a stop");
    ui.tap(Key::Escape, Modifiers::NONE);
    assert!(ui.app().overlays.help.is_none());
    assert_eq!(
        ui.app().overlays.browser().unwrap().highlighted().unwrap().id,
        "loom.filter.svf"
    );
    ui.typewrite("x");
    assert!(
        ui.app().overlays.browser().unwrap().highlighted().is_none(),
        "search retains focus"
    );
}

/// The selected tile, inspector action, and tile menu use the same model.
#[test]
fn help_from_selected_tile_inspector_and_menu() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.tap(Key::F1, Modifiers::NONE);
    let expected = ui.app().ui_snapshot().help.unwrap();
    // Help names the module's family first (ADR-0029).
    let line = format!("{} · {} · {}", expected.family, expected.category, expected.id);
    assert!(!expected.family.is_empty() && ui.text(&line).is_some(), "{line}");
    ui.tap(Key::Escape, Modifiers::NONE);
    let at = ui.text("Help").expect("inspector help action").center();
    ui.move_to(at);
    ui.press();
    ui.release();
    assert_eq!(ui.app().ui_snapshot().help, Some(expected.clone()));
    ui.tap(Key::Escape, Modifiers::NONE);
    let bounds = ui.app().session.scene.module(4).unwrap().bounds();
    let at = ui.canvas().to_window(Point::new(bounds.center().x, bounds.y + 4.0));
    ui.move_to(at);
    ui.step(&[iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))]);
    ui.step(&[iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right))]);
    assert!(ui.app().overlays.tile_menu().is_some());
    let (_, menu_at) = ui.app().overlays.tile_menu().unwrap();
    ui.move_to(Point::new(menu_at.x + 30.0, menu_at.y + 15.0));
    ui.press();
    ui.release();
    assert_eq!(ui.app().ui_snapshot().help, Some(expected));
}

/// Scrolling the notes keeps the Close button visible and clickable.
#[test]
fn help_close_stays_visible_while_scrolling() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.tap(Key::F1, Modifiers::NONE);
    let before = ui.text("Close").expect("close action");
    let body = body_id(&ui);
    let scroll_before = ui.runtime().scroll_offset(&body).expect("help scroll area");
    let _ = ui.redraw();
    let middle = ui.runtime().bounds(&body).expect("the body is laid out").center();
    ui.move_to(middle);
    ui.step(&[iced::Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Lines { x: 0.0, y: -12.0 },
    })]);
    let after = ui.text("Close").expect("close action remains visible");
    assert_eq!(before, after);
    let _ = ui.redraw();
    let scroll_after = ui.runtime().scroll_offset(&body).expect("help scroll area");
    assert_ne!(scroll_before, scroll_after, "the content scrolled");
    ui.move_to(after.center());
    ui.press();
    ui.release();
    assert!(ui.app().overlays.help.is_none());
}

/// The example button asks about unsaved changes and honors the answer.
#[test]
fn example_button_honors_unsaved_changes() {
    let mut ui = Ui::open("sine.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![3])));
    let _ = ui.app_mut().send(Message::Inspector(crate::inspector::Message::Param {
        module: 5,
        index: 0,
        position: 0.8,
    }));
    let _ = ui.app_mut().send(Message::SliderReleased);
    assert!(ui.app().unsaved());
    ui.tap(Key::F1, Modifiers::NONE);
    let _ = files::fake::asked();
    let at = tab_to_example(&mut ui);
    ui.move_to(at);
    ui.press();
    ui.release();
    assert!(ui.app().overlays.question.is_some(), "it asks first");
    let _ = ui
        .app_mut()
        .send(Message::Question(QuestionMessage::Answer(Answer::Cancel)));
    assert!(ui.app().overlays.help.is_some(), "cancellation keeps the help open");
    assert!(ui.app().unsaved());

    // The answer came as a message, not through the window: a frame lays
    // the window out without the question's panel, as one would.
    let _ = ui.redraw();
    ui.press();
    ui.release();
    assert!(ui.app().overlays.question.is_some(), "it asks again");
    let _ = ui
        .app_mut()
        .send(Message::Question(QuestionMessage::Answer(Answer::DontSave)));
    assert!(ui.app().overlays.help.is_none());
    assert!(!ui.app().unsaved());
    assert!(ui.app().session.document.file().is_none());
    assert_eq!(
        ui.app()
            .session
            .controller
            .graph()
            .nodes()
            .filter(|(_, node)| node.descriptor.id == "loom.osc.sine")
            .count(),
        1
    );
}

/// Saving from the unsaved-changes dialog writes the old patch first.
#[test]
fn example_button_saves_the_old_patch_when_requested() {
    let dir = crate::recovery::tests::TempDir::new("help-example-save");
    let destination = dir.0.join("saved.ron");
    let mut ui = Ui::open("sine.ron");
    // The test patch is an untitled copy, so saving asks for a path.
    ui.app_mut().session.document.forget_file();
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![3])));
    let _ = ui.app_mut().send(Message::Inspector(crate::inspector::Message::Param {
        module: 5,
        index: 0,
        position: 0.8,
    }));
    let _ = ui.app_mut().send(Message::SliderReleased);
    let value = ui
        .app()
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(5))
        .unwrap()
        .params[0];
    ui.tap(Key::F1, Modifiers::NONE);
    let _ = files::fake::asked();
    files::fake::answer_save(Some(destination.clone()));
    let at = tab_to_example(&mut ui);
    ui.move_to(at);
    ui.press();
    ui.release();
    let _ = ui
        .app_mut()
        .send(Message::Question(QuestionMessage::Answer(Answer::Save)));
    let saved = crate::patch_file::load(&destination).expect("old patch was saved");
    let gain = saved.modules.iter().find(|module| module.id == 5).unwrap().params["gain"];
    assert_eq!(gain, value);
    assert!(
        ui.app().session.document.file().is_none(),
        "the example is an untitled copy"
    );
    assert!(!ui.app().unsaved());
    assert!(matches!(files::fake::asked().as_slice(), [files::fake::Asked::Save(_)]));
    files::fake::answer_save(None);
}

/// Opening a bundled example respects cancellation of unsaved changes.
#[test]
fn example_uses_existing_unsaved_changes_flow() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    let _ = ui.app_mut().send(Message::ShowHelp);
    let _ = ui.app_mut().send(Message::Inspector(crate::inspector::Message::Param {
        module: 4,
        index: 0,
        position: 0.8,
    }));
    let _ = ui.app_mut().send(Message::SliderReleased);
    assert!(ui.app().unsaved());
    let _ = files::fake::asked();
    let _ = ui.app_mut().send(Message::HelpExample);
    assert!(ui.app().overlays.help.is_some());
    assert!(ui.app().overlays.question.is_some());
    let mut clean = Ui::open("subtractive.ron");
    let _ = clean.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    let _ = clean.app_mut().send(Message::ShowHelp);
    let _ = clean.app_mut().send(Message::HelpExample);
    assert!(clean.app().overlays.help.is_none());
    assert!(clean.app().session.document.file().is_none());
}

/// Clicks the middle of a window rectangle.
fn click(ui: &mut Ui, at: Point) {
    ui.move_to(at);
    ui.press();
    ui.release();
}

/// With nothing selected the inspector is short, and its button and F1
/// open the shortcuts page (#457).
#[test]
fn empty_inspector_and_f1_open_the_shortcuts() {
    let mut ui = Ui::open("subtractive.ron");
    assert!(ui.text("Select a module to edit it.").is_some());
    assert!(
        ui.text(crate::HELP.lines().next().unwrap()).is_none(),
        "no shortcut list in the panel"
    );
    let button = ui.text("Keyboard shortcuts").expect("the shortcuts button").center();
    click(&mut ui, button);
    assert_eq!(
        ui.app().overlays.help.as_ref().map(|h| h.model.name),
        Some("Keyboard shortcuts")
    );
    ui.tap(Key::Escape, Modifiers::NONE);
    assert!(ui.app().overlays.help.is_none());
    ui.tap(Key::F1, Modifiers::NONE);
    assert_eq!(
        ui.app().overlays.help.as_ref().map(|h| h.model.name),
        Some("Keyboard shortcuts")
    );
}

/// Help details read as words with units: no "unconnected:", no stray
/// spaces, and lower-case signal kinds (#457).
#[test]
fn help_details_read_as_words_with_units() {
    let svf = loom_modules::registry().resolve("loom.filter.svf").unwrap();
    let help = Help::new(svf);
    let m = &help.model;
    assert_eq!(m.params[0].details, "20 to 20000 Hz, logarithmic. Default 1000 Hz.");
    assert_eq!(m.inputs[0].details, "audio; reads 0 with no cable");
    assert_eq!(m.outputs[0].details, "audio, bipolar");
    for entry in m.params.iter().chain(&m.inputs).chain(&m.outputs) {
        assert!(!entry.details.contains("unconnected"), "{}", entry.details);
        assert!(!entry.details.contains(" ,"), "{}", entry.details);
    }
}

/// Help text wraps inside the overlay, and a click on the dimmed
/// backdrop closes help while a click inside it does not (#457).
#[test]
fn help_wraps_inside_and_closes_from_the_backdrop() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.tap(Key::F1, Modifiers::NONE);
    let close = ui.text("Close").expect("the Close button");
    // The panel ends 20 px right of its Close button.
    let right = close.x + close.width + 20.0;
    let help = ui.app().overlays.help.as_ref().unwrap().model.clone();
    let body = crate::overlays::access::help_id().with("body");
    let _ = ui.redraw();
    for (key, entries) in [
        ("parameters", &help.params),
        ("inputs", &help.inputs),
        ("outputs", &help.outputs),
    ] {
        for entry in entries {
            // Where the entry is laid out, scrolled into view or not.
            let id = body.clone().with(key).with(entry.name);
            let text = ui.runtime().bounds(&id).expect("an entry's place");
            assert!(
                text.x + text.width <= right,
                "{} runs past the panel",
                entry.description
            );
        }
    }
    // An entry in help's own body, by its UI ID, since the inspector
    // under help is the runtime's too and shows the module's name and
    // parameters (plan 3.2, row 4d).
    let entry = body.clone().with("parameters").with(help.params[0].name);
    let inside = ui.runtime().bounds(&entry).expect("an entry's place");
    click(&mut ui, inside.center());
    assert!(ui.app().overlays.help.is_some(), "a click inside keeps help open");
    click(&mut ui, Point::new(10.0, crate::WINDOW.height / 2.0));
    assert!(ui.app().overlays.help.is_none(), "a click on the backdrop closes help");
}

/// The inspector lists each port on one line with its signal and range,
/// and keeps its description for hover text (#457).
#[test]
fn inspector_ports_are_one_line_each() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    let crate::inspector::Inspector::Module(rows) = ui.app().inspector_model() else {
        panic!("the SVF's rows");
    };
    let input = &rows.ports[0];
    assert_eq!((input.label, input.details.as_str()), ("In", "audio, -1 to 1"));
    assert!(input.description.starts_with("The audio to filter."));
    assert!(ui.text("audio, -1 to 1").is_some());
    assert!(ui.text(&rows.kind).is_none(), "no raw type line");
}

/// Browser rows show no ID; the highlighted module's ID shows under
/// its description (#457).
#[test]
fn the_browser_shows_the_id_under_the_description() {
    let mut ui = Ui::open("subtractive.ron");
    // As a user opens it: A over the canvas.
    let canvas = ui.canvas();
    ui.move_to(Point::new(canvas.bounds.x + 8.0, canvas.bounds.y + 8.0));
    ui.tap(Key::Character('a'), Modifiers::NONE);
    // The built-ins come first; the first module follows them.
    ui.send(Message::Browser(crate::browser::Message::Hover(
        crate::builtin::ALL.len(),
    )));
    ui.redraw();
    let first = ui.app().overlays.browser().unwrap().highlighted().unwrap();
    assert!(ui.text(first.id).is_some(), "the highlighted ID");
    assert!(ui.text(first.description).is_some());
    let second = ui.app().overlays.browser().unwrap().results()[1];
    assert!(ui.text(second.id).is_none(), "no ID on other rows");
    // Each row shows its family (ADR-0029).
    assert!(ui.text(first.family.label()).is_some(), "the row's family");
}

/// The UI ID of the open help page's scrolling body.
fn body_id(ui: &Ui) -> loom_ui_core::UiId {
    let _ = ui;
    crate::overlays::access::help_id().with("body")
}

/// Tabs through help until Open example has the focus, which brings it
/// into view, and gives its centre.
fn tab_to_example(ui: &mut Ui) -> Point {
    let example = crate::overlays::access::help_id().with("body").with("example");
    for _ in 0..64 {
        if ui.runtime().focused() == Some(&example) {
            let _ = ui.redraw();
            return ui.text("Open example").expect("the button shows").center();
        }
        ui.tap(Key::Tab, Modifiers::NONE);
    }
    panic!(
        "Tab never reached Open example: the focus is on {:?}",
        ui.runtime().focused()
    );
}

/// Help opened with F1, a key, takes the focus to its Close button with a
/// ring; opened by the pointer, without one (help-pages.md, Keys, links,
/// and focus; the pointer case is `iced_bridge::pointers` tests').
#[test]
fn help_opened_from_the_keyboard_shows_the_ring_on_close() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.tap(Key::F1, Modifiers::NONE);
    let _ = ui.redraw();
    let close = crate::overlays::access::help_id().with("close");
    assert_eq!(ui.runtime().focused(), Some(&close));
    assert_eq!(ui.runtime().focus_ring().map(|(id, _)| id), Some(close));
}

/// Down and Page Down from Close scroll the body, which takes its keys
/// from Close (WCAG 2.2, 2.1.1), and Close stays where it is.
#[test]
fn the_scroll_keys_move_the_body_from_close() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.tap(Key::F1, Modifiers::NONE);
    let _ = ui.redraw();
    let body = body_id(&ui);
    let offset = |ui: &mut Ui| {
        let _ = ui.redraw();
        ui.runtime().scroll_offset(&body).expect("the body scrolls").0
    };
    let start = offset(&mut ui);
    ui.tap(Key::Down, Modifiers::NONE);
    let down = offset(&mut ui);
    assert!(down > start, "Down scrolls: {start} then {down}");
    ui.tap(Key::PageDown, Modifiers::NONE);
    let page = offset(&mut ui);
    assert!(page > down, "Page Down scrolls further: {down} then {page}");
    assert_eq!(
        ui.runtime().focused(),
        Some(&crate::overlays::access::help_id().with("close"))
    );
}

/// Tab moves through help's stops in reading order, Close, the links, and
/// Open example, and wraps back to Close; Shift+Tab goes the other way
/// (the WAI-ARIA modal dialog pattern).
#[test]
fn tab_cycles_helps_stops_in_order() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.tap(Key::F1, Modifiers::NONE);
    let _ = ui.redraw();
    let stops = ui.app().help_stops();
    assert!(stops.len() > 2, "Close, a link or Open example: {stops:?}");
    let mut met = vec![ui.runtime().focused().cloned().expect("Close has the focus")];
    for _ in 1..stops.len() {
        ui.tap(Key::Tab, Modifiers::NONE);
        let _ = ui.redraw();
        met.push(ui.runtime().focused().cloned().expect("a stop has the focus"));
    }
    assert_eq!(met, stops, "in reading order");
    ui.tap(Key::Tab, Modifiers::NONE);
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), stops.first(), "it wraps to Close");
    ui.tap(Key::Tab, Modifiers::SHIFT);
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), stops.last(), "Shift+Tab wraps back");
}

/// Escape closes help and gives the focus back to the runtime's stop that
/// had it, as section 6, rule 4 says: here, a toolbar button.
#[test]
fn escape_gives_the_focus_back() {
    let mut ui = Ui::open("subtractive.ron");
    ui.tap(Key::Tab, Modifiers::SHIFT);
    let _ = ui.redraw();
    let before = ui
        .runtime()
        .focused()
        .cloned()
        .expect("a stop of the bar has the focus");
    ui.tap(Key::F1, Modifiers::NONE);
    let _ = ui.redraw();
    assert_ne!(ui.runtime().focused(), Some(&before), "help took the focus");
    ui.tap(Key::Escape, Modifiers::NONE);
    let _ = ui.redraw();
    assert!(ui.app().overlays.help.is_none());
    assert_eq!(ui.runtime().focused(), Some(&before), "the focus came back");
}

/// A link to a page help cannot open, such as an explainer, is plain text:
/// no stop and no link node; a link to a bundled page is both
/// (help-pages.md, Keys, links, and focus; Brylie's choice).
#[test]
fn a_link_help_cannot_follow_is_plain_text() {
    let mut ui = Ui::open("subtractive.ron");
    let meter = ui
        .app()
        .modules
        .iter()
        .copied()
        .find(|d| d.id == "loom.display.meter")
        .expect("the meter is registered");
    let help = Help::new(meter);
    let targets: Vec<String> = help
        .model
        .lead
        .iter()
        .chain(&help.model.tail)
        .flat_map(|block| block.links().iter().map(|link| link.target.clone()))
        .collect();
    assert!(
        targets.iter().any(|t| t.contains("../learn/")),
        "the page links an explainer: {targets:?}"
    );
    ui.app_mut().overlays.help = Some(help);
    let _ = ui.redraw();
    assert!(!ui.app().bundled("../learn/output-safety.md"));
    let tree = ui.a11y_tree();
    let links: Vec<String> = tree
        .names()
        .into_iter()
        .filter(|name| name == "Output safety")
        .collect();
    let link_nodes = links
        .iter()
        .filter(|name| tree.node(name).role() == accesskit::Role::Link)
        .count();
    assert_eq!(link_nodes, 0, "no link node for an explainer");
}

/// Enter on a focused link follows it to its page, which opens at its top
/// with the focus on Close again, ringed; Space on it follows nothing and
/// scrolls the body a page, as a page's Space does (the WAI-ARIA link
/// pattern).
#[test]
fn enter_follows_a_focused_link_and_the_page_opens_at_its_top() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.app_mut().send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.tap(Key::F1, Modifiers::NONE);
    let _ = ui.redraw();
    let first = ui.app().overlays.help.as_ref().unwrap().model.id;
    let stops = ui.app().help_stops();
    let link = stops
        .iter()
        .find(|stop| {
            stop.keys()
                .last()
                .is_some_and(|key| key.to_string().starts_with("link-"))
        })
        .cloned()
        .expect("the page has a bundled link");
    for _ in 0..stops.len() {
        if ui.runtime().focused() == Some(&link) {
            break;
        }
        ui.tap(Key::Tab, Modifiers::NONE);
        let _ = ui.redraw();
    }
    assert_eq!(ui.runtime().focused(), Some(&link));
    let body = body_id(&ui);
    let (before, most) = ui.runtime().scroll_offset(&body).expect("the body scrolls");
    ui.tap(Key::Space, Modifiers::NONE);
    let _ = ui.redraw();
    assert_eq!(
        ui.app().overlays.help.as_ref().unwrap().model.id,
        first,
        "Space follows nothing"
    );
    let (after, _) = ui.runtime().scroll_offset(&body).expect("the body scrolls");
    assert!(
        after > before || before == most,
        "Space scrolls the body a page, as a page's does"
    );
    assert_eq!(ui.runtime().focused(), Some(&link), "and the link keeps the focus");
    ui.tap(Key::Enter, Modifiers::NONE);
    let _ = ui.redraw();
    let _ = ui.redraw();
    assert_ne!(
        ui.app().overlays.help.as_ref().unwrap().model.id,
        first,
        "Enter followed it"
    );
    let (offset, _) = ui.runtime().scroll_offset(&body_id(&ui)).expect("the body scrolls");
    assert_eq!(offset, 0.0, "the new page shows from its top");
    let close = crate::overlays::access::help_id().with("close");
    assert_eq!(ui.runtime().focused(), Some(&close), "the focus is on Close again");
    assert_eq!(
        ui.runtime().focus_ring().map(|(id, _)| id),
        Some(close),
        "with the ring: Enter is a key"
    );
}

/// F1 over a browser whose search field has the keys takes the focus to
/// help's Close, with the ring, since F1 is a key; the next turn keeps it
/// there; and Escape gives the keys back to the browser's field (#883,
/// row 1; help-pages.md, Keys, links, and focus).
#[test]
fn f1_over_a_browser_focuses_close_and_escape_gives_the_field_back() {
    use crate::input_owner::InputOwner;
    for (owner, field) in [
        (InputOwner::Browser, crate::browser::search_id()),
        (InputOwner::Patches, crate::patches::text_fields()[0].clone()),
    ] {
        let mut ui = Ui::open("subtractive.ron");
        crate::input_owner::tests::open_overlay(&mut ui, owner);
        let at = ui.field(&field);
        ui.move_to(at);
        ui.press();
        ui.release();
        assert_eq!(
            ui.app().focus.as_ref(),
            Some(&field),
            "{owner:?}: the field has the keys"
        );
        ui.tap(Key::F1, Modifiers::NONE);
        let _ = ui.redraw();
        let close = crate::overlays::access::help_id().with("close");
        assert_eq!(ui.runtime().focused(), Some(&close), "{owner:?}: Close has the focus");
        assert_eq!(
            ui.runtime().focus_ring().map(|(id, _)| id),
            Some(close.clone()),
            "{owner:?}: ringed"
        );
        let _ = ui.redraw();
        assert_eq!(
            ui.runtime().focused(),
            Some(&close),
            "{owner:?}: the next turn keeps it"
        );
        ui.tap(Key::Escape, Modifiers::NONE);
        let _ = ui.redraw();
        assert!(ui.app().overlays.help.is_none(), "{owner:?}: help closed");
        assert_eq!(
            ui.focused_field(),
            Some(field),
            "{owner:?}: the field has the keys again"
        );
    }
}
