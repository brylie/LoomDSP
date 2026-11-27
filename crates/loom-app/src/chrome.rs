//! The parts of the window around the canvas that `loom-ui` draws (plan for
//! leaving Iced, step 3.2, rows 1 and 2; ui-layer.md, section 5, rule 10).
//!
//! One root panel, `window`, fills the window and holds, top to bottom, the
//! toolbar's slot, the breadcrumb's (inside a sub-patch only), a fill with
//! no key that paints nothing, and the stats bar's slot. A point in the
//! fill has no element of the runtime's, so it goes on to Iced and the
//! canvas. Iced keeps transparent spaces of the same heights where these
//! were (`view.rs`), from the same tokens, so the canvas starts and ends
//! where the panels do. One root, not one per part, so the window is laid
//! out and hit-tested once, and each part keeps its own panel and UI IDs.

use crate::{App, Message, style};
use loom_ui::{Element, Layer, Length, View};
use loom_ui_core::UiId;

/// The root panel around the toolbar, the breadcrumb, and the bar.
pub(crate) fn window_id() -> UiId {
    UiId::panel("window")
}

impl App {
    /// Adds the root panel and every panel in its slots to the runtime's
    /// view.
    pub(crate) fn chrome_panels(&self, view: &mut View) {
        let shape = Shape {
            inside: !self.session.level.is_root(),
            content: if self.inspector_ours() {
                Some(crate::inspector::panel::inspector_id())
            } else if self.matrix_ours() {
                Some(crate::matrix::panel::matrix_id())
            } else {
                self.outline_slot()
            },
        };
        view.panel_with(window_id(), Layer::Panels, &shape, root);
        self.toolbar_panel(view);
        self.breadcrumb_panel(view);
        self.bar_panel(view);
        self.tabs_panel(view);
        self.inspector_panel(view);
        self.outline_panels(view);
        self.matrix_panel(view);
        // The browsers, then Preferences, then the chooser over it, then
        // help, which F1 opens over any of them (help-pages.md), so help
        // draws over them.
        self.browser_panel(view);
        self.patches_panel(view);
        self.preferences_panel(view);
        self.chooser_panel(view);
        self.help_panel(view);
        self.question_panel(view);
        self.menu_panel(view);
    }

    /// True when the canvas holds the keys (ui-layer.md, section 6, rule 1;
    /// Brylie's decision, 2026-11-19): one of its controls has focus, or,
    /// with nothing open over the canvas and no field or element of either
    /// toolkit focused, a tile is selected. The canvas then takes Tab and
    /// Shift+Tab, and walks its tiles, as the arrow keys move a selected
    /// tile; the runtime's stops are not entered from it, and Enter and
    /// Space activate none of them. The one answer every rule reads.
    pub(crate) fn canvas_has_keys(&self) -> bool {
        use crate::input_owner::InputOwner;
        let control = self.a11y.focus().is_some();
        let tile = matches!(self.input_owner(), InputOwner::Canvas)
            && self.focus.is_none()
            && !self.session.selection.is_empty();
        control || tile
    }

    /// Every focus stop of the runtime's: each toolbar button, the
    /// breadcrumb's entries, then the bar's. A screen reader's Focus can
    /// reach any of them; Tab meets the toolbar as one stop
    /// ([`Self::top_stops`]).
    pub(crate) fn our_stops(&self) -> Vec<UiId> {
        let mut stops = self.question_stops();
        stops.extend(self.help_stops());
        stops.extend(self.chooser_stops());
        stops.extend(self.browser_stops());
        stops.extend(self.patches_stops());
        stops.extend(self.patches_tags());
        stops.extend(self.preferences_stops());
        stops.extend(self.menu_stops());
        stops.extend(self.toolbar_stops());
        stops.extend(self.breadcrumb_stops());
        stops.extend(self.tabs_stops());
        stops.extend(self.inspector_stops());
        stops.extend(self.inspector_tags());
        stops.extend(self.outline_stops());
        stops.extend(self.matrix_stops());
        stops.extend(self.matrix_cells());
        stops.extend(self.bar_stops());
        stops
    }

