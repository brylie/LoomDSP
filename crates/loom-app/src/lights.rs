//! Watches the display values behind the lights on screen, off the UI
//! thread, and wakes the UI only when a light visibly changes
//! (`docs/design/panels.md`, Redraw).
//!
//! Every message to the app redraws the window, so a plain timer would draw
//! 30 frames a second even when nothing moves. This watcher reads the
//! engine's display table on its own thread and sends a message only when a
//! light's brightness changes by at least one of 256 levels.

use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_core::{DisplayReader, DisplaySource};
use std::hash::{Hash, Hasher};
use std::time::Duration;

/// How often the watcher reads the display table: about 30 times a second.
pub const INTERVAL: Duration = Duration::from_millis(33);

/// A light, by module ID and widget index.
pub type Light = (u32, usize);

/// The lights to watch, and where their values live.
#[derive(Clone)]
pub struct Watch {
    pub reader: DisplayReader,
    pub lights: Vec<(Light, DisplaySource)>,
}

impl Hash for Watch {
    // The subscription restarts when the set of lights changes. The reader
    // is the same for the whole app.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.lights.hash(state);
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
        while !tx.is_closed() {
            let readings: Vec<f32> = watch.lights.iter().map(|(_, s)| watch.reader.read(*s)).collect();
            if let Some(levels) = changed(&last, &readings) {
                let snapshot = watch.lights.iter().map(|(light, _)| *light).zip(readings).collect();
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
mod tests {
    use super::*;

    #[test]
    fn only_visible_changes_are_sent() {
        let first = changed(&[None, None], &[0.0, 1.0]).expect("the first reading is sent");
        assert_eq!(first, vec![Some(0), Some(255)]);
        // Less than one level of 256: not drawn, not sent.
        assert_eq!(changed(&first, &[0.001, 0.999]), None);
        assert_eq!(changed(&first, &[0.5, 1.0]), Some(vec![Some(128), Some(255)]));
        // Out of range values are clamped, as the light draws them.
        assert_eq!(changed(&first, &[-3.0, 7.0]), None);
    }
}
