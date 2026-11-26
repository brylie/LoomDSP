//! The right column's tabs, Inspector, Matrix, and Outline, as a panel of
//! `loom-ui`'s Panels layer (plan 3.2, row 4e), in a slot of the window's
//! root panel above the open tab's content (`chrome.rs`).
//!
//! They follow the WAI-ARIA Tabs pattern, with automatic activation: the
//! tab list is one focus stop, the open tab; Left and Right choose the
//! previous or next tab, wrapping at the ends, and Home and End the first
//! and the last; the panel follows at once (ui-layer.md, section 6,
//! rule 2). Each tab keeps the UI ID it had, `panel/<tab>`. The tree groups
//! their nodes in a tab list named "Right panel", and the open content in
//! a tab panel the open tab controls (`a11y/tree.rs`).

use crate::matrix::{Event, Tab};
use crate::style;
use crate::{App, Message};
use loom_canvas::iced_convert::ToCore as _;
use loom_ui::access::{Access, Action, Role, States};
use loom_ui::{Align, Element, Layer, Length, Looks, Padding, StateLook, View};
use loom_ui_core::{Color, Key, KeyEvent, Modifiers, UiId};

/// The tabs in the order they show, with their labels.
pub(crate) const TABS: [(Tab, &str); 3] = [
    (Tab::Inspector, "Inspector"),
    (Tab::Matrix, "Matrix"),
    (Tab::Outline, "Outline"),
];

/// The keys the open tab takes before the runtime: Left and Right, Home
/// and End (the WAI-ARIA Tabs pattern). Up and Down are not the tab list's,
/// since its tabs lie across.
const TAB_KEYS: [Key; 4] = [Key::Left, Key::Right, Key::Home, Key::End];

/// The UI ID of the tabs' panel, which the tree's tab list takes too.
pub(crate) fn tabs_id() -> UiId {
    UiId::panel("panel")
}

/// The UI ID of one tab.
pub(crate) fn tab_id(tab: Tab) -> UiId {
    tabs_id().with(key(tab))
}

/// The key of one tab under the panel.
fn key(tab: Tab) -> &'static str {
    match tab {
        Tab::Inspector => "inspector",
        Tab::Matrix => "matrix",
        Tab::Outline => "outline",
    }
}

/// The tab with UI ID `id`, if it names one.
pub(crate) fn tab_of(id: &UiId) -> Option<Tab> {
    TABS.iter().map(|(tab, _)| *tab).find(|tab| tab_id(*tab) == *id)
}

/// The label of `tab`, as it shows and as a screen reader hears it.
pub(crate) fn label(tab: Tab) -> &'static str {
    TABS.iter()
        .find(|(each, _)| *each == tab)
        .map_or("", |(_, label)| label)
}

/// What the tabs' panel is drawn from.
#[derive(Debug, Clone, PartialEq)]
struct Panel {
    /// The open tab.
    open: Tab,
    /// The colours of the tabs, from the theme.
    looks: TabLooks,
}

/// The colours of the tabs, as Iced's tab style drew them (visual-system.md,
/// Tabs): the open one filled and bordered, the others quiet text that
/// fills under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TabLooks {
    fill: Color,
    border: Color,
    text: Color,
    quiet: Color,
}

impl App {
    /// Adds the tabs' panel to the runtime's view.
    pub(crate) fn tabs_panel(&self, view: &mut View) {
        let theme = self.theme();
        let palette = theme.extended_palette();
        let panel = Panel {
            open: self.right_tab,
            looks: TabLooks {
                fill: palette.background.weak.color.to_core(),
                border: palette.background.strong.color.to_core(),
                text: palette.background.base.text.to_core(),
                quiet: style::QUIET.to_core(),
            },
        };
        view.panel_with(tabs_id(), Layer::Panels, &panel, build);
    }

    /// The tabs' one focus stop, the open tab, while nothing is open over
    /// the canvas.
    pub(crate) fn tabs_stops(&self) -> Vec<UiId> {
        if self.input_owner() == crate::input_owner::InputOwner::Canvas {
            vec![tab_id(self.right_tab)]
        } else {
            Vec::new()
        }
    }

    /// The message for a click on a tab, by the pointer, Enter or Space,
    /// or a screen reader: open it.
    pub(crate) fn tabs_click(&self, id: &UiId) -> Option<Message> {
        tab_of(id).map(|tab| Message::Matrix(Event::Tab(tab)))
    }

