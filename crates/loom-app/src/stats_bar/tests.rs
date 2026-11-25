//! Tests for the stats bar drawn by `loom-ui` (plan 3.2, row 1): what a
//! screen reader meets, the pointer and the keys on its ×, the developer
//! switches, hover text, and the room it leaves Iced above it.

use super::*;
use crate::ui_test::Ui;
use accesskit::{Action as KitAction, Role as KitRole, Toggled};
use iced::keyboard::{self, key::Named};
use loom_ui_core::Point;

/// The subtractive patch with module 4 selected, so the inspector shows
/// its rows and no patch info fields, and an error kept in the bar.
fn ui_with_error() -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().session.selection = [4].into_iter().collect();
    ui.app_mut().alerts.error(alerts::Cause::File, "save failed");
    ui.redraw();
    ui
}

/// The UI ID written `text`.
fn id(text: &str) -> UiId {
    text.parse().expect("a UI ID")
}

/// A key going down and coming up.
fn tap(ui: &mut Ui, key: Named) {
    ui.tap(keyboard::Key::Named(key), keyboard::Modifiers::default());
}

/// The bar sits at the window's foot, as high as the token, and Iced's
/// space for it ends where the bar starts: nothing of Iced's is under it.
#[test]
fn the_bar_fills_the_space_iced_keeps() {
    let mut ui = Ui::open("subtractive.ron");
    ui.lay_out_panels();
    let bar = ui.runtime().bounds(&stats_id()).expect("the bar is laid out");
    assert_eq!(bar.height, style::STATS_BAR_HEIGHT);
    assert_eq!(bar.y + bar.height, crate::WINDOW.height);
    assert_eq!(bar.width, crate::WINDOW.width);
    let canvas = ui.canvas();
    assert!(
        canvas.bounds.y + canvas.bounds.height <= bar.y,
        "the canvas ends above the bar: {canvas:?}"
    );
}

/// The bar is a group named "Status bar". Its plain items are labels with
/// the text shown, hover text is the description, and ordinary status text
/// is a label. A warning in the status line is a status, so it is
/// announced (ui-layer.md, section 7, the role table).
#[test]
fn the_bar_reports_its_items_and_the_status() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().alerts.say("Saved");
    let tree = ui.a11y_tree();
    let bar = tree.node_by_id(&stats_id());
    assert_eq!(bar.role(), KitRole::Group);
    assert_eq!(bar.name(), Some("Status bar"));
    assert!(bar.bounds().is_some(), "placed by the runtime's layout");
    let model = ui.app().bars_model();
    for item in &model.items {
        let found = tree.node_by_id(&stats_id().with(item.name));
        assert_eq!(found.value(), Some(item.text.as_str()), "{}", item.name);
        assert_eq!(found.description(), item.hover.as_deref(), "{}", item.name);
        let alert = item.kind == bars::Kind::Alert;
        assert_eq!(
            found.role(),
            if alert { KitRole::Status } else { KitRole::Label },
            "{}",
            item.name
        );
    }
    let status = tree.node_by_id(&stats_id().with("status"));
    assert_eq!(status.role(), KitRole::Label);
    assert_eq!(status.value(), Some("Saved"));
    assert_eq!(
        status.description(),
        None,
        "hover text that repeats the value is not read twice"
    );

    ui.app_mut().alerts.warn("The audio device is gone");
    let tree = ui.a11y_tree();
    let status = tree.node_by_id(&stats_id().with("status"));
    assert_eq!(status.role(), KitRole::Status);
    assert!(status.value().is_some_and(|text| text.contains("gone")));

    assert!(crate::a11y::tree::is_live_bar_item(&stats_id().with("load")));
    assert!(crate::a11y::tree::is_live_bar_item(&stats_id().with("dropouts")));
    assert!(!crate::a11y::tree::is_live_bar_item(&stats_id().with("midi")));
}

