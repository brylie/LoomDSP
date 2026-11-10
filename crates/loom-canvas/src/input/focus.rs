//! Which control has the keyboard, and what the keys do there.
//!
//! Keyboard focus is one value: nothing, or one control in one mode. A
//! control is browsed (the arrows change its value), inside a range with a
//! part picked (panel-widgets.md, rule 4), showing its open choice list, or
//! taking a typed value. One control cannot be in two modes, and a mode
//! cannot belong to a control other than the focused one. A new widget with
//! its own keyboard adds a [`FocusMode`] variant.
//!
//! The scene and the selection come from the app, so a focused control can
//! still vanish, be hidden by a mode, or lose its selection between two
//! events. [`CanvasModel::reconcile_focus`] brings a mode that no longer
//! fits back to browsing, on every event. Plain focus is checked only when a
//! key arrives, in `focus_keys`, so reselecting a module before any key
//! press keeps its focus.

use super::select_popup::select_rows;
use super::typed_field::TypedField;
use super::*;

/// Keyboard focus on the canvas.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) enum FocusState {
    /// No control has the keys.
    #[default]
    None,
    /// One control has the keys.
    Control(FocusedControl),
}

/// The focused control and what the keys do on it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FocusedControl {
    /// The module's ID.
    pub(super) module: u32,
    /// The widget's index in the module's panel.
    pub(super) widget: usize,
    /// True when Tab or Enter gave the focus, so the focus ring and hover
    /// text show. A click also focuses a control, but the pointer is on it
    /// then.
    pub(super) via_tab: bool,
    /// What the keys do on it.
    pub(super) mode: FocusMode,
}

/// What the keys do on the focused control.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum FocusMode {
    /// The arrows change the value.
    Browse,
    /// The keyboard is inside a range, with a part picked (panel-widgets.md,
    /// rule 4).
    RangePart(RangePart),
    /// The control's choice list is open.
    Popup(SelectPopup),
    /// A field for a typed value is open over the control.
    Typing(TypedField),
}

/// The part of a range the keyboard has picked, inside it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RangePart {
    /// 0 is the low handle, 1 the band, and 2 the high handle.
    pub(super) part: u8,
    /// How the arrows change the part while it is grabbed.
    pub(super) grab: Option<RangeGrab>,
}

impl FocusState {
    /// Focus on one control in one mode.
    pub(super) fn at(module: u32, widget: usize, via_tab: bool, mode: FocusMode) -> Self {
        Self::Control(FocusedControl {
            module,
            widget,
            via_tab,
            mode,
        })
    }

    /// The focused control: module ID and widget index.
    pub(super) fn control(&self) -> Option<(u32, usize)> {
        match self {
            Self::Control(c) => Some((c.module, c.widget)),
            Self::None => None,
        }
    }

    /// The focused control, when Tab or Enter gave it the focus.
    pub(super) fn tab_control(&self) -> Option<(u32, usize)> {
        match self {
            Self::Control(c) if c.via_tab => Some((c.module, c.widget)),
            _ => None,
        }
    }

    /// Focuses a control, browsing.
    pub(super) fn set(&mut self, module: u32, widget: usize, via_tab: bool) {
        *self = Self::Control(FocusedControl {
            module,
            widget,
            via_tab,
            mode: FocusMode::Browse,
        });
    }

    /// Takes the focus from every control.
    pub(super) fn clear(&mut self) {
        *self = Self::None;
    }

    /// What the keys do on the focused control.
    pub(super) fn mode(&self) -> Option<&FocusMode> {
        match self {
            Self::Control(c) => Some(&c.mode),
            Self::None => None,
        }
    }

    /// A pointer press on the canvas: the focus is now the pointer's, and
    /// the keyboard leaves a range's inside and lets go of its grab
    /// (panel-widgets.md, rule 4). An open list or field has already taken
    /// or closed on the press.
    pub(super) fn pressed(&mut self) {
        if let Self::Control(c) = self {
            c.via_tab = false;
            if matches!(c.mode, FocusMode::RangePart(_)) {
                c.mode = FocusMode::Browse;
            }
        }
    }

    /// The picked part, while the keyboard is inside a range.
    pub(super) fn range_part(&self) -> Option<RangePart> {
        match self.mode() {
            Some(FocusMode::RangePart(part)) => Some(*part),
            _ => None,
        }
    }

    /// Goes inside the focused range with this part picked, or with `None`
    /// leaves it. Nothing changes with no focus, or with a list or field
    /// open.
    pub(super) fn set_range_part(&mut self, part: Option<RangePart>) {
        if let Self::Control(c) = self
            && matches!(c.mode, FocusMode::Browse | FocusMode::RangePart(_))
        {
            c.mode = part.map_or(FocusMode::Browse, FocusMode::RangePart);
        }
    }