    /// The message for Left, Right, Home, or End on the open tab: open the
    /// previous or next tab, wrapping, or the first or the last (the
    /// WAI-ARIA Tabs pattern, with automatic activation). `None` for any
    /// other element or key.
    pub(crate) fn tabs_key(&self, id: &UiId, key: &KeyEvent) -> Option<Message> {
        let KeyEvent::Pressed { key, modifiers, .. } = key else {
            return None;
        };
        let at = TABS.iter().position(|(tab, _)| tab_id(*tab) == *id)?;
        if *modifiers != Modifiers::NONE {
            return None;
        }
        let last = TABS.len() - 1;
        let to = match key {
            Key::Left => at.checked_sub(1).unwrap_or(last),
            Key::Right => (at + 1) % TABS.len(),
            Key::Home => 0,
            Key::End => last,
            _ => return None,
        };
        Some(Message::Matrix(Event::Tab(TABS[to].0)))
    }

    /// Where Tab from the open tab goes: into its panel. The runtime's
    /// inspector's first stop is `next` in reading order; with none, Tab
    /// goes on as past the column's last stop. The matrix, which Iced
    /// still draws, is entered as Ctrl+Tab enters it.
    pub(crate) fn tab_into_panel(&self, next: Option<&UiId>) -> loom_ui::TabStep<Message> {
        use loom_ui::TabStep;
        // The Outline's tree, its current row, or the jump list's list.
        if self.outline_slot().is_some() {
            return self
                .outline_stops()
                .first()
                .map_or(TabStep::Leave, |stop| TabStep::To(stop.clone()));
        }
        if self.inspector_owns() {
            let column = next.filter(|stop| stop.is_under(&crate::inspector::panel::inspector_id()));
            let stop = column
                .cloned()
                .or_else(|| self.bar_stops().first().or(self.top_stops().first()).cloned());
            return stop.map_or(TabStep::Leave, TabStep::To);
        }
        TabStep::Send(Message::EnterTabPanel)
    }

    /// Gives the matrix, which Iced draws, the keys, as Ctrl+Tab into the
    /// column does, and the runtime's focus leaves the tab.
    pub(crate) fn enter_tab_panel(&mut self) {
        self.session.matrix.focused = self.right_tab == Tab::Matrix;
        if self.focus_on_tabs() {
            self.effect(loom_ui::Effects::unfocus());
        }
    }

    /// True while the runtime's focus is on one of the tabs.
    pub(crate) fn focus_on_tabs(&self) -> bool {
        self.focus.as_ref().is_some_and(|id| tab_of(id).is_some())
    }
}

/// The row of tabs, as tall as Iced's was (`style::RIGHT_TABS_HEIGHT`).
fn build(panel: &Panel) -> Element {
    let tabs = TABS.iter().map(|(tab, label)| one(panel, *tab, label));
    Element::row(tabs)
        .spacing(4.0)
        .padding(Padding::symmetric(6.0, 8.0))
        .align_y(Align::Center)
        .width(Length::FILL)
        .height(Length::FILL)
}

/// One tab: the open one filled, bordered, and the one focus stop.
fn one(panel: &Panel, tab: Tab, label: &str) -> Element {
    let open = tab == panel.open;
    let looks = panel.looks;
    let text = if open { looks.text } else { looks.quiet };
    let resting = StateLook {
        background: open.then_some(looks.fill),
        text: Some(text),
    };
    let lit = StateLook {
        background: Some(looks.fill),
        text: Some(text),
    };
    let access = Access::new(Role::Tab, label).action(Action::Click).states(States {
        selected: open,
        ..States::default()
    });
    let element = Element::text(label, style::CONTROL_TEXT)
        .padding(Padding::symmetric(4.0, 12.0))
        .radius(style::TAB_RADIUS)
        .border(style::TAB_BORDER, if open { looks.border } else { Color::TRANSPARENT })
        .looks(Looks {
            normal: resting,
            hovered: lit,
            pressed: lit,
            off: resting,
        })
        .key(key(tab))
        .takes_first(TAB_KEYS)
        .access(access);
    if open { element.focusable() } else { element }
}

#[cfg(test)]
mod tests;