/// The × is a button named by its hover text, a focus stop. A click, Enter,
/// Space, and a screen reader's Click each dismiss the error, through the
/// one message (section 3, rule 4).
#[test]
fn the_x_dismisses_by_pointer_key_and_screen_reader() {
    let dismiss = id(bars::STATUS_DISMISS_ID);
    for way in ["click", "enter", "space", "reader"] {
        let mut ui = ui_with_error();
        let tree = ui.a11y_tree();
        let x = tree.node_by_id(&dismiss);
        assert_eq!(x.role(), KitRole::Button);
        assert_eq!(x.name(), Some("Dismiss this error"));
        assert!(x.offers(KitAction::Click) && x.offers(KitAction::Focus), "{way}");
        match way {
            "click" => {
                let at = ui.runtime().bounds(&dismiss).expect("the ×").center();
                ui.move_to(at);
                ui.press();
                ui.release();
            }
            "enter" | "space" => {
                // From nothing: no tile selected either, or the canvas
                // holds the keys and walks its tiles (section 6, rule 1).
                ui.app_mut().session.selection.clear();
                ui.redraw();
                ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::SHIFT);
                assert_eq!(
                    ui.runtime().focused(),
                    Some(&dismiss),
                    "Shift+Tab from nothing reaches the ×, the last stop"
                );
                tap(&mut ui, if way == "enter" { Named::Enter } else { Named::Space });
            }
            _ => ui.a11y_action(&dismiss, KitAction::Click),
        }
        ui.redraw();
        assert!(ui.app().alerts.kept().is_empty(), "{way} dismissed it");
    }
}

/// The developer switches are switches. Space on a focused one turns it,
/// and so does a screen reader's Click; without the tools there are none.
#[test]
fn the_developer_switches_turn_with_space_and_a_screen_reader() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().session.selection = [4].into_iter().collect();
    ui.app_mut().dev_tools = true;
    ui.redraw();
    let measure = id(bars::MEASURE_ID);
    let tree = ui.a11y_tree();
    assert_eq!(tree.node_by_id(&measure).role(), KitRole::Switch);
    assert_eq!(tree.node_by_id(&measure).name(), Some("Measure"));
    assert_eq!(tree.node_by_id(&measure).toggled(), Some(Toggled::False));
    assert_eq!(tree.node_by_id(&id(bars::MOVE_CAMERA_ID)).name(), Some("Move camera"));

    let back = |ui: &mut Ui| {
        ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    };
    // From nothing: no tile selected either (section 6, rule 1).
    ui.app_mut().session.selection.clear();
    ui.redraw();
    back(&mut ui);
    back(&mut ui);
    assert_eq!(ui.runtime().focused(), Some(&measure));
    tap(&mut ui, Named::Space);
    ui.redraw();
    assert!(ui.app().measuring, "Space turned it on");
    tap(&mut ui, Named::Enter);
    ui.redraw();
    assert!(ui.app().measuring, "Enter does not turn a switch");

    ui.a11y_action(&measure, KitAction::Click);
    ui.redraw();
    assert!(!ui.app().measuring, "a screen reader's Click turned it off");

    ui.app_mut().dev_tools = false;
    assert!(ui.app().bar_click(&measure).is_none(), "no switch, no click");
}

/// Hover text: the × shows its own over the status line's, and the status
/// line shows all of its text, at once, as Iced's tooltips did.
#[test]
fn hover_text_shows_over_the_bar() {
    let mut ui = ui_with_error();
    let x = ui.runtime().bounds(&id(bars::STATUS_DISMISS_ID)).expect("the ×");
    ui.move_to(x.center());
    ui.lay_out_panels();
    let (shown, rect) = ui.runtime().hover_text().expect("the ×'s hover text");
    assert_eq!(shown, &id(bars::STATUS_DISMISS_ID));
    assert!(rect.y + rect.height <= x.y, "above the ×");
    let status = ui.runtime().bounds(&stats_id().with("status")).expect("the status");
    ui.move_to(Point::new(status.x + 4.0, status.center_y()));
    ui.lay_out_panels();
    assert_eq!(
        ui.runtime().hover_text().map(|(id, _)| id.clone()),
        Some(stats_id().with("status"))
    );
}

