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

/// Every variant of the inspector is the runtime's since plan 3.2, row
/// 4d.3, a sub-patch's included; the other tabs are still Iced's, until 4e.
#[test]
fn the_runtime_draws_every_variant_and_no_other_tab() {
    let mut ui = nested_patch_info();
    ui.lay_out_panels();
    assert!(ui.runtime().bounds(&tags_part()).is_some(), "patch info is laid out");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![SUB_PATCH])));
    let _ = ui.redraw();
    assert!(ui.app().inspector_ours(), "a sub-patch's inspector is the runtime's");
    ui.lay_out_panels();
    assert!(
        ui.runtime()
            .bounds(&ids::part(&ids::panel(SUB_PATCH), "name"))
            .is_some()
    );

    ui.send(Message::Canvas(CanvasEvent::Select(Vec::new())));
    ui.send(Message::Matrix(crate::MatrixEvent::Tab(Tab::Matrix)));
    assert!(!ui.app().inspector_ours(), "the matrix tab is Iced's");
    ui.lay_out_panels();
    assert!(
        ui.runtime().bounds(&tags_part()).is_none(),
        "the runtime draws no column"
    );
}

/// The sub-patch tile of the nested example patch.
const SUB_PATCH: u32 = 2;

/// The nested example patch with nothing selected.
fn nested_patch_info() -> Ui {
    let mut ui = Ui::open("nested-subtractive.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(Vec::new())));
    let _ = ui.redraw();
    assert!(ui.app().inspector_ours(), "patch info is the runtime's");
    ui
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

/// A selection that replaces what the column shows while the focus is in
/// it leaves no focus on a part that went, and a tag being typed is added,
/// since leaving the Tags field by any means adds it (Brylie's decision,
/// 2026-11-25). Each variant is the runtime's since row 4d.3.
#[test]
fn a_selection_that_replaces_the_column_drops_the_focus_it_had() {
    let mut ui = nested_patch_info();
    ui.focus(&tags_part());
    let _ = ui.typewrite("half");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![SUB_PATCH])));
    let _ = ui.redraw();
    assert!(
        ui.app()
            .focus
            .as_ref()
            .is_none_or(|focus| !focus.is_under(&ids::patch())),
        "the app's copy of the focus left patch info"
    );
    assert_eq!(ui.app().session.patch_info.tag_draft, "", "the draft is done with");
    assert_eq!(ui.app().session.document.info().tags, ["half"], "and added");
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

    // A real press on Keyboard shortcuts, under the chooser, opens nothing:
    // the routing, not only the guard.
    let at = centre(&mut ui, &shortcuts);
    click(&mut ui, at);
    assert!(ui.app().overlays.help.is_none(), "no shortcut help opened");
    assert!(ui.app().overlays.chooser.is_some(), "the chooser stays open");
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
    // Before the first, the tab list, one stop before its panel (the
    // WAI-ARIA Tabs pattern; plan 3.2, row 4e), then the top run.
    tab(&mut ui, true);
    assert_eq!(
        focused(&ui),
        Some(crate::tabs::tab_id(crate::matrix::Tab::Inspector)),
        "back to the tab list"
    );
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
    assert_eq!(
        column.role(),
        accesskit::Role::ScrollView,
        "a scroll view (section 7, rule 28)"
    );
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

/// A press on Keyboard shortcuts opens the shortcut help: the column's
/// buttons take a real pointer's click.
#[test]
fn a_press_on_keyboard_shortcuts_opens_help() {
    let mut ui = patch_info();
    let at = centre(&mut ui, &ids::part(&ids::patch(), "shortcuts"));
    click(&mut ui, at);
    assert!(ui.app().overlays.help.is_some(), "the shortcut help opened");
}

/// A choice's radios are keyed directly under `<row>/value`, by name for a
/// list whose names switch with a count and by index otherwise (the 4d.2
/// renames). No two of any registered module's choices share a key, and
/// the radio group holds nothing else, so none can collide.
#[test]
fn no_choice_key_collides_under_its_value() {
    let mut checked = 0;
    for descriptor in loom_modules::registry().iter() {
        for spec in descriptor.params {
            let names = spec.options.choices;
            if names.is_empty() {
                continue;
            }
            let keys: std::collections::BTreeSet<String> = names
                .iter()
                .enumerate()
                .map(|(at, name)| format!("{:?}", super::rows::choice_key(spec, at, name)))
                .collect();
            assert_eq!(keys.len(), names.len(), "{} {}: {names:?}", descriptor.id, spec.name);
            checked += 1;
        }
    }
    assert!(checked > 0, "some module has choices");
}

