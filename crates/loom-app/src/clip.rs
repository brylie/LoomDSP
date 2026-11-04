//! A meter's clip light and peak hold (`docs/design/panels.md`, Meter widget).
//!
//! A probe is view state and goes away whenever its meter leaves the screen,
//! so what the player expects to last cannot live with it. [`ClipHolds`], on
//! the UI thread, owns the state by module and widget. [`Peaks`], on the
//! telemetry thread, watches the raw readings and reports only what could
//! change what the UI holds.

use loom_core::{MeterReading, ProbeId};
use std::collections::{HashMap, HashSet};

/// The peak at which the light turns on: 1.0 is 0 dB, or 10 V. Cables are
/// never clipped, so this is what would clip if it reached the output.
const CLIP: f32 = 1.0;

/// How far a peak must rise before the UI hears of it: 0.1 dB, the readout's
/// step. A level step is too coarse for it, and every message redraws the
/// window.
const STEP_RATIO: f32 = 1.011_579_4;

/// What one probe has seen since the last reset.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PeakNews {
    /// The highest finite peak.
    pub peak: f32,
    /// True if a peak or RMS was not a finite number.
    pub bad: bool,
    /// The probe's reset count this came after. The UI drops news from before
    /// its meter's latest reset, which was on its way when it happened.
    pub after: u64,
}

impl PeakNews {
    /// True if this would light a clip light.
    fn clips(self) -> bool {
        self.bad || self.peak >= CLIP
    }

    /// True if the UI, knowing `shown`, should hear of this.
    fn beyond(self, shown: Self) -> bool {
        self.clips() != shown.clips() || self.peak > shown.peak.max(1e-9) * STEP_RATIO
    }
}

/// The raw peaks of the meter probes, on the telemetry thread. They build up
/// until the UI has them, so a one-block clip survives a busy UI.
#[derive(Debug, Default)]
pub struct Peaks {
    seen: HashMap<ProbeId, PeakNews>,
    shown: HashMap<ProbeId, PeakNews>,
    /// The reset count each probe was last told.
    resets: HashMap<ProbeId, u64>,
}

impl Peaks {
    /// Takes readings from the probes in `meters`. A new count in `resets` means
    /// the UI reset that probe's meter, so everything since is news again. Other
    /// probes keep what they have, so a reset never costs another meter a clip.
    pub fn note(&mut self, resets: &HashMap<ProbeId, u64>, meters: &HashSet<ProbeId>, readings: &[MeterReading]) {
        self.seen.retain(|probe, _| meters.contains(probe));
        self.shown.retain(|probe, _| meters.contains(probe));
        self.resets.retain(|probe, _| meters.contains(probe));
        for probe in meters {
            let count = resets.get(probe).copied().unwrap_or(0);
            if self.resets.insert(*probe, count).is_some_and(|last| last != count) {
                self.seen.remove(probe);
                self.shown.remove(probe);
            }
        }
        for reading in readings.iter().filter(|r| meters.contains(&r.probe)) {
            let news = self.seen.entry(reading.probe).or_default();
            let (right_peak, right_rms) = reading.right.unwrap_or((0.0, 0.0));
            for peak in [reading.peak, right_peak] {
                if peak.is_finite() {
                    news.peak = news.peak.max(peak);
                }
            }
            news.bad |= ![reading.peak, reading.rms, right_peak, right_rms]
                .iter()
                .all(|x| x.is_finite());
        }
    }

    /// What the UI has not heard yet.
    pub fn news(&self) -> Vec<(ProbeId, PeakNews)> {
        self.seen
            .iter()
            .filter(|(probe, news)| news.beyond(self.shown.get(probe).copied().unwrap_or_default()))
            .map(|(&probe, &news)| {
                let after = self.resets.get(&probe).copied().unwrap_or(0);
                (probe, PeakNews { after, ..news })
            })
            .collect()
    }

    /// Notes that the UI has these.
    pub fn sent(&mut self, news: &[(ProbeId, PeakNews)]) {
        self.shown.extend(news.iter().copied());
    }
}

/// One meter's state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Hold {
    /// True while the clip light is on.
    pub lit: bool,
    /// The highest peak since the last reset. 0 before any signal.
    pub peak: f32,
}

impl Hold {
    /// The readout: the peak in dB to one decimal, and `--` before any signal.
    pub fn text(&self) -> String {
        if self.peak > 0.0 {
            format!("{:+.1} dB", 20.0 * self.peak.log10())
        } else {
            "--".to_string()
        }
    }
}

/// Every meter's clip light and peak hold, by module and widget. It lives on
/// the UI thread and goes with the patch: a new patch starts with a new one.
#[derive(Debug, Default)]
pub struct ClipHolds {
    holds: HashMap<(u32, usize), Hold>,
    /// How many times each meter was reset. Kept apart from `holds`, because a
    /// reset removes the hold.
    resets: HashMap<(u32, usize), u64>,
}

impl ClipHolds {
    /// A meter's state.
    pub fn get(&self, meter: (u32, usize)) -> Hold {
        self.holds.get(&meter).copied().unwrap_or_default()
    }

    /// How many times a meter was reset, for [`Peaks::note`].
    pub fn resets(&self, meter: (u32, usize)) -> u64 {
        self.resets.get(&meter).copied().unwrap_or(0)
    }

    /// Takes news for a meter. Returns true if what it shows changed.
    pub fn take(&mut self, meter: (u32, usize), news: PeakNews) -> bool {
        let hold = self.holds.entry(meter).or_default();
        let before = *hold;
        hold.lit |= news.clips();
        hold.peak = hold.peak.max(news.peak);
        *hold != before
    }

    /// Clears the light and keeps the readout.
    pub fn clear_light(&mut self, meter: (u32, usize)) {
        self.holds.entry(meter).or_default().lit = false;
        *self.resets.entry(meter).or_default() += 1;
    }

    /// Clears the light and the readout.
    pub fn reset(&mut self, meter: (u32, usize)) {
        self.holds.remove(&meter);
        *self.resets.entry(meter).or_default() += 1;
    }

    /// Forgets meters `keep` returns false for, whose modules are gone.
    pub fn retain(&mut self, mut keep: impl FnMut(u32, usize) -> bool) {
        self.holds.retain(|&(module, widget), _| keep(module, widget));
        self.resets.retain(|&(module, widget), _| keep(module, widget));
    }
}

#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod tests;
