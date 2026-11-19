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
    }

    /// Every focus stop of the runtime's, in the order Tab meets them: the
    /// top run (the toolbar's, then the breadcrumb's), then the bar's.
    pub(crate) fn our_stops(&self) -> Vec<UiId> {
        let mut stops = self.top_stops();
        stops.extend(self.bar_stops());
        stops
    }

    /// Where Tab goes from the runtime's stop `from` (ui-layer.md, section
    /// 6, rule 3, from row 2; Brylie's decisions of 2026-11-18). Forward
    /// out of the top run, the key goes on to the canvas, which takes it as
    /// Tab from nothing. Back from the top run's first stop, to the bar's
    /// last, and forward from the bar's last, to the top run's first: the
    /// order wraps. Back from the bar's first stop, to patch info's last
    /// field. `next` is the stop after `from` in the runtime's reading
    /// order, which inside each run is where Tab goes.
    pub(crate) fn tab_from(&self, from: &UiId, next: Option<&UiId>, back: bool) -> loom_ui::TabStep<Message> {
        use loom_ui::TabStep;
        let (top, bar) = (self.top_stops(), self.bar_stops());
        let to = |stop: Option<&UiId>| stop.map_or(TabStep::Leave, |stop| TabStep::To(stop.clone()));
        let in_top = top.contains(from);
        let in_bar = bar.contains(from);
        if in_top && !back && top.last() == Some(from) {
            TabStep::Leave
        } else if in_top && back && top.first() == Some(from) {
            to(bar.last())
        } else if in_bar && !back && bar.last() == Some(from) {
            to(top.first())
        } else if in_bar && back && bar.first() == Some(from) {
            if self.fields_among_ours() {
                TabStep::Send(Message::EnterFields { backwards: true })
            } else {
                TabStep::Leave
            }
        } else {
            to(next)
        }
    }

    /// The top run's stops: the toolbar's turned-on buttons, then, inside
    /// a sub-patch, the breadcrumb's entries.
    pub(crate) fn top_stops(&self) -> Vec<UiId> {
        let mut stops = self.toolbar_stops();
        stops.extend(self.breadcrumb_stops());
        stops
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
