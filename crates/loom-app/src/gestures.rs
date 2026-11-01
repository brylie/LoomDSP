//! Gestures in progress that edit modulation from the inspector and the
//! canvas: the open **Add modulation** list, an output dragged onto an
//! inspector row, an arc drag that changes depth, and a placeholder label
//! being typed. Spec: `docs/design/modulation.md`.
//!
//! Each lives only until its gesture ends. What a finished gesture does to
//! the patch (an assignment, a depth, a label) stays with the app, because
//! it goes through undo.

use iced::Point;
use loom_core::AssignmentId;

/// A parameter: module ID and parameter index.
pub type Param = (u32, usize);

/// An output dragged over the window toward an inspector row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AssignDrag {
    /// The output the drag started from.
    pub from: loom_canvas::PortRef,
    /// Where the drag started, in window coordinates.
    pub start: Point,
    /// Where the pointer is now, or `None` while it is over the canvas.
    pub at: Option<Point>,
    /// The canvas zoom, so the drawn line matches the canvas cables.
    pub zoom: f32,
}

/// The modulation gestures in progress. At most one of each.
#[derive(Debug, Default)]
pub struct Gestures {
    /// The parameter whose **Add modulation** list is open.
    adding: Option<Param>,
    /// The output being dragged toward the inspector.
    assign_drag: Option<AssignDrag>,
    /// The inspector row under that drag, if it can take it.
    drop_row: Option<Param>,
    /// During an arc drag: the assignment it changes, once picked, or
    /// `Some(None)` when none fits and the drag does nothing.
    depth_drag: Option<Option<AssignmentId>>,
    /// Unapplied label text for a placeholder module.
    label_draft: Option<(u32, String)>,
}

impl Gestures {
    /// The parameter whose **Add modulation** list is open.
    pub fn adding(&self) -> Option<Param> {
        self.adding
    }

    /// Opens the **Add modulation** list of `param`, or closes it if it is
    /// open.
    pub fn toggle_adding(&mut self, param: Param) {
        self.adding = (self.adding != Some(param)).then_some(param);
    }

    /// Closes the **Add modulation** list.
    pub fn close_adding(&mut self) {
        self.adding = None;
    }

    /// The output drag in progress.
    pub fn assign_drag(&self) -> Option<AssignDrag> {
        self.assign_drag
    }

    /// Starts or moves an output drag.
    pub fn drag(&mut self, drag: AssignDrag) {
        self.assign_drag = Some(drag);
    }

    /// The inspector row under the output drag.
    pub fn drop_row(&self) -> Option<Param> {
        self.drop_row
    }

    /// Notes the inspector row under the pointer. It counts only during an
    /// output drag, and only when `allowed` (not in MIDI learn mode).
    pub fn hover_row(&mut self, row: Option<Param>, allowed: bool) {
        self.drop_row = if self.assign_drag.is_some() && allowed {
            row
        } else {
            None
        };
    }

    /// Ends the output drag from `from`. Returns the row to assign to when
    /// it `landed` on one: the drag started from this output and a row was
    /// under it. The drag ends either way.
    pub fn finish_drag(&mut self, from: loom_canvas::PortRef, landed: bool) -> Option<Param> {
        let ours = self.assign_drag.is_some_and(|drag| drag.from == from);
        let row = self.drop_row.filter(|_| landed && ours);
        self.cancel_drag();
        row
    }

    /// Ends the output drag without assigning.
    pub fn cancel_drag(&mut self) {
        self.assign_drag = None;
        self.drop_row = None;
    }

    /// The assignment an arc drag changes, once picked.
    pub fn depth_target(&self) -> Option<AssignmentId> {
        self.depth_drag.flatten()
    }

    /// True during an arc drag, picked or not, for tests.
    #[cfg(test)]
    pub fn depth_dragging(&self) -> bool {
        self.depth_drag.is_some()
    }

    /// The assignment this arc drag changes. The first step picks it with
    /// `pick`; the rest of the drag keeps that choice, even `None`.
    pub fn pick_depth(&mut self, pick: impl FnOnce() -> Option<AssignmentId>) -> Option<AssignmentId> {
        *self.depth_drag.get_or_insert_with(pick)
    }

    /// Shows an arc drag on `target`, for a screenshot of one.
    pub fn show_depth(&mut self, target: Option<AssignmentId>) {
        self.depth_drag = Some(target);
    }

    /// Ends an arc drag. Returns true if one was in progress.
    pub fn end_depth(&mut self) -> bool {
        self.depth_drag.take().is_some()
    }

    /// Notes the label typed for placeholder `module`, not yet applied.
    pub fn draft_label(&mut self, module: u32, value: String) {
        self.label_draft = Some((module, value));
    }

    /// The label being typed for placeholder `module`, if any.
    pub fn label_draft(&self, module: u32) -> Option<&str> {
        self.label_draft
            .as_ref()
            .filter(|(id, _)| *id == module)
            .map(|(_, value)| value.as_str())
    }

    /// Takes the label typed for `module`, if it is the one being typed.
    /// Any other draft is dropped.
    pub fn take_label(&mut self, module: u32) -> Option<String> {
        self.label_draft
            .take()
            .filter(|(id, _)| *id == module)
            .map(|(_, value)| value)
    }

    /// Drops an unapplied label, as undo does.
    pub fn clear_label(&mut self) {
        self.label_draft = None;
    }
}

#[cfg(test)]
mod tests;