    /// The open choice list.
    pub(super) fn popup(&self) -> Option<&SelectPopup> {
        match self.mode() {
            Some(FocusMode::Popup(popup)) => Some(popup),
            _ => None,
        }
    }

    /// The open choice list, to change.
    pub(super) fn popup_mut(&mut self) -> Option<&mut SelectPopup> {
        match self {
            Self::Control(FocusedControl {
                mode: FocusMode::Popup(popup),
                ..
            }) => Some(popup),
            _ => None,
        }
    }

    /// Opens a control's choice list. The control takes the focus, and keeps
    /// how it got it if it had it already.
    pub(super) fn open_popup(&mut self, module: u32, widget: usize, popup: SelectPopup) {
        let via_tab = matches!(self, Self::Control(c) if (c.module, c.widget) == (module, widget) && c.via_tab);
        *self = Self::Control(FocusedControl {
            module,
            widget,
            via_tab,
            mode: FocusMode::Popup(popup),
        });
    }

    /// Closes the open choice list, if one is open. The control keeps the
    /// focus.
    pub(super) fn close_popup(&mut self) {
        if let Self::Control(c) = self
            && matches!(c.mode, FocusMode::Popup(_))
        {
            c.mode = FocusMode::Browse;
        }
    }

    /// The open field for a typed value.
    pub(super) fn typed(&self) -> Option<&TypedField> {
        match self.mode() {
            Some(FocusMode::Typing(field)) => Some(field),
            _ => None,
        }
    }

    /// Opens a field for a typed value on the focused control. Nothing
    /// happens with no focus.
    pub(super) fn open_typing(&mut self, field: TypedField) {
        if let Self::Control(c) = self {
            c.mode = FocusMode::Typing(field);
        }
    }

    /// Takes the open field out, leaving the control browsed. Put it back
    /// with [`FocusState::open_typing`] to keep it open.
    pub(super) fn take_typing(&mut self) -> Option<TypedField> {
        let Self::Control(c) = self else {
            return None;
        };
        match std::mem::replace(&mut c.mode, FocusMode::Browse) {
            FocusMode::Typing(field) => Some(field),
            other => {
                c.mode = other;
                None
            }
        }
    }

    /// Closes the open field, if one is open, with no change. The control
    /// keeps the focus.
    pub(super) fn close_typing(&mut self) {
        let _ = self.take_typing();
    }

    /// True while a choice list or a field is open over the canvas. Hover
    /// text waits for it to close.
    pub(super) fn has_overlay(&self) -> bool {
        matches!(self.mode(), Some(FocusMode::Popup(_) | FocusMode::Typing(_)))
    }
}

impl CanvasModel<'_> {
    /// Brings a mode that no longer fits its control back to browsing. This
    /// runs on every event, redraws too, so a control hidden and shown again
    /// between two key presses does not keep its list, field, or grab.
    ///
    /// - Inside a range ends when the range has gone or been hidden, or its
    ///   module is no longer the one selected (panel-widgets.md, rule 4).
    /// - A choice list closes when its control has gone or been hidden. The
    ///   event that found it goes on to normal handling, so a stale list
    ///   never swallows it. A canvas that has grown can show more rows, so
    ///   the scroll comes back too, or the list would end in blank rows that
    ///   pick a wrong value.
    /// - A typed field closes, with no change, when its control has gone or
    ///   been hidden, its module is no longer the one selected, or MIDI
    ///   learn starts.
    pub(super) fn reconcile_focus(&self, state: &mut CanvasInput) {
        let height = state.size.height;
        let FocusState::Control(focused) = &mut state.focus else {
            return;
        };
        let widget = self
            .scene
            .module(focused.module)
            .and_then(|module| module.panel.widgets.get(focused.widget))
            .filter(|widget| !widget.hidden);
        let selected = self.selection.len() == 1 && self.selection.contains(&focused.module);
        let fits = match &focused.mode {
            FocusMode::Browse => true,
            FocusMode::RangePart(_) => {
                selected && widget.is_some_and(|widget| matches!(widget.kind, WidgetViewKind::Range { .. }))
            }
            FocusMode::Popup(_) => widget.is_some_and(|widget| matches!(widget.kind, WidgetViewKind::Select { .. })),
            FocusMode::Typing(_) => {
                self.learn.is_none() && selected && widget.is_some_and(|widget| widget.number_steps().is_some())
            }
        };
        if !fits {
            focused.mode = FocusMode::Browse;
        }
        if let FocusMode::Popup(popup) = &mut focused.mode {
            let count = self
                .select_options(focused.module, focused.widget)
                .map_or(0, <[String]>::len);
            popup.scroll = popup.scroll.min(count.saturating_sub(select_rows(height, count)));
        }
    }
}

#[cfg(test)]
mod tests;
