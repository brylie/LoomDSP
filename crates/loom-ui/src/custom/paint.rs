//! Painting custom elements in a panel's paint walk, which names no app
//! (ui-layer.md, section 9, rule 4). Each is painted at its place in the
//! paint order, after its own box and before what comes after it, cut to
//! its bounds and to every clip around it. Painting reads the app and
//! sends nothing, since a frame runs no update.

use super::{CustomKind, Customs};
use crate::app::App;
use crate::paint::TextPainter;
use loom_ui_core::{Rect, UiId};

/// What paints the custom elements in a panel's paint walk, which names no
/// app: the runtime's kept states, a still picture's fresh ones, or none.
pub(crate) trait PaintCustoms {
    /// Paints the custom element `id`, of `kind`, at `bounds` in the window.
    fn paint(&mut self, id: &UiId, kind: CustomKind, bounds: Rect, painter: &mut dyn TextPainter);
}

/// Paints no custom element, for a part with none, as hover text.
pub(crate) struct NoCustoms;

impl PaintCustoms for NoCustoms {
    fn paint(&mut self, _id: &UiId, _kind: CustomKind, _bounds: Rect, _painter: &mut dyn TextPainter) {}
}

/// Paints the custom elements from the states the runtime keeps.
pub(crate) struct Kept<'a, A: App> {
    /// The states.
    pub(crate) customs: &'a Customs<A>,
    /// The app they read.
    pub(crate) app: &'a A,
}

impl<A: App> PaintCustoms for Kept<'_, A> {
    fn paint(&mut self, id: &UiId, _kind: CustomKind, bounds: Rect, painter: &mut dyn TextPainter) {
        if let Some(state) = self.customs.state(id) {
            state.paint(self.app, bounds, painter);
        }
    }
}

/// Paints the custom elements from states made for this one picture, for
/// a still picture of the window with no runtime around it
/// ([`Still`](crate::Still)): each as the app makes it new.
pub(crate) struct Fresh<'a, A: App> {
    /// The app that makes and is read by them.
    pub(crate) app: &'a A,
}

impl<A: App> PaintCustoms for Fresh<'_, A> {
    fn paint(&mut self, id: &UiId, kind: CustomKind, bounds: Rect, painter: &mut dyn TextPainter) {
        if let Some(state) = self.app.make_custom(id, kind) {
            state.paint(self.app, bounds, painter);
        }
    }
}

#[cfg(test)]
mod tests;
