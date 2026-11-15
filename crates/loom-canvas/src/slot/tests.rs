//! Tests for the canvas slot.

use super::*;
use crate::{CanvasInput, CanvasModel, FrameCache, Scene};
use loom_ui_core::{Point, Size};
use std::collections::BTreeSet;

/// A frame of an empty scene at `bounds`.
fn drawing(bounds: Rect) -> CanvasDrawing {
    let scene = Scene::new(Vec::new(), Vec::new());
    let none = BTreeSet::new();
    let model = CanvasModel::new(&scene, &none);
    let frame = CanvasInput::default().frame(&model, bounds.size(), &FrameCache::default());
    CanvasDrawing { bounds, frame }
}

/// A slot gives back the frame put in it, once, and nothing after a clear.
#[test]
fn a_slot_gives_its_frame_once() {
    let slot = CanvasSlot::default();
    assert!(slot.take().is_none());
    let first = Rect::new(Point::ORIGIN, Size::new(10.0, 10.0));
    let second = Rect::new(Point::new(1.0, 2.0), Size::new(30.0, 40.0));
    slot.put(drawing(first));
    assert_eq!(slot.take().map(|d| d.bounds), Some(first));
    assert!(slot.take().is_none());
    slot.put(drawing(second));
    slot.clear();
    assert!(slot.take().is_none());
}

/// A second frame put before the slot is emptied is a bug: one canvas
/// would never be shown. Debug builds say so.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "two canvases drew into one slot in a frame")]
fn a_second_frame_in_one_slot_panics() {
    let slot = CanvasSlot::default();
    let bounds = Rect::new(Point::ORIGIN, Size::new(10.0, 10.0));
    slot.put(drawing(bounds));
    slot.put(drawing(bounds));
}
