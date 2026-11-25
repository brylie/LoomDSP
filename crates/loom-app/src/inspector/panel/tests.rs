use super::*;
use crate::Message;
use crate::patch_info::{Message as Info, authors_part, tags_part};
use crate::ui_test::Ui;
use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};
use loom_canvas::CanvasEvent;

/// The example patch with nothing selected, so the inspector shows patch
/// info, drawn by the runtime.
fn patch_info() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(Vec::new())));
    let _ = ui.redraw();
    assert!(ui.app().inspector_ours(), "patch info is the runtime's");
    ui
}

/// The first module of the example patch.
fn first_module(ui: &Ui) -> u32 {
    ui.app()
        .session
        .controller
        .graph()
        .nodes()
        .map(|(id, _)| id.0)
        .min()
        .expect("the example patch has modules")
}

/// The centre of the runtime's part `id`, as last laid out.
fn centre(ui: &mut Ui, id: &UiId) -> loom_ui_core::Point {
    ui.lay_out_panels();
    let at = ui.runtime().bounds(id).unwrap_or_else(|| panic!("no part {id}"));
    loom_ui_core::Point::new(at.x + at.width / 2.0, at.y + at.height / 2.0)
}

/// Presses and releases the pointer at `at`.
fn click(ui: &mut Ui, at: loom_ui_core::Point) {
    let _ = ui.move_to(at);
    let _ = ui.press();
    let _ = ui.release();
}

/// Patch info, a selection of several, and a module that went are the
/// runtime's; a module's own inspector is still Iced's, and so is any
/// other tab, until their rows move (plan 3.2, row 4d).
#[test]
fn the_runtime_draws_only_the_variants_moved_so_far() {
    let mut ui = patch_info();
    let module = first_module(&ui);
    ui.lay_out_panels();
    assert!(ui.runtime().bounds(&tags_part()).is_some(), "patch info is laid out");

    ui.send(Message::Canvas(CanvasEvent::Select(vec![module])));
    let _ = ui.redraw();
    assert!(!ui.app().inspector_ours(), "a module's inspector is Iced's");
    ui.lay_out_panels();
    assert!(
        ui.runtime().bounds(&tags_part()).is_none(),
        "the runtime draws no column"
    );

    let mut modules: Vec<u32> = ui
        .app()
        .session
        .controller
        .graph()
        .nodes()
        .map(|(id, _)| id.0)
        .collect();
    modules.truncate(2);
    ui.send(Message::Canvas(CanvasEvent::Select(modules)));
    let _ = ui.redraw();
    assert!(ui.app().inspector_ours(), "several is the runtime's");

    ui.send(Message::Canvas(CanvasEvent::Select(Vec::new())));
    ui.send(Message::Matrix(crate::MatrixEvent::Tab(Tab::Matrix)));
    assert!(!ui.app().inspector_ours(), "the matrix tab is Iced's");
}

/// The root panel's fill still passes a press on to the canvas and to
/// Iced's tabs above the column: the slot takes only its own room.
#[test]
fn the_canvas_and_the_tabs_still_take_presses() {
    let mut ui = patch_info();
    let module = first_module(&ui);
    let control = ui
        .app()
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(module))
        .and_then(|node| node.descriptor.params.first())
        .map(|param| param.name)
        .expect("a module with a parameter");
    let at = ui.control(module, control);
    click(&mut ui, at);
    assert!(
        ui.app().session.selection.contains(&module),
        "the canvas took the press"
    );

    ui.send(Message::Canvas(CanvasEvent::Select(Vec::new())));
    let _ = ui.redraw();
    let _ = ui.click_text("Matrix");
    assert_eq!(ui.app().right_tab, Tab::Matrix, "the tab took the press");
}

/// Typing a tag and pressing Enter adds it, and its remove button takes
/// it away again: one tag for one press, from the pointer or a screen
/// reader (patch-metadata.md, Patch info).
#[test]
fn a_tag_is_added_and_removed_once() {
    let mut ui = patch_info();
    ui.focus(&tags_part());
    let _ = ui.typewrite("drone, dark");
    let _ = ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    assert_eq!(ui.app().session.document.info().tags, ["drone", "dark"]);
    assert_eq!(ui.app().session.patch_info.tag_draft, "");

    let remove = |tag: &str| ids::part(&ids::part(&ids::patch(), "tag").with(tag), "remove");
    let at = centre(&mut ui, &remove("drone"));
    click(&mut ui, at);
    assert_eq!(ui.app().session.document.info().tags, ["dark"], "one press, one tag");

    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    ui.a11y_action(&remove("dark"), accesskit::Action::Click);
    assert!(
        ui.app().session.document.info().tags.is_empty(),
        "a screen reader's Click removes it"
    );
}

