//! MIDI learn in the app: learn mode, the armed parameter, the labels on
//! tiles, and a watcher that wakes the UI when MIDI control changes arrive.
//! Spec: `docs/design/midi-learn.md`.

use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_canvas::LearnView;
use loom_core::{Controller, MidiMapping, MidiReader, ModuleId};
use std::hash::{Hash, Hasher};
use std::time::Duration;

/// How often the watcher looks for new control changes. A hardware knob
/// then moves the knob on screen within about one frame.
pub const INTERVAL: Duration = Duration::from_millis(15);

/// A parameter: module ID and parameter index.
pub type Param = (u32, usize);

/// The armed parameter, and the serial of the last control change when it
/// was armed. Only a newer control change maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Armed {
    pub param: Param,
    pub serial: Option<u32>,
}

/// Watches for control changes. One for the whole app.
#[derive(Clone)]
pub struct Watch {
    pub reader: MidiReader,
    /// Counts the engines the app has built (see `lights::Watch`).
    pub generation: u64,
}

impl Hash for Watch {
    // The reader is the same for one engine, so the subscription restarts
    // only with a new engine.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "midi-learn".hash(state);
        self.generation.hash(state);
    }
}

/// Yields the serial of each new control change, checked every
/// [`INTERVAL`]. Several changes between checks give one message. The thread
/// stops when the subscription ends.
pub fn stream(watch: &Watch) -> impl Stream<Item = u32> + use<> {
    let (mut tx, rx) = mpsc::channel(1);
    let reader = watch.reader.clone();
    std::thread::spawn(move || {
        // Start from "nothing seen", so a control change that arrives while
        // the thread starts is still reported. The app ignores any that came
        // before a parameter was armed.
        let mut last = None;
        while !tx.is_closed() {
            // Control changes and note ons share one serial count.
            let note = reader.last_note_on().map(|seen| seen.serial);
            let now = reader.last_control_change().map(|seen| seen.serial).max(note);
            if let Some(serial) = now.filter(|_| now != last) {
                match tx.try_send(serial) {
                    Ok(()) => last = now,
                    // The UI has not taken the last one yet: it reads fresh
                    // values when it does, so try again next time.
                    Err(e) if e.is_full() => {}
                    Err(_) => break,
                }
            }
            std::thread::sleep(INTERVAL);
        }
    });
    rx
}

/// What the player asks of learn mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// Turn learn mode on or off.
    Toggle,
    /// Arm a parameter, or disarm it if it is armed. Ignored while learn
    /// mode is off.
    Arm(Param),
}

/// Learn mode's state: whether it is on, the armed parameter, and what the
/// canvas shows. The app owns one and reaches it through these methods;
/// mapping a control and the status bar stay with the app, because they
/// touch the controller and undo.
#[derive(Default)]
pub struct Learn {
    on: bool,
    armed: Option<Armed>,
    /// Rebuilt by [`refresh`](Self::refresh) when mappings or the armed
    /// parameter change.
    view: LearnView,
}

impl Learn {
    /// True while learn mode is on.
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Turns learn mode on. Nothing is armed yet.
    pub fn start(&mut self) {
        self.on = true;
    }

    /// Turns learn mode off and disarms.
    pub fn stop(&mut self) {
        self.on = false;
        self.armed = None;
    }

    /// The armed parameter, if any.
    pub fn armed(&self) -> Option<Armed> {
        self.armed
    }

    /// True if `param` is armed.
    pub fn is_armed(&self, param: Param) -> bool {
        self.armed.is_some_and(|armed| armed.param == param)
    }

    /// Arms `param`. Only a control change newer than `serial`, the last
    /// one seen now, maps it.
    pub fn arm(&mut self, param: Param, serial: Option<u32>) {
        self.armed = Some(Armed { param, serial });
    }

    /// Disarms, keeping learn mode on.
    pub fn disarm(&mut self) {
        self.armed = None;
    }

    /// What the canvas shows while learn mode is on.
    pub fn shown(&self) -> Option<&LearnView> {
        self.on.then_some(&self.view)
    }

    /// Rebuilds what the canvas shows, after mappings or the armed parameter
    /// changed. A removed module disarms its parameter.
    pub fn refresh(&mut self, controller: &Controller) {
        if self
            .armed
            .is_some_and(|armed| controller.graph().node(ModuleId(armed.param.0)).is_none())
        {
            self.armed = None;
        }
        self.view = view(controller, self.armed);
    }
}

/// What learn mode shows on the canvas: the armed control and a label on
/// every mapped control.
pub fn view(controller: &Controller, armed: Option<Armed>) -> LearnView {
    let graph = controller.graph();
    let mut view = LearnView::default();
    for (id, node) in graph.nodes() {
        let d = node.descriptor;
        for (widget, w) in d.panel.widgets.iter().enumerate() {
            if !w.is_control() {
                continue;
            }
            let Some(param) = d.param_index(w.bind) else {
                continue;
            };
            if armed.is_some_and(|a| a.param == (id.0, param)) {
                view.armed = Some((id.0, widget));
            }
            if let Some((_, mapping)) = graph.midi_mapping_of(id, param) {
                view.labels
                    .insert((id.0, widget), format!("CC {}", mapping.control.controller));
            }
        }
    }
    view
}

/// A parameter as the status bar names it: "Filter #3 Cutoff".
pub fn param_name(controller: &Controller, (module, param): Param) -> String {
    controller
        .graph()
        .node(ModuleId(module))
        .and_then(|node| {
            if let Some(unknown) = &node.unknown {
                return Some(format!(
                    "{} #{module} {}",
                    unknown.label.as_deref().unwrap_or(&unknown.kind),
                    unknown.parameters.get(param)?
                ));
            }
            let spec = node.descriptor.params.get(param)?;
            Some(format!("{} #{module} {}", node.descriptor.name, spec.label))
        })
        .unwrap_or_else(|| format!("#{module}"))
}

/// The status after a mapping is made. Names the parameter a control left,
/// if it was mapped elsewhere before (spec, Making a mapping).
pub fn mapped_status(controller: &Controller, mapping: MidiMapping, removed: &[MidiMapping]) -> String {
    let target = (mapping.target.0, mapping.param);
    let mut status = format!("Mapped {} to {}", mapping.control, param_name(controller, target));
    if let Some(left) = removed
        .iter()
        .find(|old| old.control == mapping.control && (old.target.0, old.param) != target)
    {
        status.push_str(&format!(
            ". It no longer moves {}",
            param_name(controller, (left.target.0, left.param))
        ));
    }
    status
}

#[cfg(test)]
mod tests;