/// In learn mode, a parameter's Learn button is on its mapping line,
/// `<row>/midi/line/learn`, with no mapping yet and once mapped, so its ID
/// does not move when a mapping is made; Options and × join it there.
#[test]
fn learn_stays_on_the_mapping_line_with_and_without_a_mapping() {
    let mut ui = Ui::open("modulation.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.send(Message::Learn(crate::learn::Message::Toggle));
    let _ = ui.redraw();
    assert!(ui.app().inspector_ours(), "a module's inspector is the runtime's");
    let row = ids::row(4, ids::Kind::Param, "cutoff");
    let line = ids::part(&row, "midi/line");
    let learn = ids::part(&row, "midi/line/learn");
    ui.lay_out_panels();
    assert!(ui.runtime().bounds(&learn).is_some(), "Learn with no mapping");
    assert!(ui.runtime().bounds(&ids::part(&row, "midi/line/remove")).is_none());

    ui.app_mut()
        .session
        .controller
        .map_midi_named(
            loom_core::MidiControl {
                channel: 0,
                controller: 74,
            },
            loom_core::ModuleId(4),
            "cutoff",
        )
        .unwrap();
    ui.send(Message::Tick);
    let _ = ui.redraw();
    ui.lay_out_panels();
    for part in [
        learn,
        ids::part(&row, "midi/line/options"),
        ids::part(&row, "midi/line/remove"),
    ] {
        let at = ui.runtime().bounds(&part).unwrap_or_else(|| panic!("{part} is drawn"));
        let on_line = ui.runtime().bounds(&line).expect("the line");
        assert!(
            at.y >= on_line.y - 0.5 && at.y + at.height <= on_line.y + on_line.height + 0.5,
            "{part} is on the line"
        );
    }
}

/// A module's column taller than the window scrolls with the wheel, and
/// its rows' nodes move with the rows (section 7, rule 7); the column's
/// node is as tall as what shows, not as what it holds, so a screen reader
/// does not take rows out of view as visible. The runtime's versions of
/// the Iced inspector's scroll tests (plan 3.2, row 4d.2).
#[test]
fn a_modules_column_scrolls_its_rows_and_says_what_shows() {
    let mut ui = Ui::open("subtractive.ron");
    ui.resize(900.0, 300.0);
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let column = ids::panel(4);
    let row = ids::row(4, ids::Kind::Param, "resonance");
    let tree = ui.a11y_tree();
    let shown = tree.node_by_id(&column).bounds().expect("the column's bounds");
    let before = tree.node_by_id(&row).bounds().expect("the row's bounds");
    ui.lay_out_panels();
    let (_, most) = ui.runtime().scroll_offset(&column).expect("the column scrolls");
    assert!(most > 0.0, "the rows are taller than the window leaves them");
    let slot = ui.runtime().bounds(&column).expect("the column is laid out");
    let scale = f64::from(ui.app().window_scale);
    assert!(
        (shown.height() - f64::from(slot.height) * scale).abs() < 1.0,
        "the column's node is as tall as what shows: {shown:?} {slot:?}"
    );

    let at = centre(&mut ui, &ids::part(&ids::panel(4), "description"));
    let _ = ui.move_to(at);
    let _ = ui.step(&[iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 },
    })]);
    let _ = ui.redraw();
    let after = ui.a11y_tree().node_by_id(&row).bounds().expect("the row's bounds");
    assert!(
        after.y0 < before.y0,
        "the row's node moved up with the row: {before:?} to {after:?}"
    );
}

/// A screen reader's SetValue on a row's value field commits the value as
/// one undo step, as Iced's field did: it goes to the app's item, not the
/// runtime's field, which would only make a draft of it (plan 3.2, row
/// 4d.2).
#[test]
fn a_screen_readers_set_value_on_a_value_field_commits_one_step() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let row = ids::row(4, ids::Kind::Param, "cutoff");
    let field = ids::part(&row, "field");
    let steps = ui.app().session.edits.history.len();
    ui.a11y_set_value(&field, "1234");
    let node = ui
        .app()
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(4))
        .unwrap();
    let value = node.params[node.descriptor.param_index("cutoff").unwrap()];
    assert!((value - 1234.0).abs() < 1.0, "committed: {value}");
    assert!(ui.app().session.value_draft.is_none(), "no draft left");
    assert_eq!(ui.app().session.edits.history.len(), steps + 1, "one undo step");
}

