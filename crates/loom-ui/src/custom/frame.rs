//! A frame's plan: the runtime's painting, cut at each custom element that
//! draws with the GPU (ui-layer.md, section 9, rule 4, draw order is paint
//! order).
//!
//! A frame draws what was painted before an element, then the element's
//! GPU drawing, then its own painting and everything after it. So a
//! parent's background is under the element and a sibling painted later
//! is over it. The plan is plain data with no GPU in it: a driver runs its
//! steps, and a test reads them.

use crate::app::App;
use crate::draw::Panels;
use crate::paint::{Mark, PaintList};
use crate::view::Layer;
use loom_ui_core::{Rect, UiId};
use std::ops::Range;

/// The steps of one frame's drawing, in order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FramePlan {
    steps: Vec<FrameStep>,
}

impl FramePlan {
    /// The steps, first to last.
    pub fn steps(&self) -> &[FrameStep] {
        &self.steps
    }
}

/// One step of a frame's drawing.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum FrameStep {
    /// Draw these layers of the paint list
    /// ([`PaintRenderer::draw_range`](crate::paint::PaintRenderer::draw_range)).
    Paint(Range<usize>),
    /// The custom element `id` draws with the GPU: `bounds` is where it is
    /// in the window, and `clip` what of it shows there, both in logical
    /// pixels.
    Custom {
        /// The element's UI ID.
        id: UiId,
        /// Where it is in the window.
        bounds: Rect,
        /// What of it shows: its bounds cut by every clip around it.
        clip: Rect,
    },
    /// The custom element `id` draws a second time, above the panels of
    /// `layer`, cut to the window only.
    Above {
        /// The element's UI ID.
        id: UiId,
        /// The layer it draws above.
        layer: Layer,
    },
}

impl<A: App> Panels<'_, A> {
    /// The plan for drawing `list`, painted from the runtime's layers: its
    /// layers in order, cut at each custom element that draws with the GPU
    /// and shows, and at the end of each window layer an element draws
    /// above.
    pub fn frame_plan(&self, list: &PaintList) -> FramePlan {
        let mut steps = Vec::new();
        let mut from = 0;
        let cut = |steps: &mut Vec<FrameStep>, at: usize, from: &mut usize| {
            if *from < at {
                steps.push(FrameStep::Paint(*from..at));
            }
            *from = at;
        };
        // Elements in paint order, for what they draw above a layer.
        let painted: Vec<&UiId> = list
            .marks()
            .iter()
            .filter_map(|(_, mark)| match mark {
                Mark::Custom(id) => Some(id),
                Mark::LayerEnd(_) => None,
            })
            .collect();
        for (at, mark) in list.marks() {
            match mark {
                Mark::Custom(id) if self.customs.uses_gpu(id) => {
                    let (Some(clip), Some(bounds)) = (list.layer_clip(*at), self.customs.bounds(id)) else {
                        continue;
                    };
                    // Nothing of it shows: it draws nothing.
                    if clip.width <= 0.0 || clip.height <= 0.0 {
                        continue;
                    }
                    cut(&mut steps, *at, &mut from);
                    steps.push(FrameStep::Custom {
                        id: id.clone(),
                        bounds,
                        clip,
                    });
                }
                Mark::Custom(_) => {}
                Mark::LayerEnd(layer) => {
                    let above: Vec<&UiId> = painted
                        .iter()
                        .copied()
                        .filter(|id| self.customs.above(self.app, id, *layer))
                        .collect();
                    if above.is_empty() {
                        continue;
                    }
                    cut(&mut steps, *at, &mut from);
                    for id in above {
                        steps.push(FrameStep::Above {
                            id: id.clone(),
                            layer: *layer,
                        });
                    }
                }
            }
        }
        cut(&mut steps, list.layer_count(), &mut from);
        FramePlan { steps }
    }
}

// The tests use an element that draws with the GPU.
#[cfg(all(test, feature = "gpu"))]
mod tests;
