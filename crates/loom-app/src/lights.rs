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
//!
//! Which lights are on screen is a list the app sets after each update
//! ([`Watch`]). The thread follows the list and keeps each light's state by
//! light ([`Tracker`]), so a pan or zoom neither restarts it nor disturbs a
//! light that stays (`docs/design/panels.md`, Redraw, A pan or zoom does not
//! disturb a light).

use crate::watcher::{Outlet, Watched};
use loom_core::{DisplayKind, DisplayRange, DisplayReader, DisplaySource, ModuleDescriptor, Widget, WidgetKind};
use std::collections::HashMap;
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

/// One light on screen, and where its value lives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Entry {
    pub light: Light,
    pub source: DisplaySource,
    /// The range a change is measured in, if it has one (see [`range`]). A
    /// meter on a display value may read outside 0 to 1.
    pub range: Option<DisplayRange>,
    /// The range that says how the light writes its value, if it is a
    /// readout (see [`readout`]).
    pub readout: Option<DisplayRange>,
}

/// The lights on screen, with the reader of the engine whose table they
/// read: what `App::watched_lights` builds.
#[derive(Clone)]
pub struct OnScreen {
    pub reader: DisplayReader,
    pub lights: Vec<Entry>,
}

impl OnScreen {
    /// Reads every light once, as stored, with no thread: for `--frames`. A
    /// trigger light reads 1.0 if it fired since the last read, with no fade,
    /// so a trigger that fired during the run shows lit.
    pub fn read(&self) -> Vec<(Light, f32)> {
        self.lights
            .iter()
            .map(|entry| (entry.light, self.reader.read(entry.source)))
            .collect()
    }
}

/// What the light thread needs for its whole life: the engine's reader, the
/// engine's generation, and the shared list of lights on screen, which the
/// app sets after each update. A pan or zoom changes the list, not the
/// watch, so it starts no thread (`docs/design/panels.md`, Redraw, At most
/// one thread per engine). The list travels with the reader, so it is never
/// read with another engine's reader.
#[derive(Clone)]
pub struct Watch {
    pub reader: DisplayReader,
    /// The lights on screen, as the app last set them.
    pub list: Watched<Vec<Entry>>,
    /// Counts the engines the app has built. A new engine numbers its modules
    /// from 1 again, so the same lights can read a different table.
    pub generation: u64,
}

impl Hash for Watch {
    // The watcher restarts only with a new engine. Which lights are on
    // screen is in the list, which the running thread follows.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.generation.hash(state);
    }
}

/// What the light thread keeps for one light.
#[derive(Debug, Clone)]
struct Kept {
    /// Where the value was read: a new source starts the light again.
    source: DisplaySource,
    /// The step last sent, if any.
    last: Option<i32>,
    fade: Fade,
    hold: ReadoutHold,
}

/// Each light's state on the light thread, kept by light, not by its place
/// in the list: its last step sent, its fade, and its readout hold. Pure,
/// with the time passed in, so a test can step it.
#[derive(Debug, Default)]
pub struct Tracker {
    kept: HashMap<Light, Kept>,
}

/// What one read shows, when it differs from what was sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// Every light on the list, as drawn.
    pub snapshot: Vec<(Light, f32)>,
    /// The step each one shows, in the same order.
    steps: Vec<Option<i32>>,
}

impl Tracker {
    /// Takes one read of `lights` at `now`, `readings` in the list's order,
    /// and gives what to send if any light shows a different step. A light
    /// that left the list is forgotten, and so is one whose source changed.
    /// A light new to the list has no last step, so its first reading is
    /// sent. A trigger light new to the list throws its first read away and
    /// starts dark: the latch holds any trigger since it was last read,
    /// however long ago.
    pub fn read(&mut self, lights: &[Entry], readings: &[f32], now: Instant) -> Option<Change> {
        let listed: HashMap<Light, DisplaySource> = lights.iter().map(|entry| (entry.light, entry.source)).collect();
        self.kept.retain(|light, kept| listed.get(light) == Some(&kept.source));
        let mut values = Vec::with_capacity(lights.len());
        let mut last = Vec::with_capacity(lights.len());
        for (entry, &reading) in lights.iter().zip(readings) {
            let new = !self.kept.contains_key(&entry.light);
            let kept = self.kept.entry(entry.light).or_insert_with(|| Kept {
                source: entry.source,
                last: None,
                fade: Fade::default(),
                hold: ReadoutHold::default(),
            });
            let mut value = match entry.source.kind() {
                DisplayKind::Trigger => kept.fade.brightness(if new { 0.0 } else { reading }, now),
                DisplayKind::Level => reading,
            };
            if let Some(range) = &entry.readout {
                value = kept.hold.held(value, range, now);
            }
            values.push(value);
            last.push(kept.last);
        }
        let ranges: Vec<_> = lights.iter().map(|entry| entry.range).collect();
        let readouts: Vec<_> = lights.iter().map(|entry| entry.readout).collect();
        let steps = changed(&last, &values, &ranges, &readouts)?;
        Some(Change {
            snapshot: lights.iter().map(|entry| entry.light).zip(values).collect(),
            steps,
        })
    }

    /// `change` reached the app: its steps are what the lights show now.
    pub fn sent(&mut self, change: &Change) {
        for ((light, _), &step) in change.snapshot.iter().zip(&change.steps) {
            if let Some(kept) = self.kept.get_mut(light) {
                kept.last = step;
            }
        }
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
        .then(|| descriptor.displays.iter().find(|display| display.name == widget.bind()))
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

/// Reads the lights on the watch's list and sends their values when one
/// visibly changes. The first reading of each light is always sent. Each
/// turn goes: wait, check closed, read the table, compare, and send. It
/// waits on the list for up to [`INTERVAL`], so a new list is read at once,
/// and a dropped handle ends the thread at once.
pub fn run(watch: &Watch, tx: Outlet<Vec<(Light, f32)>>) {
    let mut tracker = Tracker::default();
    // Nothing seen yet, so the first wait returns at once.
    let mut seen = None;
    loop {
        tx.wait(&watch.list, seen, INTERVAL);
        if tx.is_closed() {
            break;
        }
        let (lights, version) = watch.list.get();
        seen = Some(version);
        let readings: Vec<f32> = lights.iter().map(|entry| watch.reader.read(entry.source)).collect();
        if let Some(change) = tracker.read(&lights, &readings, Instant::now()) {
            match tx.try_send(change.snapshot.clone()) {
                Ok(()) => tracker.sent(&change),
                // The UI has not taken the last one yet: try again next
                // time, with fresh values.
                Err(e) if e.is_full() => {}
                Err(_) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests;