/// The Authors box (ui-layer.md, section 8, rule 7): Enter inserts a line
/// break, the box keeps what was typed while it has focus, the stored
/// authors are tidied, and once focus leaves the box shows them. Patch
/// info's box makes no undo step.
#[test]
fn the_authors_box_keeps_its_draft_until_focus_leaves() {
    let mut ui = patch_info();
    let steps = ui.app().session.edits.history.len();
    ui.focus(&authors_part());
    let _ = ui.typewrite("Ada");
    let _ = ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    let _ = ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    let _ = ui.typewrite("Sam");
    assert_eq!(ui.app().session.patch_info.authors, "Ada\n\nSam", "as typed");
    assert_eq!(ui.app().session.document.info().authors, ["Ada", "Sam"], "tidied");
    assert_eq!(ui.focused_field(), Some(authors_part()), "Enter kept the focus");

    // Focus leaving, with a press on the Tags field, shows the stored list.
    let at = centre(&mut ui, &tags_part());
    click(&mut ui, at);
    assert_eq!(ui.focused_field(), Some(tags_part()));
    assert_eq!(ui.app().session.patch_info.authors, "Ada\nSam");
    assert_eq!(ui.app().session.edits.history.len(), steps, "no undo step");
}

/// A screen reader's SetValue on the Authors box takes the path focus
/// leaving takes: the stored authors are tidied and shown at once, with
/// no undo step.
#[test]
fn a_screen_readers_set_value_on_the_authors_is_stored_and_shown() {
    let mut ui = patch_info();
    let steps = ui.app().session.edits.history.len();
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    ui.a11y_set_value(&authors_part(), " Ada\n\nSam\nada");
    assert_eq!(ui.app().session.document.info().authors, ["Ada", "Sam"]);
    assert_eq!(ui.app().session.patch_info.authors, "Ada\nSam");
    assert_eq!(ui.app().session.edits.history.len(), steps, "no undo step");
}

/// The column scrolls with the wheel over it when patch info is taller
/// than the window leaves it.
#[test]
fn the_wheel_scrolls_the_column() {
    let mut ui = patch_info();
    ui.resize(900.0, 260.0);
    let _ = ui.redraw();
    let column = ids::patch();
    let at = centre(&mut ui, &tags_part());
    let _ = ui.move_to(at);
    let before = ui.runtime().scroll_offset(&column).expect("the column scrolls");
    let _ = ui.step(&[iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -6.0 },
    })]);
    let after = ui.runtime().scroll_offset(&column).expect("the column scrolls");
    // The offset, then the most it can be.
    assert!(after.0 > before.0, "it scrolled: {before:?} to {after:?}");
}

/// While some variants are the runtime's and some Iced's, a selection can
/// swap who draws the column while the focus is in it. The focus never
/// stays on a part that went, either way, and a tag being typed stays in
/// its draft, unadded, as it did on Iced.
#[test]
fn a_selection_that_swaps_the_toolkit_drops_the_focus_it_had() {
    let mut ui = patch_info();
    let module = first_module(&ui);
    ui.focus(&tags_part());
    let _ = ui.typewrite("half");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![module])));
    let _ = ui.redraw();
    assert_eq!(ui.focused_field(), None, "no field keeps the focus");
    assert!(
        ui.app()
            .focus
            .as_ref()
            .is_none_or(|focus| !focus.is_under(&ids::patch())),
        "the app's copy of the focus left patch info"
    );
    assert_eq!(ui.app().session.patch_info.tag_draft, "half", "the draft stays");
    assert!(ui.app().session.document.info().tags.is_empty(), "and is not added");

    // The other way: a field of Iced's, then patch info.
    let crate::inspector::Inspector::Module(rows) = ui.app().inspector_model() else {
        panic!("a module's inspector");
    };
    let index = rows
        .params
        .iter()
        .find(|row| row.field.is_some())
        .map(|row| row.index)
        .expect("a module with a value field");
    let at = ui.value_field(module, index);
    click(&mut ui, at);
    assert!(ui.focused_field().is_some(), "Iced's value field has focus");
    ui.send(Message::Canvas(CanvasEvent::Select(Vec::new())));
    let _ = ui.redraw();
    assert_eq!(ui.focused_field(), None, "no field keeps the focus");
    assert!(ui.app().session.value_draft.is_none(), "the value draft went");
}

