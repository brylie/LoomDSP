//! Watches the display values behind the lights on screen, off the UI
//! thread, and wakes the UI only when a light visibly changes
//! (`docs/design/panels.md`, Redraw).
//!
//! Every message to the app redraws the window, so a plain timer would draw
//! 30 frames a second even when nothing moves. This watcher reads the
//! engine's display table on its own thread and sends a message only when a
//! light's brightness, or a meter's place in its range, changes by at least
//! one of 256 levels.
//!
//! A trigger light holds its flash: a read of 1.0 lights it at full, and it
//! fades to dark over [`FADE`]. The watcher is the only reader of trigger
//! values, because a read takes them (`docs/design/panels.md`, Lights for
//! one-sample triggers). It sends while a light fades, then nothing.

use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_core::{DisplayKind, DisplayRange, DisplayReader, DisplaySource, ModuleDescriptor, Widget, WidgetKind};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// How often the watcher reads the display table: about 30 times a second.
pub const INTERVAL: Duration = Duration::from_millis(33);

/// The least time between two changes of a readout's text. Text that changes
/// faster cannot be read, and each change lays out text again.
pub const READOUT_INTERVAL: Duration = Duration::from_millis(100);

/// How long a trigger light takes to fade from full to dark.
pub const FADE: Duration = Duration::from_millis(120);

/// A light, by module ID and widget index.
pub type Light = (u32, usize);

/// The lights to watch, and where their values live.
#[derive(Clone)]
pub struct Watch {
    pub reader: DisplayReader,
    pub lights: Vec<(Light, DisplaySource)>,
    /// The range of each entry in `lights` that is a readout (see [`readout`]):
    /// it says how the readout writes its value.
    pub readouts: Vec<Option<DisplayRange>>,
    /// The range of each entry in `lights`, if it has one (see [`range`]). A
    /// meter on a display value may read outside 0 to 1.
    pub ranges: Vec<Option<DisplayRange>>,
    /// Counts the engines the app has built. A new engine numbers its modules
    /// from 1 again, so the same lights can read a different table.
    pub generation: u64,
}

impl Hash for Watch {
    // The subscription restarts when the set of lights changes, or when the
    // engine does.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.generation.hash(state);
        self.lights.hash(state);
    }
}

impl Watch {
    /// Reads every light once, as stored. A trigger light reads 1.0 if it
    /// fired since the last read, with no fade.
    pub fn read(&self) -> Vec<(Light, f32)> {
        self.lights
            .iter()
            .map(|(light, s)| (*light, self.reader.read(*s)))
            .collect()
    }

    /// Reads every light once, as drawn at `now`: a trigger light fades
    /// after its last flash, with `fades` keeping when each one fired.
    pub fn shown(&self, fades: &mut [Fade], now: Instant) -> Vec<(Light, f32)> {
        self.lights
            .iter()
            .zip(fades)
            .map(|((light, s), fade)| {
                let value = self.reader.read(*s);
                let shown = match s.kind() {
                    DisplayKind::Trigger => fade.brightness(value, now),
                    DisplayKind::Level => value,
                };
                (*light, shown)
            })
            .collect()
    }
}

/// What a readout last showed, so its text changes at most 10 times a second
/// and only when it would look different (panel-widgets.md, Readout). The
/// snapshot carries every entry, so a light fading at 30 Hz would otherwise
/// carry fresh readout values with it.
#[derive(Debug, Clone, Default)]
pub struct ReadoutHold {
    value: f32,
    text: String,
    at: Option<Instant>,
}

impl ReadoutHold {
    /// The value to show at `now`, after a read of `reading`: the held one
    /// until the text changes and [`READOUT_INTERVAL`] has passed.
    pub fn held(&mut self, reading: f32, range: &DisplayRange, now: Instant) -> f32 {
        let too_soon = self
            .at
            .is_some_and(|at| now.saturating_duration_since(at) < READOUT_INTERVAL);
        if too_soon {
            return self.value;
        }
        let text = range.text(reading);
        if self.at.is_none() || text != self.text {
            self.value = reading;
            self.text = text;
            self.at = Some(now);
        }
        self.value
    }
}

/// When a trigger light last flashed, so it can fade over [`FADE`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Fade {
    lit_at: Option<Instant>,
}

