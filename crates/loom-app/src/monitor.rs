//! The telemetry thread: drains the engine's meter and scope queues off the
//! UI thread, and wakes the UI only when a meter level visibly changes
//! (`docs/design/panels.md`, How levels reach a meter).
//!
//! Every message to the app redraws the window, so draining telemetry on a
//! UI timer would draw frames even when every meter is silent.

use crate::live::{Level, Levels};
use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_core::{ProbeId, Telemetry};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How often the thread drains the queues: about 30 times a second.
pub const INTERVAL: Duration = Duration::from_millis(33);

/// Hands the engine's `Telemetry` to the telemetry thread, once.
#[derive(Clone)]
pub struct Monitor {
    telemetry: Arc<Mutex<Option<Telemetry>>>,
    generation: u64,
}

impl Monitor {
    /// `generation` counts the engines the app has built. Opening a patch
    /// builds a new one, and the subscription must restart to take its
    /// `Telemetry`.
    pub fn new(telemetry: Telemetry, generation: u64) -> Self {
        Self {
            telemetry: Arc::new(Mutex::new(Some(telemetry))),
            generation,
        }
    }
}

impl Hash for Monitor {
    // One monitor per engine: the subscription restarts with a new engine.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-app telemetry monitor".hash(state);
        self.generation.hash(state);
    }
}

/// A level as drawn: 256 steps for RMS and peak.
fn steps(level: Level) -> (u8, u8) {
    let step = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    (step(level.rms), step(level.peak))
}

/// The levels that differ from `shown` by at least one step.
pub fn changed(shown: &HashMap<ProbeId, (u8, u8)>, levels: &[(ProbeId, Level)]) -> Vec<(ProbeId, Level)> {
    levels
        .iter()
        .filter(|(probe, level)| shown.get(probe).copied().unwrap_or((0, 0)) != steps(*level))
        .copied()
        .collect()
}

/// Drains the queues every [`INTERVAL`] and yields the meter levels that
/// changed. The thread stops when the app closes.
pub fn stream(monitor: &Monitor) -> impl Stream<Item = Vec<(ProbeId, Level)>> + use<> {
    let (mut tx, rx) = mpsc::channel(1);
    let telemetry = monitor.telemetry.lock().ok().and_then(|mut t| t.take());
    if let Some(mut telemetry) = telemetry {
        std::thread::spawn(move || {
            let mut levels = Levels::default();
            let mut shown: HashMap<ProbeId, (u8, u8)> = HashMap::new();
            while !tx.is_closed() {
                levels.drain(&mut telemetry, Instant::now());
                let current: Vec<(ProbeId, Level)> = levels.all().collect();
                let changes = changed(&shown, &current);
                if !changes.is_empty() {
                    match tx.try_send(changes.clone()) {
                        Ok(()) => {
                            for (probe, level) in changes {
                                shown.insert(probe, steps(level));
                            }
                            shown.retain(|probe, _| current.iter().any(|(p, _)| p == probe));
                        }
                        // The UI has not taken the last one yet: try again
                        // next time, with fresh levels.
                        Err(e) if e.is_full() => {}
                        Err(_) => break,
                    }
                }
                std::thread::sleep(INTERVAL);
            }
        });
    }
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_visible_changes_are_sent() {
        let probe = ProbeId(1);
        let mut shown = HashMap::new();
        let silent = [(probe, Level::default())];
        // A new probe at level 0 looks the same as no meter: nothing to send.
        assert!(changed(&shown, &silent).is_empty());
        let loud = [(probe, Level { rms: 0.5, peak: 0.8 })];
        assert_eq!(changed(&shown, &loud).len(), 1);
        shown.insert(probe, steps(loud[0].1));
        // Less than one step of 256: not drawn, not sent.
        let close = [(
            probe,
            Level {
                rms: 0.501,
                peak: 0.799,
            },
        )];
        assert!(changed(&shown, &close).is_empty());
    }
}