    /// Where Tab goes from the runtime's stop `from` (ui-layer.md, section
    /// 6, rule 3, from row 2; Brylie's decisions of 2026-11-18 and
    /// 2026-11-19). The toolbar is one stop (the WAI-ARIA toolbar
    /// pattern): Tab into it lands on [`Self::toolbar_entry`], and Tab from
    /// any of its buttons leaves it. Forward out of the top run, the key
    /// goes on to the canvas, which takes it as Tab from nothing. Back from
    /// the top run's first stop, to the bar's last, and forward from the
    /// bar's last, to the top run's first: the order wraps. Back from the
    /// bar's first stop, to patch info's last field. `next` is the stop
    /// after `from` in the runtime's reading order, which inside the bar is
    /// where Tab goes.
    pub(crate) fn tab_from(&self, from: &UiId, next: Option<&UiId>, back: bool) -> loom_ui::TabStep<Message> {
        use loom_ui::TabStep;
        // The question keeps Tab among its answers, wrapping at both ends
        // (section 6, rule 6, step 2; the WAI-ARIA dialog pattern).
        // As a Move, which wraps and takes the focus along, so Tab from a
        // script, which reaches the question as its key, gives the same.
        if self.question_stops().contains(from) {
            let by = if back { -1 } else { 1 };
            return TabStep::Send(Message::Question(crate::question::Message::Move(by)));
        }
        // Tab stays inside help, wrapping at both ends (the WAI-ARIA modal
        // dialog pattern; help-pages.md, Keys, links, and focus).
        if self.help_stops().contains(from) {
            return self
                .help_next_stop(Some(from), back)
                .map_or(TabStep::Leave, TabStep::To);
        }
        // The browser's search field is its dialog's only stop, so Tab and
        // Shift+Tab stay on it (module-browser.md, Focus and Tab; the
        // WAI-ARIA modal dialog pattern).
        if self.browser_stops().contains(from) {
            return TabStep::To(from.clone());
        }
        // Tab loops inside Your patches, the tags as one stop
        // (patch-metadata.md, Keys; the WAI-ARIA modal dialog and toolbar
        // patterns).
        if let Some(to) = self.patches_next_stop(from, back) {
            return TabStep::To(to);
        }
        // Tab stays inside the licence chooser, wrapping at both ends (the
        // WAI-ARIA modal dialog pattern; patch-metadata.md, Keys).
        if let Some(to) = self.chooser_tab(from, next, back) {
            return TabStep::To(to);
        }
        // Tab stays inside Preferences, wrapping at both ends (the WAI-ARIA
        // modal dialog pattern; preferences.md, Keys and focus).
        if from.is_under(&crate::settings::preferences_id()) {
            return self
                .preferences_next_stop(from, back)
                .map_or(TabStep::Leave, TabStep::To);
        }
        // Tab on a menu item closes the menu and moves on from what opened
        // it (the WAI-ARIA menu pattern; section 6, rule 8).
        if self.menu_stops().contains(from) {
            return TabStep::Send(Message::MenuTab { back });
        }
        let (top, bar) = (self.top_stops(), self.bar_stops());
        let to = |stop: Option<&UiId>| stop.map_or(TabStep::Leave, |stop| TabStep::To(stop.clone()));
        // The tab list is one stop before its panel (the WAI-ARIA Tabs
        // pattern): Tab goes into the open panel, and Shift+Tab to the top
        // run's last stop, where the column's first went before.
        if self.tabs_stops().contains(from) {
            return if back {
                to(top.last().or(bar.last()))
            } else {
                self.tab_into_panel(next)
            };
        }
        // The inspector's column, while the runtime draws it (plan 3.2,
        // row 4d): between the canvas and the bar in reading order.
        if let Some(step) = self.inspector_next_stop(from, back) {
            return step;
        }
        // The matrix, its grid one stop (plan 3.2, row 4e).
        if let Some(step) = self.matrix_next_stop(from, back) {
            return step;
        }
        let column = if self.matrix_owns() {
            self.matrix_stops()
        } else {
            self.inspector_stops()
        };
        // A module's, an unknown module's, or a slot's rows have no list of
        // stops of the app's: Tab follows the runtime's reading order among
        // them, and past either end leaves the column as patch info does.
        // A step onto the toolbar lands on its one stop.
        if self.inspector_owns() && from.is_under(&crate::inspector::panel::inspector_id()) {
            let toolbar = self.toolbar_stops();
            let step = match next {
                Some(stop) if toolbar.contains(stop) => to(self.toolbar_entry().as_ref()),
                Some(stop) => TabStep::To(stop.clone()),
                None if back => to(bar.last().or(top.last())),
                None => to(bar.first().or(top.first())),
            };
            return step;
        }
        let rows = self.inspector_owns() && column.is_empty();
        // Any toolbar button stands for the toolbar's one stop.
        let toolbar = self.toolbar_stops();
        let entry = self.toolbar_entry();
        let from = match &entry {
            Some(entry) if toolbar.contains(from) => entry,
            _ => from,
        };
        if let Some(at) = top.iter().position(|stop| stop == from) {
            if back {
                at.checked_sub(1).map_or_else(
                    || {
                        if rows && bar.is_empty() {
                            to(next)
                        } else {
                            to(bar.last().or(column.last()))
                        }
                    },
                    |at| to(top.get(at)),
                )
            } else {
                top.get(at + 1).map_or(TabStep::Leave, |stop| TabStep::To(stop.clone()))
            }
        } else if bar.contains(from) && !back && bar.last() == Some(from) {
            to(top.first())
        } else if bar.contains(from) && back && bar.first() == Some(from) {
            if let Some(last) = column.last() {
                TabStep::To(last.clone())
            } else if rows {
                to(next)
            } else if self.fields_among_ours() {
                TabStep::Send(Message::EnterFields { backwards: true })
            } else {
                TabStep::Leave
            }
        } else {
            to(next)
        }
    }

