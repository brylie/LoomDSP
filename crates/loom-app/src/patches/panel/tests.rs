//! Tests for Your patches as the runtime's panels (plan 3.2, row 5b;
//! patch-metadata.md, Your patches browser).

use super::super::{Message, PatchBrowser, Row, licence_id, part_id, patch_files, patches_id, tag_id};
use super::tags_id;
use crate::recovery::tests::TempDir;
use crate::ui_test::Ui;
use loom_ui_core::{Key, Modifiers, Point, UiId};
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// A library folder with a patch in a subfolder, one at the top, and a
/// file that does not read.
fn library(name: &str) -> TempDir {
    let dir = TempDir::new(&format!("patches-panel-{name}"));
    std::fs::create_dir_all(dir.join("live")).unwrap();
    let write = |file: &str, text: &str| std::fs::write(dir.join(file), text).unwrap();
    write(
        "live/bass.ron",
        r#"Patch(version: 1, metadata: Metadata(name: "Bass", authors: ["Sam Rivera"], tags: ["drone", "808"], license: "MIT"))"#,
    );
    write(
        "pad.ron",
        r#"Patch(version: 1, metadata: Metadata(name: "Pad", tags: ["ambient"]))"#,
    );
    write("broken.ron", "not a patch");
    dir
}

/// A browser that has read every file in `dir`, one file per batch.
fn browser(dir: &Path) -> PatchBrowser {
    let mut browser = PatchBrowser::new(1, dir.to_path_buf());
    for path in patch_files(dir, &AtomicBool::new(false)) {
        let _ = browser.update(Message::Found {
            open: 1,
            rows: vec![Row::read(&path)],
        });
    }
    browser
}

/// The app with Your patches open over `dir`, read, with the focus in the
/// search field, as opening it gives.
fn open(dir: &Path) -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut()
        .overlays
        .open(crate::overlays::Overlay::Patches(browser(dir)));
    let _ = ui.redraw();
    click_on(&mut ui, &part_id("search"));
    assert_eq!(ui.runtime().focused(), Some(&part_id("search")));
    ui
}

/// A left click on the middle of `id`.
fn click_on(ui: &mut Ui, id: &UiId) {
    ui.lay_out_panels();
    let at = ui.runtime().bounds(id).unwrap_or_else(|| panic!("{id} is laid out"));
    ui.move_to(Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0));
    ui.press();
    ui.release();
}

/// Presses `key` with no modifier.
fn tap(ui: &mut Ui, key: Key) {
    let _ = ui.tap(key, Modifiers::NONE);
}

/// The browser's model.
fn model(ui: &Ui) -> &super::super::Model {
    ui.app().overlays.patches().expect("Your patches is open").model()
}

/// A screen reader meets a dialog named "Your patches" with Close, the
/// two fields, a check box per licence filter keyed by its fixed key, the
/// tags as a toolbar of toggle buttons, and the list box of rows, keyed by
/// path in the library folder inside the list.
#[test]
fn a_screen_reader_finds_the_dialog_and_its_parts() {
    let dir = library("parts");
    let mut ui = open(&dir);
    let tree = ui.a11y_tree();
    let dialog = tree.node_by_id(&patches_id());
    assert_eq!(dialog.role(), accesskit::Role::Dialog);
    assert_eq!(dialog.name(), Some("Your patches"));
    assert_eq!(tree.node_by_id(&part_id("close")).role(), accesskit::Role::Button);
    let search = tree.node_by_id(&part_id("search"));
    assert_eq!(search.role(), accesskit::Role::TextInput);
    assert!(search.is_focused());
    assert_eq!(tree.node_by_id(&part_id("author")).role(), accesskit::Role::TextInput);
    for key in ["lets-me-change", "allows-commercial-use", "no-licence-stated"] {
        let check = tree.node_by_id(&licence_id(key));
        assert_eq!(check.role(), accesskit::Role::CheckBox);
        assert_eq!(check.toggled(), Some(accesskit::Toggled::False));
    }
    let tags = tree.node_by_id(&tags_id());
    assert_eq!(tags.role(), accesskit::Role::Toolbar);
    assert_eq!(tags.name(), Some("Tags"));
    // A tag of digits alone stays a name, not a number.
    let tag = tree.node_by_id(&tag_id("808"));
    assert_eq!(tag.role(), accesskit::Role::Button);
    assert_eq!(tag.toggled(), Some(accesskit::Toggled::False));
    let bass = part_id("list").with("row").with("live/bass.ron");
    assert_eq!(bass.to_string(), "patches/list/row/live%2Fbass.ron");
    let option = tree.node_by_id(&bass);
    assert_eq!(option.role(), accesskit::Role::ListBoxOption);
    assert!(option.description().is_some_and(|text| text.contains("Sam Rivera")));
}

