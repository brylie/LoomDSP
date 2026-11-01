//! Watches the display values behind the lights on screen, off the UI
//! thread, and wakes the UI only when a light visibly changes
//! (`docs/design/panels.md`, Redraw).
//!
//! Every message to the app redraws the window, so a plain timer would draw
//! 30 frames a second even when nothing moves. This watcher reads the
//! engine's display table on its own thread and sends a message only when a
//! light's brightness changes by at least one of 256 levels.
//!
//! A trigger light holds its flash: a read of 1.0 lights it at full, and it
//! fades to dark over [`FADE`]. The watcher is the only reader of trigger
//! values, because a read takes them (`docs/design/panels.md`, Lights for
//! one-sample triggers). It sends while a light fades, then nothing.

use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_core::{DisplayKind, DisplayReader, DisplaySource};
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// How often the watcher reads the display table: about 30 times a second.
pub const INTERVAL: Duration = Duration::from_millis(33);

/// How long a trigger light takes to fade from full to dark.
pub const FADE: Duration = Duration::from_millis(120);

/// A light, by module ID and widget index.
pub type Light = (u32, usize);

/// The lights to watch, and where their values live.
#[derive(Clone)]
pub struct Watch {
    pub reader: DisplayReader,
    pub lights: Vec<(Light, DisplaySource)>,
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

/// The drawn levels of `readings`, if any differs from `last`.
pub fn changed(last: &[Option<u8>], readings: &[f32]) -> Option<Vec<Option<u8>>> {
    let levels: Vec<Option<u8>> = readings.iter().map(|&r| Some(level(r))).collect();
    (levels != last).then_some(levels)
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
        while !tx.is_closed() {
            let snapshot = watch.shown(&mut fades, Instant::now());
            let readings: Vec<f32> = snapshot.iter().map(|&(_, r)| r).collect();
            if let Some(levels) = changed(&last, &readings) {
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
