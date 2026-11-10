//! Turns a screen reader's requests into the app's messages.
//!
//! No platform code. Every request becomes ordinary messages, the same ones
//! the pointer and the keyboard send, so undo, autosave, MIDI feedback, and
//! the unsaved mark need nothing extra (ui-layer.md, section 7, rule 6).

use super::tree::{STEP, WidgetIds, tile_of};
use crate::Message;
use crate::snapshot::UiSnapshot;
use accesskit::{Action, ActionData, ActionRequest};
use loom_canvas::{CanvasEvent, ControlPart};
use loom_ui_core::IdTable;

/// The canvas events for a screen reader's request, in order. Empty when
/// the request names nothing on screen, or asks for something the target
/// cannot do.
///
/// Click on a tile selects its module, as a pointer click on the tile does.
/// Focus gives no events: focus on a node never changes the patch, so it
/// does not select a tile (ui-layer.md, section 7, rule 9).
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
    let Some((module, index)) = ids.find(id) else {
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
    let set = |position: f32| {
        vec![CanvasEvent::SetControl {
            module,
            widget: index,
            position: position.clamp(0.0, 1.0),
            part: ControlPart::Main,
        }]
    };
    match (request.action, widget.kind) {
        (Action::Increment, "knob" | "fader" | "number") => set(widget.position + STEP),
        (Action::Decrement, "knob" | "fader" | "number") => set(widget.position - STEP),
        (Action::SetValue, "knob" | "fader" | "number") => match request.data {
            Some(ActionData::NumericValue(v)) => set(v as f32),
            _ => Vec::new(),
        },
        (Action::Click, "toggle") => set(if widget.position >= 0.5 { 0.0 } else { 1.0 }),
        (Action::Click, "button") => vec![CanvasEvent::ButtonPressed { module, widget: index }],
        _ => Vec::new(),
    }
}

/// The messages for a screen reader's request, in order.
///
/// A set is one turn of a knob: one change, then let go, so it is one undo
/// step, as the control channel's `set` is.
pub(crate) fn messages(
    snapshot: &UiSnapshot,
    ids: &WidgetIds,
    table: &IdTable,
    request: &ActionRequest,
) -> Vec<Message> {
    let events = canvas_events(snapshot, ids, table, request);
    let turned = events.iter().any(|e| matches!(e, CanvasEvent::SetControl { .. }));
    events
        .into_iter()
        .map(Message::Canvas)
        .chain(turned.then_some(Message::SliderReleased))
        .collect()
}