/// Two screen reader Increments on an inspector slider in quick turn, then
/// only time passing: the runtime wakes at the group's deadline, the group
/// ends, and the two steps are one undo step (section 7, rule 6).
#[test]
fn a_screen_readers_slider_steps_end_with_time_alone() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let slider = ids::part(&ids::row(4, ids::Kind::Param, "cutoff"), "value");
    let steps = ui.app().session.edits.history.len();
    ui.a11y_action(&slider, accesskit::Action::Increment);
    ui.a11y_action(&slider, accesskit::Action::Increment);
    assert_eq!(ui.app().session.edits.history.len(), steps, "the group is still open");
    let heard = ui.wake_after(std::time::Duration::from_millis(600));
    let ends = heard.iter().filter(|m| matches!(m, Message::SliderReleased)).count();
    assert_eq!(ends, 1, "one End: {heard:?}");
    assert_eq!(ui.app().session.edits.history.len(), steps + 1, "one undo step");
}

/// Leaving the Tags field adds what is typed, by a press elsewhere or by
/// Tab, as Enter does; Escape drops it (Brylie's decision, 2026-11-25).
#[test]
fn leaving_the_tags_field_adds_the_typed_tag_and_escape_drops_it() {
    let mut ui = patch_info();
    ui.focus(&tags_part());
    let _ = ui.typewrite("drone");
    let _ = ui.tap(Key::Named(Named::Tab), Modifiers::empty());
    assert_eq!(ui.app().session.document.info().tags, ["drone"], "Tab adds it");

    ui.focus(&tags_part());
    let _ = ui.typewrite("dark");
    let at = centre(&mut ui, &authors_part());
    click(&mut ui, at);
    assert_eq!(
        ui.app().session.document.info().tags,
        ["drone", "dark"],
        "a press elsewhere adds it"
    );

    ui.focus(&tags_part());
    let _ = ui.typewrite("slow");
    let _ = ui.tap(Key::Named(Named::Escape), Modifiers::empty());
    assert_eq!(
        ui.app().session.document.info().tags,
        ["drone", "dark"],
        "Escape drops it"
    );
    assert_eq!(ui.app().session.patch_info.tag_draft, "");
}

/// A sub-patch's inspector on the runtime (plan 3.2, row 4d.3): the Mode
/// select changes the mode, a layer's members are rows of
/// `<t>/members/<member>` with Up turned off on the first and Down on the
/// last, Copies sets a stack's count, and a spread's slider moves it as one
/// undo step.
#[test]
fn the_sub_patch_mode_rows_act() {
    let mut ui = nested_patch_info();
    ui.send(Message::Canvas(CanvasEvent::Select(vec![SUB_PATCH])));
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let panel = ids::panel(SUB_PATCH);
    let mode = |ui: &Ui| match ui.app().inspector_model() {
        crate::inspector::Inspector::SubPatch(s) => s.mode,
        _ => panic!("a sub-patch's inspector"),
    };
    let layer = loom_patch::Mode::ALL
        .iter()
        .position(|m| *m == loom_patch::Mode::Layer)
        .expect("a layer mode");
    let chose = ui
        .app()
        .inspector_chose(&ids::part(&panel, "mode"), layer)
        .expect("a choice");
    ui.send(chose);
    let _ = ui.redraw();
    assert_eq!(mode(&ui), loom_patch::Mode::Layer);
    let crate::inspector::Inspector::SubPatch(sub) = ui.app().inspector_model() else {
        panic!("a sub-patch's inspector");
    };
    let members = ids::part(&panel, "members");
    let tree = ui.a11y_tree();
    let first = members.clone().with(sub.members[0].id);
    assert!(
        tree.node_by_id(&first.clone().with("up")).is_disabled(),
        "the first can't go up"
    );
    let last = members.with(sub.members.last().unwrap().id);
    assert!(
        tree.node_by_id(&last.with("down")).is_disabled(),
        "the last can't go down"
    );

    let stack = loom_patch::Mode::ALL
        .iter()
        .position(|m| *m == loom_patch::Mode::Stack)
        .expect("a stack mode");
    ui.send(ui.app().inspector_chose(&ids::part(&panel, "mode"), stack).unwrap());
    ui.send(ui.app().inspector_chose(&ids::part(&panel, "copies"), 2).unwrap());
    let _ = ui.redraw();
    let crate::inspector::Inspector::SubPatch(sub) = ui.app().inspector_model() else {
        panic!("a sub-patch's inspector");
    };
    assert_eq!(sub.copies, 3, "the third option is three copies");
    if let Some(spread) = sub.spread.first() {
        let id = ids::part(&panel, "spread").with(spread.name.as_str());
        let steps = ui.app().session.edits.history.len();
        let before = spread.amount;
        ui.a11y_action(&id, accesskit::Action::Increment);
        let _ = ui.wake_after(std::time::Duration::from_millis(600));
        let crate::inspector::Inspector::SubPatch(sub) = ui.app().inspector_model() else {
            panic!("a sub-patch's inspector");
        };
        assert!(sub.spread[0].amount > before, "the spread moved");
        assert_eq!(ui.app().session.edits.history.len(), steps + 1, "one undo step");
    }
}