/// Under an overlay the column is drawn but inert: a click on one of its
/// buttons does nothing (ui-layer.md, section 7, rule 24).
#[test]
fn under_an_overlay_the_column_takes_no_click() {
    let mut ui = patch_info();
    ui.send(Message::PatchInfo(Info::ChooseLicence));
    let _ = ui.redraw();
    let shortcuts = ids::part(&ids::patch(), "shortcuts");
    assert!(ui.app().inspector_click(&shortcuts).is_none());
    assert!(ui.app().inspector_stops().is_empty());
}

/// Patch info with the tags `tags`, nothing selected.
fn with_tags(tags: &[&str]) -> Ui {
    let mut ui = patch_info();
    ui.app_mut()
        .session
        .document
        .edit_info(|info| info.tags = tags.iter().map(ToString::to_string).collect());
    let _ = ui.redraw();
    ui
}

/// The runtime's focus now.
fn focused(ui: &Ui) -> Option<UiId> {
    ui.runtime().focused().cloned()
}

/// Presses Tab, or Shift+Tab when `back`.
fn tab(ui: &mut Ui, back: bool) {
    let shift = if back { Modifiers::SHIFT } else { Modifiers::empty() };
    let _ = ui.tap(Key::Named(Named::Tab), shift);
}

/// Every control of patch info is in the Tab order, top to bottom as it is
/// laid out (WCAG 2.2, SC 2.4.3; Brylie's decision, 2026-11-25): Authors,
/// the tags as one stop, the Tags field, Choose, Keyboard shortcuts, then
/// the bar. Back from Authors, the top run's last stop.
#[test]
fn tab_walks_patch_info_top_to_bottom() {
    use crate::patch_info::tag_remove_id;
    let mut ui = with_tags(&["drone", "dark", "slow"]);
    let patch = ids::patch();
    let order = [
        authors_part(),
        tag_remove_id("drone"),
        tags_part(),
        ids::part(&patch, "choose_licence"),
        ids::part(&patch, "shortcuts"),
    ];
    ui.lay_out_panels();
    let tops: Vec<f32> = order
        .iter()
        .map(|id| ui.runtime().bounds(id).expect("laid out").y)
        .collect();
    assert!(
        tops.windows(2).all(|pair| pair[0] < pair[1]),
        "laid out top to bottom: {tops:?}"
    );

    ui.focus(&authors_part());
    for id in &order[1..] {
        tab(&mut ui, false);
        assert_eq!(focused(&ui).as_ref(), Some(id));
    }
    // On to the bar, or the top run when the bar has no stop, and back.
    let (bar, top) = (ui.app().bar_stops(), ui.app().top_stops());
    tab(&mut ui, false);
    assert_eq!(focused(&ui).as_ref(), bar.first().or(top.first()), "on past the column");
    tab(&mut ui, true);
    assert_eq!(focused(&ui).as_ref(), order.last(), "and back");
    for id in order[..order.len() - 1].iter().rev() {
        tab(&mut ui, true);
        assert_eq!(focused(&ui).as_ref(), Some(id));
    }
    tab(&mut ui, true);
    assert_eq!(
        focused(&ui).as_ref(),
        ui.app().top_stops().last(),
        "back to the top run"
    );
}

/// The tags are one stop, a toolbar named "Tags" (the WAI-ARIA toolbar
/// pattern, as in Your patches): Left and Right move and stop at the ends,
/// Home and End go to them, and Tab comes back to the tag last focused.
/// Each remove button keeps its own name.
#[test]
fn the_tags_are_one_toolbar_stop() {
    use crate::patch_info::{tag_remove_id, tags_toolbar_id};
    let mut ui = with_tags(&["drone", "dark", "slow"]);
    let press = |ui: &mut Ui, named: Named| {
        let _ = ui.tap(Key::Named(named), Modifiers::empty());
    };
    ui.focus(&authors_part());
    tab(&mut ui, false);
    assert_eq!(focused(&ui), Some(tag_remove_id("drone")));
    press(&mut ui, Named::ArrowLeft);
    assert_eq!(focused(&ui), Some(tag_remove_id("drone")), "Left stops at the first");
    press(&mut ui, Named::ArrowRight);
    assert_eq!(focused(&ui), Some(tag_remove_id("dark")));
    press(&mut ui, Named::End);
    assert_eq!(focused(&ui), Some(tag_remove_id("slow")));
    press(&mut ui, Named::ArrowRight);
    assert_eq!(focused(&ui), Some(tag_remove_id("slow")), "Right stops at the last");
    press(&mut ui, Named::Home);
    press(&mut ui, Named::ArrowRight);
    tab(&mut ui, false);
    assert_eq!(focused(&ui), Some(tags_part()), "Tab leaves the tags");
    tab(&mut ui, true);
    assert_eq!(
        focused(&ui),
        Some(tag_remove_id("dark")),
        "back to the tag last focused"
    );

    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let tree = ui.a11y_tree();
    let toolbar = tree.node_by_id(&tags_toolbar_id());
    assert_eq!(toolbar.role(), accesskit::Role::Toolbar);
    assert_eq!(toolbar.name(), Some("Tags"));
    let remove = tree.node_by_id(&tag_remove_id("dark"));
    assert_eq!(remove.name(), Some("Remove tag dark"));
}