/// The space above the bar takes no point: with the bar's panels there, a
/// press on a tile still selects its module, and one on the toolbar still
/// reaches Iced's button.
#[test]
fn presses_above_the_bar_reach_the_canvas_and_the_toolbar() {
    let mut ui = Ui::open("subtractive.ron");
    ui.lay_out_panels();
    let found = crate::control::Found {
        canvas: Some(ui.canvas()),
        ..Default::default()
    };
    let tile = crate::control::locate(ui.app(), "4", &found).expect("module 4 is on the canvas");
    ui.move_to(Point::new(tile.x + 8.0, tile.y + 4.0));
    ui.press();
    ui.release();
    assert!(ui.app().session.selection.contains(&4), "the press reached the canvas");
    let heard = ui.click_text("Preferences");
    assert!(
        heard.iter().any(|m| matches!(m, Message::Preferences(_))),
        "the press reached the toolbar: {heard:?}"
    );
}

/// While a panel owns the input, the bar's nodes are hidden with the rest
/// of the window, so the root's shown children no longer hold the bar.
#[test]
fn the_bar_is_hidden_below_an_open_panel() {
    let mut ui = ui_with_error();
    let shown = |ui: &mut Ui| {
        ui.a11y_tree()
            .root()
            .children()
            .iter()
            .any(|child| child.name() == Some("Status bar"))
    };
    assert!(shown(&mut ui), "the bar shows");
    ui.app_mut()
        .overlays
        .open(crate::overlays::Overlay::RecentList(Vec::new()));
    ui.redraw();
    assert!(!shown(&mut ui), "the bar is hidden below the open list");
}

/// The subtractive patch with nothing selected, so patch info's two fields
/// show above the bar, with an error kept in the bar or none.
fn ui_with_patch_info(error: bool) -> Ui {
    let mut ui = Ui::open("subtractive.ron");
    if error {
        ui.app_mut().alerts.error(alerts::Cause::File, "save failed");
    }
    ui.redraw();
    ui
}

/// Clicks into patch info's Authors box, as a user does.
fn click_authors(ui: &mut Ui) {
    let label = ui.text("Authors, one per line").expect("the patch form shows");
    ui.move_to(Point::new(label.center_x(), label.y + label.height + 14.0));
    ui.press();
    ui.release();
}

/// Where the keyboard is: Iced's field, or the runtime's element.
fn at(ui: &mut Ui) -> (Option<UiId>, Option<UiId>) {
    (ui.focused_field(), ui.runtime().focused().cloned())
}

/// Tab in reading order (section 6, rule 3; Brylie's decisions,
/// 2026-11-18, 2026-11-19, and 2026-11-25): Tab from nothing is the
/// canvas's, as before; patch info's stops are the runtime's since plan
/// 3.2, row 4d, top to bottom; past its last, Keyboard shortcuts, the
/// bar's ×; past the ×, the order wraps to the toolbar's first button,
/// New. Shift+Tab before patch info's first stop reaches the top run's
/// last stop, which at the top level is the toolbar's one stop, on its
/// first button the first time.
#[test]
fn tab_reads_the_canvas_patch_info_the_bar_then_the_toolbar() {
    let [authors, tags] = crate::patch_info::text_fields();
    let patch = crate::inspector::ids::patch();
    let part = |key: &'static str| crate::inspector::ids::part(&patch, key);
    let x = id(bars::STATUS_DISMISS_ID);
    let new = crate::toolbar::toolbar_id().with("new");
    let mut from_nothing = ui_with_patch_info(true);
    tap(&mut from_nothing, Named::Tab);
    assert_eq!(at(&mut from_nothing), (None, None), "from nothing, nothing of ours");
    assert!(
        !from_nothing.app().session.selection.is_empty(),
        "the canvas took it, as before"
    );

    let mut ui = ui_with_patch_info(true);
    click_authors(&mut ui);
    assert_eq!(ui.runtime().focused(), Some(&authors));
    for stop in [tags, part("choose_licence"), part("shortcuts")] {
        tap(&mut ui, Named::Tab);
        assert_eq!(ui.runtime().focused(), Some(&stop));
    }
    tap(&mut ui, Named::Tab);
    assert_eq!(at(&mut ui), (None, Some(x.clone())), "past patch info, the bar");
    assert!(ui.runtime().focus_ring().is_some(), "from the keyboard, with a ring");
    tap(&mut ui, Named::Tab);
    assert_eq!(
        at(&mut ui),
        (None, Some(new.clone())),
        "past the bar, the toolbar's first"
    );

    // Back the other way: before patch info's first stop, the top run's
    // last stop.
    let mut ui = ui_with_patch_info(true);
    click_authors(&mut ui);
    ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    assert_eq!(at(&mut ui), (None, Some(new)), "before patch info, the toolbar");
}

