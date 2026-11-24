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
        let inside = !self.session.level.is_root();
        view.panel_with(window_id(), Layer::Panels, &inside, root);
        self.toolbar_panel(view);
        self.breadcrumb_panel(view);
        self.bar_panel(view);
        // The chooser first, so help, which F1 opens over it, draws over it.
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
        stops.extend(self.menu_stops());
        stops.extend(self.toolbar_stops());
        stops.extend(self.breadcrumb_stops());
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
        // Tab stays inside the licence chooser, wrapping at both ends (the
        // WAI-ARIA modal dialog pattern; patch-metadata.md, Keys).
        if let Some(to) = self.chooser_tab(from, next, back) {
            return TabStep::To(to);
        }
        // Tab on a menu item closes the menu and moves on from what opened
        // it (the WAI-ARIA menu pattern; section 6, rule 8).
        if self.menu_stops().contains(from) {
            return TabStep::Send(Message::MenuTab { back });
        }
        let (top, bar) = (self.top_stops(), self.bar_stops());
        let to = |stop: Option<&UiId>| stop.map_or(TabStep::Leave, |stop| TabStep::To(stop.clone()));
        // Any toolbar button stands for the toolbar's one stop.
        let toolbar = self.toolbar_stops();
        let entry = self.toolbar_entry();
        let from = match &entry {
            Some(entry) if toolbar.contains(from) => entry,
            _ => from,
        };
        if let Some(at) = top.iter().position(|stop| stop == from) {
            if back {
                at.checked_sub(1).map_or_else(|| to(bar.last()), |at| to(top.get(at)))
            } else {
                top.get(at + 1).map_or(TabStep::Leave, |stop| TabStep::To(stop.clone()))
            }
        } else if bar.contains(from) && !back && bar.last() == Some(from) {
            to(top.first())
        } else if bar.contains(from) && back && bar.first() == Some(from) {
            if self.fields_among_ours() {
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

/// The root: the toolbar's slot, the breadcrumb's when `inside` a
/// sub-patch, the fill, and the bar's slot.
fn root(inside: &bool) -> Element {
    let slot = |id: UiId, height: f32| Element::slot(id).width(Length::FILL).height(Length::Fixed(height));
    let mut parts = vec![slot(crate::toolbar::toolbar_id(), style::TOOLBAR_HEIGHT)];
    if *inside {
        parts.push(slot(crate::breadcrumb::breadcrumb_id(), style::BREADCRUMB_HEIGHT));
    }
    parts.push(Element::space(Length::Shrink, Length::FILL));
    parts.push(slot(crate::stats_bar::stats_id(), style::STATS_BAR_HEIGHT));
    Element::column(parts).width(Length::FILL).height(Length::FILL)
}