/// A file that does not read says so in its name, stays enabled so a
/// screen reader does not skip it, offers no Click, and opens nothing when
/// the pointer clicks it.
#[test]
fn an_unread_row_says_so_and_opens_nothing() {
    let dir = library("unread");
    let mut ui = open(&dir);
    let broken = part_id("list").with("row").with("broken.ron");
    let tree = ui.a11y_tree();
    let option = tree.node_by_id(&broken);
    assert_eq!(option.name(), Some("broken.ron, could not read"));
    assert!(!option.is_disabled());
    assert!(!option.offers(accesskit::Action::Click), "no dead Click");
    let pad = tree.node_by_id(&part_id("list").with("row").with("pad.ron"));
    assert!(pad.offers(accesskit::Action::Click), "a row that reads offers Click");
    click_on(&mut ui, &broken);
    assert!(ui.app().overlays.patches().is_some(), "nothing opened");
}

/// Tab loops through Close, the search field, the Author field, each
/// licence check box, then the tags as one stop, and back to Close;
/// Shift+Tab goes the other way (the WAI-ARIA modal dialog pattern).
#[test]
fn tab_loops_through_the_dialog_with_the_tags_as_one_stop() {
    let dir = library("tab");
    let mut ui = open(&dir);
    let order = [
        part_id("author"),
        licence_id("lets-me-change"),
        licence_id("allows-commercial-use"),
        licence_id("no-licence-stated"),
        tag_id("808"),
        part_id("close"),
        part_id("search"),
    ];
    for want in &order {
        tap(&mut ui, Key::Tab);
        assert_eq!(ui.runtime().focused(), Some(want));
    }
    let _ = ui.tap(Key::Tab, Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), Some(&part_id("close")));
}

/// On the tags, Left, Right, Home, and End move among them and stop at the
/// ends; Space picks the focused tag; and Tab back into the tags lands on
/// the tag last used (the WAI-ARIA toolbar pattern).
#[test]
fn the_tags_are_a_toolbar() {
    let dir = library("tags");
    let mut ui = open(&dir);
    let tags: Vec<UiId> = model(&ui).tags.iter().map(|tag| tag_id(&tag.text)).collect();
    assert_eq!(tags.len(), 3, "808, ambient, drone");
    let _ = ui.tap(Key::Tab, Modifiers::SHIFT);
    let _ = ui.tap(Key::Tab, Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), Some(&tags[0]), "the first tag");
    tap(&mut ui, Key::Left);
    assert_eq!(ui.runtime().focused(), Some(&tags[0]), "it stops at the start");
    tap(&mut ui, Key::Right);
    assert_eq!(ui.runtime().focused(), Some(&tags[1]));
    tap(&mut ui, Key::End);
    assert_eq!(ui.runtime().focused(), Some(&tags[2]));
    tap(&mut ui, Key::Home);
    tap(&mut ui, Key::Right);
    tap(&mut ui, Key::Space);
    assert!(model(&ui).tags[1].picked, "Space picked the focused tag");
    tap(&mut ui, Key::Tab);
    assert_eq!(ui.runtime().focused(), Some(&part_id("close")));
    let _ = ui.tap(Key::Tab, Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), Some(&tags[1]), "back on the tag last used");
}