/// A sub-patch's name field takes typing as a draft and Enter applies it,
/// and its Authors box is a multi-line text input whose SetValue, with no
/// focus, is saved and shown at once as one undo step.
#[test]
fn the_sub_patch_name_and_authors_are_runtime_fields() {
    let mut ui = nested_patch_info();
    ui.send(Message::Canvas(CanvasEvent::Select(vec![SUB_PATCH])));
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let name = ids::part(&ids::panel(SUB_PATCH), "name");
    ui.focus(&name);
    let _ = ui.tap(Key::Named(Named::End), Modifiers::empty());
    let _ = ui.typewrite("X");
    let _ = ui.tap(Key::Named(Named::Enter), Modifiers::empty());
    let crate::inspector::Inspector::SubPatch(sub) = ui.app().inspector_model() else {
        panic!("a sub-patch's inspector");
    };
    assert!(sub.name.ends_with('X'), "renamed: {}", sub.name);

    ui.unfocus_all();
    let authors = crate::patch_info::part_authors_id(SUB_PATCH);
    let tree = ui.a11y_tree();
    assert_eq!(tree.node_by_id(&authors).role(), accesskit::Role::MultilineTextInput);
    let steps = ui.app().session.edits.history.len();
    ui.a11y_set_value(&authors, " Ada\n\nSam");
    let crate::inspector::Inspector::SubPatch(sub) = ui.app().inspector_model() else {
        panic!("a sub-patch's inspector");
    };
    assert_eq!(sub.authors, ["Ada", "Sam"]);
    assert_eq!(
        ui.app().session.patch_info.part_authors(SUB_PATCH),
        Some("Ada\nSam"),
        "shown at once"
    );
    assert_eq!(ui.app().session.edits.history.len(), steps + 1, "one undo step");
}