impl Fade {
    /// Brightness at `now`, after a read of `reading`. A reading above 0
    /// starts a new flash at full. Otherwise the last flash fades
    /// linearly, and is dark from [`FADE`] after it.
    pub fn brightness(&mut self, reading: f32, now: Instant) -> f32 {
        if reading > 0.0 {
            self.lit_at = Some(now);
        }
        let Some(lit_at) = self.lit_at else {
            return 0.0;
        };
        let left = 1.0 - now.saturating_duration_since(lit_at).as_secs_f32() / FADE.as_secs_f32();
        if left <= 0.0 {
            self.lit_at = None;
            0.0
        } else {
            left
        }
    }
}

/// Brightness as drawn: 256 levels.
pub fn level(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The range a change in `widget`'s value is measured in, if it has one: what
/// it shows. A meter on a display value shows its place in the value's range. A
/// light has none, as it draws its value as brightness, whatever the range.
pub fn range(descriptor: &ModuleDescriptor, widget: &Widget) -> Option<DisplayRange> {
    descriptor.meter_display(widget).map(|display| display.range)
}

/// The range that says how `widget` writes its value, if it is a readout.
pub fn readout(descriptor: &ModuleDescriptor, widget: &Widget) -> Option<DisplayRange> {
    matches!(widget.kind, WidgetKind::Readout { .. })
        .then(|| descriptor.displays.iter().find(|display| display.name == widget.bind))
        .flatten()
        .map(|display| display.range)
}

/// A reading as a step that shows. A light has 256 levels. A meter has as many
/// steps across its range, and more past its ends: the bar stops there, but its
/// text still says the value.
fn step(reading: f32, range: Option<DisplayRange>) -> i32 {
    match range {
        Some(range) => ((reading - range.min) / (range.max - range.min) * 255.0).round() as i32,
        None => i32::from(level(reading)),
    }
}

/// The steps of `readings`, if any differs from `last`. A held readout value
/// only changes when its text does, so any change at all counts for it.
pub fn changed(
    last: &[Option<i32>],
    readings: &[f32],
    ranges: &[Option<DisplayRange>],
    readouts: &[Option<DisplayRange>],
) -> Option<Vec<Option<i32>>> {
    let steps: Vec<Option<i32>> = readings
        .iter()
        .zip(ranges)
        .zip(readouts)
        .map(|((&reading, &range), readout)| {
            Some(if readout.is_some() {
                reading.to_bits() as i32
            } else {
                step(reading, range)
            })
        })
        .collect();
    (steps != last).then_some(steps)
}

/// Holds each readout in a snapshot until its text may change. Pure, with `now`
/// passed in, so a test can step time.
pub fn hold_readouts(
    snapshot: &mut [(Light, f32)],
    readouts: &[Option<DisplayRange>],
    holds: &mut [ReadoutHold],
    now: Instant,
) {
    for ((entry, range), hold) in snapshot.iter_mut().zip(readouts).zip(holds) {
        if let Some(range) = range {
            entry.1 = hold.held(entry.1, range, now);
        }
    }
}

/// Reads the lights every [`INTERVAL`] and yields their values when one
/// changes. The first reading is always sent. The thread stops when the
/// subscription ends.
pub fn stream(watch: &Watch) -> impl Stream<Item = Vec<(Light, f32)>> + use<> {
    let (mut tx, rx) = mpsc::channel(1);
    let watch = watch.clone();
    std::thread::spawn(move || {
        let mut last = vec![None; watch.lights.len()];
        let mut fades = vec![Fade::default(); watch.lights.len()];
        let mut holds = vec![ReadoutHold::default(); watch.lights.len()];
        while !tx.is_closed() {
            let now = Instant::now();
            let mut snapshot = watch.shown(&mut fades, now);
            hold_readouts(&mut snapshot, &watch.readouts, &mut holds, now);
            let readings: Vec<f32> = snapshot.iter().map(|&(_, r)| r).collect();
            if let Some(levels) = changed(&last, &readings, &watch.ranges, &watch.readouts) {
                match tx.try_send(snapshot) {
                    Ok(()) => last = levels,
                    // The UI has not taken the last one yet: try again next
                    // time, with fresh values.
                    Err(e) if e.is_full() => {}
                    Err(_) => break,
                }
            }
            std::thread::sleep(INTERVAL);
        }
    });
    rx
}

#[cfg(test)]
mod tests;