/// Shift+Tab from nothing enters the bar's last stop, the last in reading
/// order; before the bar's first, patch info's last stop takes it.
#[test]
fn shift_tab_from_nothing_enters_the_bar() {
    let x = id(bars::STATUS_DISMISS_ID);
    let shortcuts = crate::inspector::ids::part(&crate::inspector::ids::patch(), "shortcuts");
    let mut ui = ui_with_patch_info(true);
    ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    assert_eq!(at(&mut ui), (None, Some(x)), "from nothing, the last stop");
    ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    assert_eq!(
        at(&mut ui),
        (None, Some(shortcuts)),
        "before the bar, patch info's last stop"
    );
}

/// With no stop in the bar, patch info's stops hand Tab on to the
/// toolbar, which always has a stop: past the last, the toolbar's first
/// button.
#[test]
fn with_no_stop_in_the_bar_the_fields_hand_on_to_the_toolbar() {
    let shortcuts = crate::inspector::ids::part(&crate::inspector::ids::patch(), "shortcuts");
    let mut ui = ui_with_patch_info(false);
    assert!(ui.app().bar_stops().is_empty());
    click_authors(&mut ui);
    for _ in 0..3 {
        tap(&mut ui, Named::Tab);
    }
    assert_eq!(ui.runtime().focused(), Some(&shortcuts));
    tap(&mut ui, Named::Tab);
    assert_eq!(
        at(&mut ui),
        (None, Some(crate::toolbar::toolbar_id().with("new"))),
        "past patch info, the toolbar"
    );
}

/// The app's list of the runtime's stops is the runtime's own order, so
/// the hand-offs and Tab inside each run agree.
#[test]
fn the_app_stops_are_the_runtimes() {
    let mut ui = ui_with_error();
    ui.app_mut().dev_tools = true;
    ui.app_mut().alerts.say("saved");
    ui.redraw();
    ui.lay_out_panels();
    let stops = ui.app().bar_stops();
    assert_eq!(stops.len(), 3, "two switches and the kept error's ×: {stops:?}");
    assert_eq!(ui.runtime().focus_stops(), ui.app().our_stops().as_slice());
}

/// A status longer than the bar is cut, not the fixed items: each keeps its
/// width, and the status starts after the last of them and ends inside the
/// window, as Iced's clipped field did.
#[test]
fn a_long_status_is_cut_and_the_items_keep_their_width() {
    let mut ui = Ui::open("subtractive.ron");
    let widths = |ui: &mut Ui| -> Vec<(&'static str, f32)> {
        ui.lay_out_panels();
        let model = ui.app().bars_model();
        model
            .items
            .iter()
            .map(|item| {
                let at = ui.runtime().bounds(&stats_id().with(item.name)).expect("laid out");
                (item.name, at.width)
            })
            .collect()
    };
    ui.app_mut().alerts.say("short");
    ui.redraw();
    let before = widths(&mut ui);
    for (name, width) in &before {
        assert!(*width > 0.0, "{name} has a width to keep");
    }
    ui.app_mut().alerts.say("a long message ".repeat(40));
    ui.redraw();
    assert_eq!(widths(&mut ui), before, "each item keeps the width it had");
    let model = ui.app().bars_model();
    let mut right = 0.0_f32;
    for item in &model.items {
        let at = ui.runtime().bounds(&stats_id().with(item.name)).expect("laid out");
        right = right.max(at.x + at.width);
    }
    let status = ui.runtime().bounds(&stats_id().with("status")).expect("the status");
    assert!(status.x >= right, "the status starts after the items: {status:?}");
    assert!(status.x + status.width <= crate::WINDOW.width, "and ends in the window");
}

/// `locate` on a status line too long for the bar gives the part of it
/// that shows, cut by its row, so the point it gives is on visible text.
#[test]
fn locate_on_a_cut_status_gives_the_part_that_shows() {
    let mut ui = Ui::open("subtractive.ron");
    let long = "a long message ".repeat(40);
    ui.app_mut().alerts.say(long.clone());
    ui.redraw();
    ui.lay_out_panels();
    let status = ui.runtime().bounds(&stats_id().with("status")).expect("the status");
    let text = ui
        .runtime()
        .text_bounds(long.trim_end())
        .or_else(|| ui.runtime().text_bounds(&long));
    let text = text.expect("the status shows part of its text");
    assert!(
        text.x >= status.x && text.x + text.width <= status.x + status.width,
        "{text:?} in {status:?}"
    );
    assert!(status.contains(text.center()), "the point is on what shows");
}