/// The general check across every kind of control in the inspector's
/// column, as the runtime gives its nodes (plan 3.2, row 4d; it replaces
/// the Iced item list's `every_inspector_part_is_named_and_placed`). For
/// every variant the column shows, across the example patches, at the top
/// level and inside each sub-patch: every role is one section 7's table
/// allows; a check box and a radio say on or off, a select its choice, a
/// slider its number, a field its text; every operable part that is on
/// offers its action; every part but text and a field's runs has a name;
/// every part with bounds has a size and lies inside the column; and the
/// column is a scroll view that cuts its parts' bounds (section 7, rule 28).
#[test]
fn every_inspector_node_is_named_placed_and_acts() {
    use crate::inspector::Inspector;
    use accesskit::{Action, Role};
    let allowed = [
        Role::Group,
        Role::Label,
        Role::Button,
        Role::CheckBox,
        Role::RadioGroup,
        Role::RadioButton,
        Role::ComboBox,
        Role::Slider,
        Role::TextInput,
        Role::MultilineTextInput,
        Role::Toolbar,
        Role::TextRun,
        Role::ScrollView,
    ];
    let mut seen = std::collections::BTreeSet::new();
    let mut check = |ui: &mut Ui, at: &str| {
        let _ = ui.redraw();
        let kind = match ui.app().inspector_model() {
            Inspector::Patch(_) => "patch",
            Inspector::Several(_) => "several",
            Inspector::Port(_) => "port",
            Inspector::Module(_) => "module",
            Inspector::Unknown(_) => "unknown",
            Inspector::Slot(_) => "slot",
            Inspector::SubPatch(_) => "sub-patch",
            Inspector::Empty => return,
        };
        seen.insert(kind);
        let tree = ui.a11y_tree();
        let inspector = inspector_id();
        let parts = tree.parts_under(&inspector);
        // The column's node: the scroll container under the panel.
        let column = parts
            .iter()
            .find(|id| id.keys().len() == inspector.keys().len() + 1)
            .unwrap_or_else(|| panic!("{at} {kind}: the column's node"));
        let column = tree.node_by_id(column);
        assert_eq!(
            column.role(),
            Role::ScrollView,
            "{at} {kind}: the column is a scroll view"
        );
        let shown = column.bounds().expect("the column's bounds");
        for id in &parts {
            let node = tree.node_by_id(id);
            let role = node.role();
            assert!(allowed.contains(&role), "{at} {kind}: {id} is a {role:?}");
            match role {
                Role::CheckBox | Role::RadioButton => {
                    assert!(node.toggled().is_some(), "{at}: {id} says on or off");
                }
                Role::ComboBox => assert!(node.value().is_some(), "{at}: {id} says its choice"),
                Role::Slider => assert!(node.number().is_some(), "{at}: {id} says its number"),
                Role::TextInput | Role::MultilineTextInput => {
                    assert!(node.value().is_some(), "{at}: {id} says its text");
                }
                _ => {}
            }
            let operable = matches!(role, Role::Button | Role::CheckBox | Role::RadioButton);
            if operable && !node.is_disabled() {
                assert!(node.offers(Action::Click), "{at}: {id} can be pressed");
            }
            if role == Role::Slider && !node.is_disabled() {
                assert!(node.offers(Action::Increment), "{at}: {id} steps");
            }
            if !matches!(role, Role::Label | Role::TextRun) {
                assert!(
                    node.name().is_some_and(|name| !name.is_empty()),
                    "{at}: {id} has a name"
                );
            }
            if let Some(bounds) = node.bounds().filter(|_| role != Role::TextRun) {
                // A part scrolled out of view is cut to nothing both ways
                // (section 7, rule 28); one squeezed to nothing one way is
                // a layout bug.
                let (wide, tall) = (bounds.width() > 0.0, bounds.height() > 0.0);
                assert!(wide == tall, "{at}: {id} has a size both ways: {bounds:?} in {shown:?}");
                // Cut by the column as drawn: inside it both ways.
                assert!(
                    bounds.x0 >= shown.x0 - 0.5
                        && bounds.x1 <= shown.x1 + 0.5
                        && bounds.y0 >= shown.y0 - 0.5
                        && bounds.y1 <= shown.y1 + 0.5,
                    "{at}: {id} lies inside the column: {bounds:?} {shown:?}"
                );
            }
        }
    };
    let patches: Vec<String> = std::fs::read_dir("../../examples/patches")
        .expect("the example patches")
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".ron"))
        .collect();
    for patch in &patches {
        let mut ui = Ui::open(patch);
        ui.app_mut().a11y_event(crate::a11y::Event::Activated);
        let tiles: Vec<u32> = ui.app().ui_snapshot().tiles.iter().map(|tile| tile.id).collect();
        ui.send(Message::Canvas(CanvasEvent::Select(Vec::new())));
        check(&mut ui, patch);
        if tiles.len() > 1 {
            ui.send(Message::Canvas(CanvasEvent::Select(tiles[..2].to_vec())));
            check(&mut ui, patch);
        }
        for &tile in &tiles {
            ui.send(Message::Canvas(CanvasEvent::Select(vec![tile])));
            check(&mut ui, patch);
            // Inside a sub-patch: its port tiles and its own modules.
            if matches!(ui.app().inspector_model(), Inspector::SubPatch(_)) {
                ui.send(Message::OpenTile(tile));
                let inner: Vec<u32> = ui.app().ui_snapshot().tiles.iter().map(|tile| tile.id).collect();
                for inside in inner {
                    ui.send(Message::Canvas(CanvasEvent::Select(vec![inside])));
                    check(&mut ui, patch);
                }
                ui.send(Message::Canvas(CanvasEvent::Select(Vec::new())));
                ui.app_mut().show_level(&loom_patch::ModulePath::root());
            }
        }
    }
    // An unknown module, which no example patch has.
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let missing = loom_core::ModuleId(100);
    ui.app_mut()
        .session
        .controller
        .add_unknown_with_id(
            missing,
            loom_core::UnknownModule {
                kind: "future.effect@1".into(),
                params: std::collections::BTreeMap::from([("amount".into(), 0.5)]),
                ranges: std::collections::BTreeMap::new(),
                inputs: vec!["in".into()],
                outputs: vec!["out".into()],
                parameters: vec!["amount".into()],
            },
        )
        .unwrap();
    ui.app_mut().commit();
    ui.send(Message::Canvas(CanvasEvent::Select(vec![missing.0])));
    check(&mut ui, "an unknown module");
    for kind in ["patch", "several", "port", "module", "unknown", "slot", "sub-patch"] {
        assert!(seen.contains(kind), "no example showed the {kind} inspector: {seen:?}");
    }
}