/// Removing the focused tag moves the focus to the next tag, or the
/// previous at the end, or the Tags field once none are left, and the
/// tags' Tab stop never lands on a tag that went.
#[test]
fn removing_the_focused_tag_keeps_the_keyboards_place() {
    use crate::patch_info::tag_remove_id;
    let mut ui = with_tags(&["drone", "dark", "slow"]);
    let enter = |ui: &mut Ui| {
        let _ = ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    };
    ui.focus(&authors_part());
    tab(&mut ui, false);
    let _ = ui.tap(Key::Named(Named::ArrowRight), Modifiers::empty());
    enter(&mut ui);
    assert_eq!(ui.app().session.document.info().tags, ["drone", "slow"]);
    assert_eq!(focused(&ui), Some(tag_remove_id("slow")), "the next tag");
    enter(&mut ui);
    assert_eq!(focused(&ui), Some(tag_remove_id("drone")), "the previous, at the end");
    assert_eq!(ui.app().session.patch_info.last_tag.as_deref(), Some("drone"));
    enter(&mut ui);
    assert!(ui.app().session.document.info().tags.is_empty());
    assert_eq!(focused(&ui), Some(tags_part()), "the Tags field, once none are left");
    assert!(
        ui.app()
            .inspector_stops()
            .iter()
            .all(|stop| !stop.is_under(&crate::patch_info::tags_toolbar_id())),
        "no stop for a tag that went"
    );
}

/// Escape on one of patch info's buttons gives the keys back to the
/// canvas, as from the toolbar.
#[test]
fn escape_on_a_button_returns_to_the_canvas() {
    let mut ui = patch_info();
    ui.focus(&authors_part());
    tab(&mut ui, false);
    assert_eq!(focused(&ui), Some(tags_part()));
    tab(&mut ui, false);
    let _ = ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(focused(&ui), None, "the column let the focus go");
}

/// Every node patch info gives has a role, every part but text has a name,
/// every part that acts offers its action and lies in the column, and the
/// parts Iced's items gave keep their IDs (plan 3.2, row 4d's gate).
#[test]
fn patch_infos_nodes_are_named_and_placed() {
    let mut ui = with_tags(&["drone"]);
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let tree = ui.a11y_tree();
    let patch = ids::patch();
    let column = tree.node_by_id(&patch);
    assert_eq!(column.name(), Some("Patch"));
    let left = column.bounds().expect("the column has bounds").x0;
    let parts = tree.parts_under(&patch);
    for id in &parts {
        let node = tree.node_by_id(id);
        // A field's text runs are its text, for a screen reader's caret.
        if node.role() == accesskit::Role::TextRun {
            continue;
        }
        if node.role() == accesskit::Role::Label {
            assert!(node.value().is_some_and(|text| !text.is_empty()), "{id} says its words");
            continue;
        }
        assert!(node.name().is_some_and(|name| !name.is_empty()), "{id} has a name");
        if node.role() == accesskit::Role::Button {
            assert!(node.offers(accesskit::Action::Click), "{id} can be pressed");
            let bounds = node.bounds().expect("a button has bounds");
            assert!(bounds.x0 >= left - 0.5, "{id} lies in the column");
        }
    }
    let part = |key: &'static str| ids::part(&patch, key);
    for id in [
        part("name"),
        authors_part(),
        crate::patch_info::tag_remove_id("drone"),
        tags_part(),
        part("licence"),
        part("choose_licence"),
        part("hint"),
        part("shortcuts"),
    ] {
        assert!(parts.contains(&id), "{id} is reported");
    }
    assert_eq!(
        tree.node_by_id(&authors_part()).role(),
        accesskit::Role::MultilineTextInput
    );
}
