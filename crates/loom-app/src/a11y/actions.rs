//! Turns a screen reader's requests into the app's messages.
//!
//! No platform code. Every request becomes ordinary messages, the same ones
//! the pointer and the keyboard send, so undo, autosave, MIDI feedback, and
//! the unsaved mark need nothing extra (ui-layer.md, section 7, rule 6).

use super::tree::{Piece, WidgetIds, tile_of};
use crate::Message;
use crate::snapshot::UiSnapshot;
use accesskit::{Action, ActionData, ActionRequest};
use loom_canvas::{CanvasEvent, ControlPart, WidgetViewKind, arrow_step};
use loom_ui_core::IdTable;

/// The canvas events for a screen reader's request, in order. Empty when
/// the request names nothing on screen, or asks for something the target
/// cannot do.
///
/// Click on a tile selects its module, as a pointer click on the tile does.
/// Focus gives no events here: the app asks the canvas for it, and the
/// canvas selects the control's tile (ui-layer.md, section 7, rule 20).
pub(super) fn canvas_events(
    snapshot: &UiSnapshot,
    ids: &WidgetIds,
    table: &IdTable,
    request: &ActionRequest,
) -> Vec<CanvasEvent> {
    // The tree may be a step behind the patch: check the widget is still
    // shown before acting on it.
    let Some(id) = table.id(request.target_node.0) else {
        return Vec::new();
    };
    if let Some(module) = tile_of(id) {
        let shown = snapshot.tiles.iter().any(|t| t.id == module);
        return match request.action {
            Action::Click if shown => vec![CanvasEvent::Select(vec![module])],
            _ => Vec::new(),
        };
    }
    let Some((module, index, piece)) = ids.find_piece(id) else {
        return Vec::new();
    };
    let Some(widget) = snapshot
        .tiles
        .iter()
        .find(|t| t.id == module)
        .and_then(|t| t.widgets.get(index))
        .filter(|w| !w.hidden)
    else {
        return Vec::new();
    };
    let Some(detail) = ids.detail(module, index) else {
        return Vec::new();
    };
    let main = widget.position;
    let second = widget.second.as_ref().map_or(0.0, |second| second.position);
    let set = |part: ControlPart, position: f32| {
        vec![CanvasEvent::SetControl {
            module,
            widget: index,
            position,
            part,
        }]
    };
    // One arrow key press on `part`, as the keyboard takes it.
    let step = |part: ControlPart, up: bool| match arrow_step(&detail.kind, part, main, second, up, false) {
        Some((next, _)) if part == ControlPart::Main && next != main => set(part, next),
        Some((_, next)) if part == ControlPart::Second && next != second => set(part, next),
        _ => Vec::new(),
    };
    // The number a screen reader sent, as a position on the control. NaN
    // has no place on a control, and clamping would pass it on.
    let sent = |spec: Option<&loom_core::ParamSpec>| match request.data {
        Some(ActionData::NumericValue(v)) if !v.is_nan() => Some(match spec {
            Some(spec) => spec.normalize(v as f32),
            None => (v as f32).clamp(0.0, 1.0),
        }),
        _ => None,
    };
    let one_number = matches!(
        detail.kind,
        WidgetViewKind::Knob { .. } | WidgetViewKind::Fader { .. } | WidgetViewKind::Number { .. }
    );
    let two_numbers = matches!(detail.kind, WidgetViewKind::XyPad { .. } | WidgetViewKind::Range { .. });
    let range = matches!(detail.kind, WidgetViewKind::Range { .. });
    match (request.action, piece) {
        (Action::Increment | Action::Decrement, Piece::Whole)
            if one_number || matches!(detail.kind, WidgetViewKind::Select { .. }) =>
        {
            step(ControlPart::Main, request.action == Action::Increment)
        }
        (Action::SetValue, Piece::Whole) if one_number => sent(detail.main.as_ref())
            .map(|position| set(ControlPart::Main, position))
            .unwrap_or_default(),
        (Action::Increment | Action::Decrement, Piece::Main | Piece::Second) if two_numbers => {
            let part = if piece == Piece::Main {
                ControlPart::Main
            } else {
                ControlPart::Second
            };
            step(part, request.action == Action::Increment)
        }
        // A range's handles do not pass each other: each stops at the
        // other, from the side it is on now. A typed value can leave low
        // above high, and then low stays above.
        (Action::SetValue, Piece::Main) if two_numbers => sent(detail.main.as_ref())
            .map(|position| {
                set(
                    ControlPart::Main,
                    if range {
                        stop_at(position, main, second, false)
                    } else {
                        position
                    },
                )
            })
            .unwrap_or_default(),
        (Action::SetValue, Piece::Second) if two_numbers => sent(detail.second.as_ref())
            .map(|position| {
                set(
                    ControlPart::Second,
                    if range {
                        stop_at(position, second, main, true)
                    } else {
                        position
                    },
                )
            })
            .unwrap_or_default(),
        (Action::Click, Piece::Whole) if matches!(detail.kind, WidgetViewKind::Toggle) => {
            set(ControlPart::Main, if main >= 0.5 { 0.0 } else { 1.0 })
        }
        (Action::Click, Piece::Whole) if matches!(detail.kind, WidgetViewKind::Button) => {
            vec![CanvasEvent::ButtonPressed { module, widget: index }]
        }
        // A choice's position is the index of its option.
        (Action::Click, Piece::Option(option)) => match &detail.kind {
            WidgetViewKind::Radio { options } | WidgetViewKind::Segmented { options, .. }
                if option < options.len() && option != main.round() as usize =>
            {
                set(ControlPart::Main, option as f32)
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// The control a screen reader's Focus request names: its module and panel
/// index. `None` for a tile, for a widget that only shows a value, for one
/// that is hidden, and for a node that is gone. Focus on one value of an XY
/// pad or a range, or on one option, focuses the control that holds it.
pub(crate) fn focus_target(
    snapshot: &UiSnapshot,
    ids: &WidgetIds,
    table: &IdTable,
    request: &ActionRequest,
) -> Option<(u32, usize)> {
    let id = table.id(request.target_node.0)?;
    let (module, index, _) = ids.find_piece(id)?;
    let shown = snapshot
        .tiles
        .iter()
        .find(|t| t.id == module)
        .and_then(|t| t.widgets.get(index))
        .is_some_and(|w| !w.hidden);
    let control = ids.get(module, index).is_some_and(|own| !ids.is_live(own));
    (shown && control).then_some((module, index))
}

/// A range handle's new position: `sent`, stopped at the other handle. The
/// handle stays on the side of the other that it is on now, at `own`.
/// Where the two are together, low is the one below and high the one
/// above, as the canvas takes them, so each can still move away.
pub(super) fn stop_at(sent: f32, own: f32, other: f32, high: bool) -> f32 {
    let below = if high { own < other } else { own <= other };
    if below { sent.min(other) } else { sent.max(other) }
}

/// True when the request names a tile, a widget, or a piece of one that
/// the window shows now.
pub(crate) fn names_a_part(ids: &WidgetIds, table: &IdTable, request: &ActionRequest) -> bool {
    table
        .id(request.target_node.0)
        .is_some_and(|id| tile_of(id).is_some() || ids.find_piece(id).is_some())
}

/// True for an action that steps or sets a number: SetValue, Increment,
/// or Decrement. These join the screen reader's undo group
/// (ui-layer.md, section 7, rule 6). A Click does not: it is one step of
/// its own.
pub(crate) fn joins_group(action: Action) -> bool {
    matches!(action, Action::SetValue | Action::Increment | Action::Decrement)
}

/// The messages for a screen reader's request, in order.
///
/// An action that joins the undo group gives only its change: the app
/// opens and ends the group around it. Any other change is followed by a
/// release, so it is one undo step, as the control channel's `set` is.
pub(crate) fn messages(
    snapshot: &UiSnapshot,
    ids: &WidgetIds,
    table: &IdTable,
    request: &ActionRequest,
) -> Vec<Message> {
    let events = canvas_events(snapshot, ids, table, request);
    let turned = !joins_group(request.action) && events.iter().any(|e| matches!(e, CanvasEvent::SetControl { .. }));
    events
        .into_iter()
        .map(Message::Canvas)
        .chain(turned.then_some(Message::SliderReleased))
        .collect()
}
