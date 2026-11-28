//! The window's layers on `loom-ui`'s shell: the app draws every frame
//! itself (ui-layer.md, section 9, rule 8; plan for leaving Iced, step P4).
//!
//! The shell owns the window, the GPU, the runtime, and the redraw rule.
//! These layers draw a frame through [`WindowPainter`] (`drawing.rs`), the
//! same path a capture takes, and hand the shell the control channel's
//! scripted input, one event a report, as the keyboard and the pointer
//! give theirs (`pointer.rs`).

use crate::App;
use crate::drawing::{FrameTarget, WindowPainter};
use loom_ui::paint::Typesetter;
use loom_ui::shell::{Frame, Gate, Layers, Parts, Redraw, Report};
use std::cell::RefCell;

/// The routing target with no toolkit behind it: the window's every part
/// is the runtime's (ui-layer.md, section 2, the end of the bridge). Every
/// event passes on to the runtime's own layers, and a focus or scroll the
/// runtime hands it names no element the window has, so it is dropped, as
/// a focus effect on a missing element is (section 6, rule 2). P5 removes
/// the target from `loom-ui` altogether.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct NoToolkit;

impl loom_ui::Target<crate::Message> for NoToolkit {
    fn input(
        &mut self,
        _event: &loom_ui_core::Event,
        _pointer: Option<loom_ui_core::Point>,
        _now: std::time::Instant,
        _out: &mut Vec<crate::Message>,
    ) -> loom_ui::Routed {
        loom_ui::Routed::Passed
    }

    fn focus(&mut self, _target: &loom_ui::effect::FocusTarget, _select: loom_ui::effect::Selection) {}

    fn scroll_into_view(&mut self, _id: &loom_ui_core::UiId) {}

    fn focused(&self) -> Option<loom_ui_core::UiId> {
        None
    }

    fn needs_redraw(&mut self, _updates: usize) -> bool {
        false
    }
}

/// The app's layers in the shell's window.
pub(crate) struct AppLayers {
    /// The renderers, made at the first frame for its format, and again if
    /// a frame's format differs.
    painter: Option<WindowPainter>,
    /// The text the panels are measured and drawn with: one typesetter for
    /// both, so what is measured is what is drawn.
    text: RefCell<Typesetter>,
    /// A scripted step went in and no frame has been drawn since: the next
    /// waits for one, so each step is drawn before the next (control
    /// channel, Pointer commands).
    stepped: bool,
}

impl AppLayers {
    /// Layers with nothing drawn yet.
    pub(crate) fn new() -> Self {
        Self {
            painter: None,
            text: RefCell::new(Typesetter::new()),
            stepped: false,
        }
    }
}

impl AppLayers {
    /// A frame was drawn: one the window shows lets the next scripted step
    /// go in; a capture, drawn into its own texture, does not.
    fn drawn(&mut self, capture: bool) {
        if !capture {
            self.stepped = false;
        }
    }
}

impl Layers<App, NoToolkit> for AppLayers {
    /// Draws a whole frame, for the window or for a capture: a capture is
    /// the frame the window shows, drawn again into its own texture.
    fn draw(&mut self, mut parts: Parts<'_, App, NoToolkit>, frame: &Frame<'_>) {
        if self
            .painter
            .as_ref()
            .is_none_or(|painter| painter.format() != frame.format)
        {
            self.painter = Some(WindowPainter::new(frame.device, frame.format));
        }
        let Some(painter) = self.painter.as_mut() else { return };
        let target = FrameTarget {
            device: frame.device,
            queue: frame.queue,
            view: frame.view,
            format: frame.format,
            size: frame.size,
            scale: frame.scale,
            capture: frame.capture,
        };
        painter.draw(&mut parts.panels, &self.text, &target);
        self.drawn(frame.capture);
    }

    /// The control channel's scripted input, one step a report, each after
    /// the last one's frame, as a pointer and a keyboard give theirs
    /// (`pointer.rs`). A scripted press outside the cable jump list closes
    /// it first, as a real one does.
    fn report(&mut self, parts: Parts<'_, App, NoToolkit>) -> Report {
        let input = if self.stepped {
            Vec::new()
        } else {
            parts.app.next_scripted(&parts.panels)
        };
        self.stepped |= !input.is_empty();
        for step in &input {
            parts
                .waker
                .send_all(parts.app.press_messages(&parts.panels, &step.event, step.at));
        }
        Report {
            redraw: if parts.app.pointer.waiting() || !input.is_empty() {
                Redraw::NextFrame
            } else {
                Redraw::No
            },
            input,
            ..Report::default()
        }
    }

    /// A real pointer takes over from the script (`Pointer::gate`), and a
    /// real press outside the cable jump list closes it, before the press
    /// goes on. What the press sends goes only once it passes, so it comes
    /// after the input the script held, which the shell runs first.
    fn pointer_gate(
        &mut self,
        parts: Parts<'_, App, NoToolkit>,
        event: &loom_ui_core::Event,
        pointer: Option<loom_ui_core::Point>,
    ) -> Gate {
        let gate = parts.app.pointer.gate(event);
        if gate == Gate::Pass {
            parts
                .waker
                .send_all(parts.app.press_messages(&parts.panels, event, pointer));
        }
        gate
    }
}

#[cfg(test)]
mod tests;