/// The selection now, as text, to compare before and after a key.
fn selection(ui: &Ui) -> String {
    format!("{:?}", ui.app().session.selection)
}

/// Tab and Shift+Tab from a control that a click focused step within the
/// column and never select a tile (WAI-ARIA APG, keyboard navigation
/// within a page; UX review of #900, finding 1): from Authors in patch
/// info, and from a module row's value field and slider.
#[test]
fn tab_after_a_click_stays_in_the_column() {
    let mut ui = patch_info();
    let at = centre(&mut ui, &authors_part());
    click(&mut ui, at);
    assert_eq!(focused(&ui), Some(authors_part()), "the click focuses Authors");
    let before = selection(&ui);
    tab(&mut ui, false);
    assert_eq!(focused(&ui), Some(tags_part()), "Tab goes on to Tags");
    assert_eq!(selection(&ui), before, "Tab selects no tile");

    for part in ["field", "value"] {
        let mut ui = Ui::open("subtractive.ron");
        ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
        let _ = ui.redraw();
        let id = ids::part(&ids::row(4, ids::Kind::Param, "cutoff"), part);
        let at = centre(&mut ui, &id);
        click(&mut ui, at);
        assert_eq!(focused(&ui), Some(id.clone()), "the click focuses {id}");
        let before = selection(&ui);
        for back in [false, true] {
            tab(&mut ui, back);
            let now = focused(&ui).unwrap_or_else(|| panic!("Tab from {id} (back: {back}) dropped the focus"));
            assert!(
                now.is_under(&ids::panel(4)),
                "Tab from {id} (back: {back}) left the column for {now}"
            );
            assert_eq!(selection(&ui), before, "Tab from {id} (back: {back}) selects no tile");
        }
    }
}

/// Ctrl+Tab into the column's Authors field, then Tab, goes on to Tags
/// and selects no tile, as from a click (UX review of #900, finding 1).
#[test]
fn tab_after_ctrl_tab_into_the_column_stays_in_it() {
    let mut ui = patch_info();
    for _ in 0..8 {
        if focused(&ui) == Some(authors_part()) {
            break;
        }
        let _ = ui.tap(Key::Named(Named::Tab), Modifiers::CTRL);
    }
    assert_eq!(focused(&ui), Some(authors_part()), "Ctrl+Tab reaches Authors");
    let before = selection(&ui);
    tab(&mut ui, false);
    assert_eq!(focused(&ui), Some(tags_part()), "Tab goes on to Tags");
    assert_eq!(selection(&ui), before, "Tab selects no tile");
}

/// A screen reader's Focus on an inspector field, while a canvas knob has
/// the keys, gives the field the keys: the knob lets go, so the canvas no
/// longer reports a focus or takes the keys (CodeRabbit on #900).
#[test]
fn a_readers_focus_on_a_field_takes_the_keys_from_a_knob() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    let _ = ui.redraw();
    let knob: UiId = "canvas/4/param/cutoff".parse().unwrap();
    ui.a11y_action(&knob, accesskit::Action::Focus);
    let _ = ui.redraw();
    assert!(ui.app().a11y.focus().is_some(), "the knob has the keys");
    let field = ids::part(&ids::row(4, ids::Kind::Param, "cutoff"), "field");
    ui.a11y_action(&field, accesskit::Action::Focus);
    let _ = ui.redraw();
    assert_eq!(focused(&ui), Some(field), "the field has the focus");
    assert_eq!(ui.app().a11y.focus(), None, "the knob let go");
    assert!(!ui.app().canvas_has_keys(), "the canvas has no keys");
}