/// Up and Down move the highlight from either field, and the highlighted
/// row is each field's active descendant; Escape closes from a field and
/// from a tag.
#[test]
fn both_fields_hold_the_list_and_escape_closes_from_anywhere() {
    let dir = library("keys");
    let mut ui = open(&dir);
    tap(&mut ui, Key::Tab);
    assert_eq!(ui.runtime().focused(), Some(&part_id("author")));
    // The highlight is on the patch read first, which newer rows sorted
    // below; Up moves it one row toward the top.
    let start = model(&ui).highlight.expect("a highlight");
    tap(&mut ui, Key::Up);
    let now = model(&ui).highlight.expect("a highlight");
    assert_eq!(now, start.saturating_sub(1));
    let row = model(&ui).rows[now].id.clone();
    let tree = ui.a11y_tree();
    assert_eq!(tree.node_by_id(&part_id("author")).active_descendant(), Some(&row));
    assert_eq!(tree.node_by_id(&part_id("search")).active_descendant(), Some(&row));
    tap(&mut ui, Key::Escape);
    assert!(ui.app().overlays.patches().is_none(), "Escape closed it from a field");

    let mut ui = open(&dir);
    let _ = ui.tap(Key::Tab, Modifiers::SHIFT);
    let _ = ui.tap(Key::Tab, Modifiers::SHIFT);
    assert!(ui.runtime().focused().is_some_and(|id| id.is_under(&tags_id())));
    tap(&mut ui, Key::Escape);
    assert!(ui.app().overlays.patches().is_none(), "Escape closed it from a tag");
}

/// Typing in a field filters, and the pointer over a row highlights it.
#[test]
fn typing_filters_and_hover_highlights() {
    let dir = library("type");
    let mut ui = open(&dir);
    ui.typewrite("bass");
    assert_eq!(model(&ui).search, "bass");
    assert_eq!(model(&ui).rows.len(), 1);
    let mut ui = open(&dir);
    let second = model(&ui).rows[1].id.clone();
    ui.lay_out_panels();
    let at = ui.runtime().bounds(&second).expect("the row");
    ui.move_to(Point::new(at.x + 10.0, at.y + 10.0));
    assert_eq!(model(&ui).highlight, Some(1));
}

/// Rows that arrive in batches while the user looks move neither the
/// focus nor the highlighted patch.
#[test]
fn a_batch_of_rows_moves_neither_the_focus_nor_the_highlight() {
    let dir = library("batch");
    let mut ui = open(&dir);
    tap(&mut ui, Key::Up);
    let highlighted = {
        let model = model(&ui);
        model.rows[model.highlight.expect("a highlight")].id.clone()
    };
    std::fs::write(
        dir.join("zzz.ron"),
        r#"Patch(version: 1, metadata: Metadata(name: "Zed"))"#,
    )
    .unwrap();
    ui.send(crate::Message::Patches(Message::Found {
        open: 1,
        rows: vec![Row::read(&dir.join("zzz.ron"))],
    }));
    let _ = ui.redraw();
    assert_eq!(ui.runtime().focused(), Some(&part_id("search")));
    let model = model(&ui);
    assert_eq!(model.highlight.map(|at| &model.rows[at].id), Some(&highlighted));
}

/// In a window narrower than the box, the box shrinks to fit and nothing
/// is cut: every part is inside the window, and the list still scrolls.
#[test]
fn in_a_narrow_window_nothing_is_cut() {
    let dir = library("narrow");
    let mut ui = open(&dir);
    ui.resize(480.0, 700.0);
    let _ = ui.redraw();
    ui.lay_out_panels();
    for id in [part_id("close"), part_id("search"), part_id("author"), part_id("list")] {
        let at = ui.runtime().bounds(&id).unwrap_or_else(|| panic!("{id}"));
        assert!(at.x >= 0.0 && at.x + at.width <= 480.0, "{id} at {at:?}");
    }
    let list = ui.runtime().bounds(&part_id("list")).unwrap();
    assert!(list.height > 0.0);
}

/// As rows stream in, a row keeps its UI ID, every file read is counted,
/// and the highlight stays on the same patch.
#[test]
fn streamed_rows_keep_their_ids_and_the_list_grows() {
    let dir = library("stream");
    let paths = patch_files(&dir, &AtomicBool::new(false));
    let mut browser = PatchBrowser::new(1, dir.to_path_buf());
    let mut seen: Vec<UiId> = Vec::new();
    for path in &paths {
        let highlighted = browser.model().highlight.map(|at| browser.model().rows[at].id.clone());
        let _ = browser.update(Message::Found {
            open: 1,
            rows: vec![Row::read(path)],
        });
        let model = browser.model();
        let ids: Vec<UiId> = model.rows.iter().map(|row| row.id.clone()).collect();
        assert!(seen.iter().all(|id| ids.contains(id)), "rows keep their IDs");
        assert_eq!(model.loaded, model.rows.len());
        if let Some(before) = highlighted {
            let now = model.highlight.map(|at| &model.rows[at].id);
            assert_eq!(now, Some(&before), "the highlight stays on its patch");
        }
        seen = ids;
    }
    assert_eq!(seen.len(), paths.len());
}