    /// The top run's stops as Tab meets them: the toolbar as one stop, its
    /// [`Self::toolbar_entry`], then, inside a sub-patch, the breadcrumb's
    /// entries, one stop each, since a breadcrumb is a list of links, not
    /// a toolbar.
    pub(crate) fn top_stops(&self) -> Vec<UiId> {
        let mut stops: Vec<UiId> = self.toolbar_entry().into_iter().collect();
        stops.extend(self.breadcrumb_stops());
        stops
    }

    /// The toolbar button Tab and Ctrl+Tab enter the toolbar on: the one
    /// last focused in this level while it is still turned on, otherwise
    /// the first turned-on button. `None` when every button is off.
    pub(crate) fn toolbar_entry(&self) -> Option<UiId> {
        let stops = self.toolbar_stops();
        self.session
            .outline
            .last_toolbar
            .clone()
            .filter(|id| stops.contains(id))
            .or_else(|| stops.into_iter().next())
    }
}

/// What the root's slots are: the breadcrumb's is there `inside` a
/// sub-patch, and the right column's content slot holds the panel the
/// runtime draws there: the inspector, the matrix, the Outline's tree, or
/// the jump list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shape {
    inside: bool,
    content: Option<UiId>,
}

/// The root: the toolbar's slot, the breadcrumb's inside a sub-patch, the
/// fill beside the right column, and the bar's slot. The column is at the
/// right edge, as wide as Iced's: the tabs' slot (plan 3.2, row 4e), then
/// the content's slot, which holds the open tab's panel, then the rule over
/// the bar.
fn root(shape: &Shape) -> Element {
    let slot = |id: UiId, height: f32| Element::slot(id).width(Length::FILL).height(Length::Fixed(height));
    let mut parts = vec![slot(crate::toolbar::toolbar_id(), style::TOOLBAR_HEIGHT)];
    if shape.inside {
        parts.push(slot(crate::breadcrumb::breadcrumb_id(), style::BREADCRUMB_HEIGHT));
    }
    let fill = Element::space(Length::FILL, Length::FILL);
    let content = if let Some(panel) = shape.content.clone() {
        Element::slot(panel)
            .width(Length::FILL)
            .height(Length::FILL)
            .min_height(0.0)
    } else {
        Element::space(Length::FILL, Length::FILL)
    };
    let column = Element::column([
        slot(crate::tabs::tabs_id(), style::RIGHT_TABS_HEIGHT),
        content,
        Element::space(Length::FILL, Length::Fixed(style::RULE)),
    ])
    .width(Length::Fixed(style::RIGHT_COLUMN_WIDTH))
    .height(Length::FILL);
    parts.push(Element::row([fill, column]).width(Length::FILL).height(Length::FILL));
    parts.push(slot(crate::stats_bar::stats_id(), style::STATS_BAR_HEIGHT));
    Element::column(parts).width(Length::FILL).height(Length::FILL)
}
