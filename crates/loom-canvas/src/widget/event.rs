//! The gestures the canvas reports to the application.

use super::*;

/// A gesture on the canvas. The application applies it to its model.
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasEvent {
    /// The pointer pressed on the canvas while matrix keys had focus.
    PointerDown,
    /// Replace the selection.
    Select(Vec<u32>),
    /// Move modules by a world offset (always whole grid steps).
    Move {
        /// Modules to move.
        ids: Vec<u32>,
        /// Offset in world units.
        delta: Vector,
    },
    /// Set a growable tile's dimensions in grid units.
    Resize {
        /// Module to resize.
        module: u32,
        /// Width and height in grid units.
        size: (u8, u8),
    },
    /// Connect an output to an input (replacing any cable into that input).
    Connect {
        /// The output.
        from: PortRef,
        /// The input.
        to: PortRef,
    },
    /// Remove the cable into this input.
    Disconnect(PortRef),
    /// The range arc of a knob was dragged: change the depth of one of its
    /// assignments by `delta`, as a share of the knob from -1 to 1
    /// (`docs/design/modulation.md`, Arc drag). The app picks the
    /// assignment, from `pinned` first. Sent between a
    /// [`CanvasEvent::GestureStart`] and [`CanvasEvent::GestureEnd`].
    DragDepth {
        /// Module ID of the knob.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
        /// The output whose lines are pinned, if any.
        pinned: Option<PortRef>,
        /// Change of depth since the last event.
        delta: f32,
    },
    /// An output was dropped on a control that takes modulation: assign the
    /// output to the control's parameter (`docs/design/modulation.md`,
    /// Making an assignment). Not sent in MIDI learn mode.
    Assign {
        /// The output.
        from: PortRef,
        /// Module ID of the control.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// An output is being dragged at window coordinates. `None` means the
    /// pointer left the window, so a later release cannot make an assignment.
    AssignDrag {
        from: PortRef,
        start: Point,
        at: Option<Point>,
        zoom: f32,
    },
    /// An output was released away from a canvas target. The app may assign
    /// it to the inspector row under the pointer.
    AssignDrop { from: PortRef, at: Option<Point> },
    /// The output drag ended without an inspector drop.
    AssignCancel,
    /// Delete modules.
    Delete(Vec<u32>),
    /// Set a control on a module's panel. `position` is the knob position
    /// from 0 to 1, 0 or 1 for a toggle, or the option index for radio
    /// buttons. A button reports [`CanvasEvent::ButtonPressed`] instead.
    SetControl {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
        /// The new position.
        position: f32,
    },
    /// A button was pressed, by the pointer or by Space or Enter.
    ///
    /// The app decides the parameter value from the button's mode. The
    /// canvas does not.
    ButtonPressed {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// A button hold ended.
    ///
    /// Pointer up sends this, including after the pointer has left the
    /// button. So does another mouse button, key up, and the window losing
    /// focus. The app ignores it for a toggle and clears a momentary gate.
    ButtonReleased {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// An arrow key on a focused button. `high` is up or right.
    ButtonArrow {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
        /// Up or right. Down or left is false.
        high: bool,
    },
    /// Reset a control to its parameter's default (double-click).
    ResetControl {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// Open the module browser (`docs/design/module-browser.md`): `A` was
    /// pressed with the pointer over the canvas, empty canvas was
    /// double-clicked, or the browser key changed.
    OpenBrowser {
        /// Where a new module goes, in world units, not yet snapped.
        world: Point,
        /// Where the browser opens, in window coordinates.
        at: Point,
        /// The canvas area, in window coordinates, so the browser can stay
        /// inside it.
        area: Rectangle,
    },
    /// An output or a cable was right-clicked without moving: open the menu
    /// with Add scope and Add meter (`docs/design/panels.md`, Add scope and
    /// Add meter).
    ProbeMenu {
        /// The output to probe. On a cable, its source.
        output: PortRef,
        /// Where the menu was opened, in world units: the output port, or
        /// the point clicked on the cable.
        world: Point,
        /// Where the menu opens, in window coordinates.
        at: Point,
        /// The canvas area, in window coordinates, so the menu can stay
        /// inside it.
        area: Rectangle,
    },
    /// A tile was right-clicked without moving.
    TileMenu {
        /// The module under the pointer.
        module: u32,
        /// Menu position in window coordinates.
        at: Point,
        /// Canvas area for positioning the menu.
        area: Rectangle,
    },
    /// Ctrl+V or Cmd+V: paste the clipboard (`docs/design/clipboard.md`).
    /// `world` is the pointer when it is over the canvas, and the centre of
    /// the view when it is not, in world units, not yet snapped.
    Paste {
        /// Where the top-left corner of the pasted modules goes.
        world: Point,
    },
    /// The modules whose lights or meters are on screen, at a zoom where
    /// they are drawn, changed. Only these need their values read.
    VisibleDisplays(Vec<u32>),
    /// Scopes on screen now draw their full traces (true, close up) or a
    /// flat line for their level (false, further out). The app then reads
    /// only what is drawn (`docs/design/panels.md`, Drawing the scope trace).
    TraceDetail(bool),
    /// In MIDI learn mode, a control was clicked: arm it, or disarm it if it
    /// is armed. Its value does not change (`docs/design/midi-learn.md`).
    Learn {
        /// Module ID.
        module: u32,
        /// Index in the module's panel widgets.
        widget: usize,
    },
    /// Space was pressed without a control that uses it (including a focused
    /// knob). The app toggles transport Run. Enter does not send this.
    ToggleRun,
    /// A drag began: turning a knob, moving tiles, or pulling a cable. Every
    /// edit until [`CanvasEvent::GestureEnd`] is one undo step
    /// (`docs/design/undo.md`, Gestures). Arrow edits end with
    /// [`CanvasEvent::ArrowGroupEnd`] after their grouping window.
    GestureStart,
    /// The gesture ended. It is always sent after a start, also when the
    /// pointer is released outside the window or the window loses focus.
    GestureEnd,
    /// The arrow key was released; another quick press may share its step.
    ArrowGroupPending,
    /// An arrow key was pressed again before the grouping deadline.
    ArrowGroupResumed,
    /// The arrow group ended, including when another interaction replaces it.
    ArrowGroupEnd,
}