/// The kept model never drifts from what the browser's state says: after
/// every kind of message, in an order that reaches each arm of `update`,
/// it equals the model worked out from scratch.
#[test]
fn the_kept_model_matches_a_fresh_one_after_every_message() {
    use loom_patch::metadata::LicenceFilter;
    let dir = library("fresh");
    let paths = patch_files(&dir, &AtomicBool::new(false));
    let mut browser = PatchBrowser::new(1, dir.to_path_buf());
    let check = |browser: &PatchBrowser, step: &str| {
        assert_eq!(browser.model(), &browser.fresh_model(), "after {step}");
    };
    check(&browser, "open");
    let mut steps: Vec<(&str, Message)> = vec![(
        "Found for an earlier opening",
        Message::Found {
            open: 0,
            rows: vec![Row::read(&paths[0])],
        },
    )];
    for path in &paths {
        steps.push((
            "Found",
            Message::Found {
                open: 1,
                rows: vec![Row::read(path)],
            },
        ));
    }
    steps.extend([
        ("Step down", Message::Step(1)),
        ("Hover", Message::Hover(2)),
        ("Step up", Message::Step(-1)),
        ("Search", Message::Search("bass".into())),
        ("Search cleared", Message::Search(String::new())),
        ("Author", Message::Author("sam".into())),
        ("Author with no match", Message::Author("nobody at all".into())),
        ("Author cleared", Message::Author(String::new())),
        ("Tag on", Message::Tag("drone".into())),
        ("Licence on", Message::Licence(LicenceFilter::LetsMeChange, true)),
        ("Licence off", Message::Licence(LicenceFilter::LetsMeChange, false)),
        ("Tag off", Message::Tag("drone".into())),
        ("Pick", Message::Pick(0)),
        ("Submit", Message::Submit),
        ("Close", Message::Close),
    ]);
    for (step, message) in steps {
        let _ = browser.update(message);
        check(&browser, step);
    }
}

/// Escape closes Your patches from Close and from a check box too, through
/// the runtime's keys, as from a field or a tag.
#[test]
fn escape_closes_from_close_and_a_check_box() {
    let dir = library("escape");
    let mut ui = open(&dir);
    let _ = ui.tap(Key::Tab, Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), Some(&part_id("close")));
    tap(&mut ui, Key::Escape);
    assert!(ui.app().overlays.patches().is_none(), "from Close");

    let mut ui = open(&dir);
    for _ in 0..2 {
        tap(&mut ui, Key::Tab);
    }
    assert_eq!(ui.runtime().focused(), Some(&licence_id("lets-me-change")));
    tap(&mut ui, Key::Escape);
    assert!(ui.app().overlays.patches().is_none(), "from a check box");
}

/// Enter that opens a patch with unsaved changes asks about them, and the
/// same Enter does not answer the question it opened: the runtime routes
/// the key before any of its updates run, so the question, opened by one
/// of them, hears nothing of it (ui-layer.md, section 6, rule 6). Before
/// this was so, Enter in the search field discarded the changes at once.
#[test]
fn enter_that_opens_a_patch_does_not_answer_the_question_it_asks() {
    let dir = library("enter-question");
    let mut ui = open(&dir);
    ui.app_mut().edit(|app| {
        app.session.controller.add_module("loom.osc.sine").unwrap();
        app.commit();
    });
    // The pad's row, by name: the folder's order differs between systems,
    // and the unreadable file opens nothing.
    ui.typewrite("Pad");
    let _ = ui.redraw();
    let rows = &model(&ui).rows;
    let highlight = model(&ui).highlight.expect("a row is highlighted");
    assert!(
        rows[highlight].name.contains("Pad"),
        "the Pad row: {:?}",
        rows[highlight].name
    );
    tap(&mut ui, Key::Enter);
    assert!(
        ui.app().overlays.question.is_some(),
        "the question about the unsaved changes still waits for its answer"
    );
}