/// A slider on an exposed row stores the value the row shows: its
/// position is over the surface's range, here 200 to 2000 Hz, not the
/// filter's whole range (sub-patches.md; CodeRabbit on #900).
#[test]
fn an_exposed_slider_stores_the_value_over_its_surface_range() {
    let patch = include_str!("../../../../../examples/patches/nested-subtractive.ron")
        .replace("range: Some((20.0, 20000.0))", "range: Some((200.0, 2000.0))");
    let mut ui = Ui::with_app(crate::tests::app_with_text(&patch));
    ui.send(Message::Canvas(CanvasEvent::Select(vec![2])));
    let _ = ui.redraw();
    let (svf, cutoff) = {
        let graph = ui.app().session.controller.graph();
        let (svf, node) = graph
            .nodes()
            .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
            .expect("the nested filter");
        (
            svf,
            loom_core::ModuleNames::parameter_index(node, "cutoff").expect("cutoff"),
        )
    };
    let surface = {
        let app = ui.app();
        crate::surface::control_target(
            app.session.controller.graph(),
            &app.session.hierarchy,
            2,
            0,
            loom_canvas::ControlPart::Main,
        )
        .expect("the exposed cutoff")
        .spec
    };
    let whole = ui
        .app()
        .session
        .controller
        .graph()
        .node(svf)
        .unwrap()
        .spec(cutoff)
        .unwrap();
    assert!(surface.max < whole.max, "the surface range is narrower");
    let slider = ids::part(&ids::row(2, ids::Kind::Exposed, "cutoff"), "value");
    let message = ui
        .app()
        // Off the middle: on a log scale, 0.5 is the same frequency over
        // 20 to 20000 Hz as over 200 to 2000 Hz.
        .inspector_slid(&slider, 0.25, loom_ui::SlidePhase::Move)
        .expect("the exposed slider routes");
    ui.send(message);
    let stored = ui.app().session.controller.graph().node(svf).unwrap().params[cutoff];
    let shown = surface.denormalize(0.25);
    assert!((stored - shown).abs() < 1e-3, "stored {stored}, the row shows {shown}");
}

/// A draft that focus leaving commits tells autosave to look again, as
/// Enter does: a tag, and a module's typed value, each committed by a
/// press elsewhere (autosave.md; CodeRabbit on #900).
#[test]
fn a_commit_by_leaving_a_field_wakes_autosave() {
    let mut ui = patch_info();
    ui.focus(&tags_part());
    let _ = ui.typewrite("dark");
    ui.app_mut().session.document.settle();
    let at = centre(&mut ui, &authors_part());
    click(&mut ui, at);
    assert_eq!(ui.app().session.document.info().tags, ["dark"], "the tag is added");
    assert!(
        !ui.app().session.document.is_settled(),
        "autosave looks again after a tag"
    );

    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    let _ = ui.redraw();
    let field = ids::part(&ids::row(4, ids::Kind::Param, "cutoff"), "field");
    let at = centre(&mut ui, &field);
    click(&mut ui, at);
    let _ = ui.typewrite("500");
    ui.app_mut().session.document.settle();
    let other = ids::part(&ids::row(4, ids::Kind::Param, "resonance"), "field");
    let at = centre(&mut ui, &other);
    click(&mut ui, at);
    let (_, cutoff) = (4, 0);
    let stored = ui
        .app()
        .session
        .controller
        .graph()
        .node(loom_core::ModuleId(4))
        .unwrap()
        .params[cutoff];
    assert!((stored - 500.0).abs() < 1e-3, "the value is committed: {stored}");
    assert!(
        !ui.app().session.document.is_settled(),
        "autosave looks again after a value"
    );
}