/// A screen reader's Focus on the bar's × moves the keyboard there, as the
/// node's Focus action says it does (section 7, rule 6), so Enter then
/// dismisses the error. Focus on a × the bar no longer shows does nothing.
#[test]
fn a_screen_readers_focus_moves_the_keyboard_to_the_x() {
    let x = id(bars::STATUS_DISMISS_ID);
    let mut ui = ui_with_error();
    let node = ui.a11y_tree().node_by_id(&x).node_id();
    ui.a11y_action(&x, KitAction::Focus);
    assert_eq!(ui.runtime().focused(), Some(&x));
    tap(&mut ui, Named::Enter);
    ui.redraw();
    assert!(ui.app().alerts.kept().is_empty(), "Enter on the focused × dismissed it");
    assert!(!ui.app().bar_stops().contains(&x), "and the × is gone");
    // A Focus on the node the × had, sent again, moves nothing now.
    let before = ui.runtime().focused().cloned();
    let request = accesskit::ActionRequest {
        action: KitAction::Focus,
        target_tree: accesskit::TreeId::ROOT,
        target_node: node,
        data: None,
    };
    let _ = ui.deliver(Message::A11y(crate::a11y::Event::Action(request)));
    ui.redraw();
    assert_eq!(
        ui.runtime().focused().cloned(),
        before,
        "Focus on a gone × does nothing"
    );
}

/// A test's change made outside a turn (`app_mut`) is shown by the next
/// rebuild, and a rebuild that an update causes uses that change up too, so
/// no extra rebuild waits after it.
#[test]
fn a_rebuild_after_an_update_uses_up_a_stale_view() {
    let mut ui = Ui::open("subtractive.ron");
    ui.app_mut().alerts.say("changed outside a turn");
    assert!(ui.runtime().view_is_stale());
    let _ = ui.deliver(Message::Tick);
    assert!(!ui.runtime().view_is_stale(), "the update's rebuild used it up");
}

/// A screen reader's Focus on a bar stop ends its open undo group, as a
/// focus move on the canvas does, so a step after it is a step of its own.
#[test]
fn focus_on_a_bar_stop_ends_the_screen_readers_undo_group() {
    use crate::edits::ReaderGroup;
    let mut ui = ui_with_error();
    let knob = id("canvas/4/param/cutoff");
    ui.a11y_action(&knob, KitAction::Increment);
    assert_ne!(
        ui.app().session.edits.reader_group(),
        ReaderGroup::Closed,
        "the step opened a group"
    );
    ui.a11y_action(&id(bars::STATUS_DISMISS_ID), KitAction::Focus);
    assert_eq!(ui.app().session.edits.reader_group(), ReaderGroup::Closed);
}

/// A screen reader's Focus on the bar's ×, while a canvas control has the
/// keyboard, takes the keyboard from the canvas without selecting
/// anything: the selected tile stays selected, and Enter on the × then
/// dismisses the error rather than going to the canvas.
#[test]
fn focus_on_the_x_takes_the_keyboard_from_the_canvas() {
    let mut ui = ui_with_error();
    let knob = id("canvas/4/param/cutoff");
    ui.a11y_action(&knob, KitAction::Focus);
    assert!(ui.app().a11y.focus().is_some(), "the canvas control has the keyboard");
    assert!(
        ui.app().session.selection.contains(&4),
        "Focus on the knob selected its tile"
    );
    let selected = ui.app().session.selection.clone();
    ui.a11y_action(&id(bars::STATUS_DISMISS_ID), KitAction::Focus);
    assert_eq!(ui.app().a11y.focus(), None, "the canvas let go");
    assert_eq!(ui.app().session.selection, selected, "nothing else selected");
    tap(&mut ui, Named::Enter);
    ui.redraw();
    assert!(ui.app().alerts.kept().is_empty(), "Enter on the × dismissed the error");
}

/// A change made while a tree rebuild is in flight reaches the screen
/// reader: a rebuild asked for while one is on its way is run once that
/// one is sent, so an autosave's warning that arrives then is in the tree
/// the follow-up sends.
#[test]
fn a_warning_during_a_rebuild_reaches_the_next_tree() {
    use crate::a11y::{Event, Touch};
    let reads_warning = |tree: &accesskit::TreeUpdate| {
        tree.nodes
            .iter()
            .any(|(_, node)| node.value().is_some_and(|value| value.contains("Autosave failed")))
    };
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.deliver(Message::A11y(Event::Activated));
    ui.redraw();
    let _ = ui.take_sent_trees();
    let _ = ui.take_sent_changes();
    // A rebuild goes out, and the warning comes while it is on its way.
    ui.app_mut().a11y_after(Touch::Changed);
    ui.app_mut().alerts.warn("Autosave failed");
    ui.app_mut().a11y_after(Touch::Changed);
    for _ in 0..4 {
        ui.redraw();
    }
    let sent = ui.take_sent_trees();
    // Exactly one more, not "at least": a rebuild that kept asking for
    // itself would also send two or more.
    assert_eq!(sent.len(), 2, "the rebuild and exactly one more");
    assert!(
        sent.last().is_some_and(reads_warning),
        "the follow-up tree reads the warning"
    );
    assert!(
        ui.take_sent_changes().iter().any(reads_warning),
        "the warning reached the adapter"
    );
}

/// A screen reader's Focus on the bar's × lets go of the canvas even when
/// the canvas has not yet taken an earlier Focus: a knob's Focus and the
/// ×'s Focus arrive with no frame between them, so the knob's request is
/// still on its way. On the next frame the canvas must not take the keys
/// back, and Enter must reach the ×.
#[test]
fn focus_on_the_x_cancels_a_canvas_focus_still_on_its_way() {
    let mut ui = ui_with_error();
    let tree = ui.a11y_tree();
    let knob = tree.node_by_id(&id("canvas/4/param/cutoff")).node_id();
    let x = tree.node_by_id(&id(bars::STATUS_DISMISS_ID)).node_id();
    drop(tree);
    for node in [knob, x] {
        let request = accesskit::ActionRequest {
            action: KitAction::Focus,
            target_tree: accesskit::TreeId::ROOT,
            target_node: node,
            data: None,
        };
        let _ = ui.deliver(Message::A11y(crate::a11y::Event::Action(request)));
    }
    ui.redraw();
    assert_eq!(ui.app().a11y.focus(), None, "the canvas did not take the keys back");
    assert_eq!(ui.runtime().focused(), Some(&id(bars::STATUS_DISMISS_ID)));
    tap(&mut ui, Named::Enter);
    ui.redraw();
    assert!(ui.app().alerts.kept().is_empty(), "Enter on the × dismissed the error");
}

/// With a tile selected and nothing else focused, the canvas holds the
/// keys (section 6, rule 1; Brylie's decision, 2026-11-19): Shift+Tab
/// walks the canvas's tiles, and does not enter the bar, though the bar
/// has a stop.
#[test]
fn shift_tab_with_a_tile_selected_walks_the_canvas() {
    let mut ui = ui_with_error();
    assert!(ui.app().canvas_has_keys(), "tile 4 is selected");
    ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), None, "the bar's × was not entered");
    assert_eq!(ui.app().session.selection.len(), 1, "the canvas selected a tile");
    assert_ne!(ui.app().session.selection, [4].into_iter().collect(), "another one");
}

/// A keyboard user reaches the bar's × with a tile selected: Escape lets
/// the selection go, and Shift+Tab from nothing then enters the bar's last
/// stop, the ×.
#[test]
fn the_x_is_reached_from_a_selected_tile_by_escape_then_shift_tab() {
    let mut ui = ui_with_error();
    let x = id(bars::STATUS_DISMISS_ID);
    tap(&mut ui, Named::Escape);
    ui.redraw();
    assert!(ui.app().session.selection.is_empty(), "Escape let the selection go");
    ui.tap(keyboard::Key::Named(Named::Tab), keyboard::Modifiers::SHIFT);
    assert_eq!(ui.runtime().focused(), Some(&x));
}