/// A whole-number parameter steps one value by key however wide its
/// range, as Iced's slider did on main: a noise seed spans 1 to 65535,
/// so a step of 1% would skip about 655 seeds (CodeRabbit on #900).
#[test]
fn a_wide_whole_number_slider_steps_one_value_by_key() {
    let mut ui = Ui::open("plucked-string.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    let _ = ui.redraw();
    let seed = |ui: &Ui| {
        let node = ui
            .app()
            .session
            .controller
            .graph()
            .node(loom_core::ModuleId(4))
            .unwrap();
        let index = loom_core::ModuleNames::parameter_index(node, "seed").expect("a seed");
        node.params[index]
    };
    let slider = ids::part(&ids::row(4, ids::Kind::Param, "seed"), "value");
    ui.focus(&slider);
    assert_eq!(focused(&ui), Some(slider), "the seed's slider has the focus");
    let before = seed(&ui);
    let _ = ui.tap(Key::Named(Named::ArrowRight), Modifiers::empty());
    assert_eq!(seed(&ui), before + 1.0, "one arrow, one seed");
}

/// Under an open overlay the inspector's rows are covered and act on
/// nothing (ui-layer.md, section 5, rule 24): a screen reader's SetValue
/// on a stale node of a row's field changes no value, under help and
/// under Preferences. A MIDI range field takes the same path, the guard
/// in `App::a11y_panel_action` (CodeRabbit on #900).
#[test]
fn a_covered_rows_field_takes_no_screen_readers_value() {
    /// An overlay's name, and how to open it over a selected module.
    type Open = (&'static str, fn(&mut Ui));
    let opens: [Open; 2] = [
        ("help", |ui| ui.send(Message::ShowHelp)),
        ("Preferences", |ui| {
            let _ = ui.tap(Key::Character(",".into()), Modifiers::COMMAND);
        }),
    ];
    let cutoff = |ui: &Ui| {
        ui.app()
            .session
            .controller
            .graph()
            .node(loom_core::ModuleId(4))
            .unwrap()
            .params[0]
    };
    for (name, open) in opens {
        let mut ui = Ui::open("subtractive.ron");
        ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
        ui.app_mut().a11y_event(crate::a11y::Event::Activated);
        let _ = ui.redraw();
        let field = ids::part(&ids::row(4, ids::Kind::Param, "cutoff"), "field");
        let before = cutoff(&ui);
        open(&mut ui);
        let _ = ui.redraw();
        assert!(!ui.app().no_overlay(), "{name} covers the column");
        ui.a11y_set_value(&field, "500");
        let _ = ui.redraw();
        assert_eq!(cutoff(&ui), before, "under {name}, the covered field changed nothing");
    }
}

/// The end of a slider's gesture writes nothing: after two steps, a value
/// set some other way before the group of steps closes, as a MIDI control
/// sets it, is the value left once it closes (CodeRabbit on #900).
#[test]
fn a_sliders_end_keeps_a_value_set_after_its_last_step() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let slider = ids::part(&ids::row(4, ids::Kind::Param, "cutoff"), "value");
    ui.a11y_action(&slider, accesskit::Action::Increment);
    ui.a11y_action(&slider, accesskit::Action::Increment);
    let module = loom_core::ModuleId(4);
    ui.app_mut()
        .session
        .controller
        .set_param(module, 0, 1234.0)
        .expect("set as MIDI would");
    let heard = ui.wake_after(std::time::Duration::from_millis(600));
    let cutoff = ui.app().session.controller.graph().node(module).unwrap().params[0];
    assert_eq!(cutoff, 1234.0, "the end wrote no older position");
    assert!(
        heard.iter().any(|m| matches!(m, Message::SliderReleased)),
        "the group closed"
    );
}

/// A screen reader edits a row's value field as it edits any of the
/// runtime's fields: ReplaceSelectedText on the text the focus selected
/// makes the draft. Only SetValue is the app's, which commits it as one
/// step (section 7, rule 10; CodeRabbit on #900).
#[test]
fn a_readers_replace_selection_on_a_value_field_makes_the_draft() {
    let mut ui = Ui::open("subtractive.ron");
    ui.send(Message::Canvas(CanvasEvent::Select(vec![4])));
    ui.app_mut().a11y_event(crate::a11y::Event::Activated);
    let _ = ui.redraw();
    let field = ids::part(&ids::row(4, ids::Kind::Param, "cutoff"), "field");
    ui.a11y_action(&field, accesskit::Action::Focus);
    assert_eq!(focused(&ui), Some(field.clone()), "the field has the focus");
    ui.a11y_replace_selection(&field, "777");
    let draft = ui.app().session.value_draft.as_ref().map(|d| d.draft.text.clone());
    assert_eq!(draft.as_deref(), Some("777"), "the selected value is replaced");
}
