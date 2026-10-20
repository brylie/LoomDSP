//! Frame-rate measurement: the stats bar numbers, and the same numbers as
//! text on stdout for scripts and agents.
//!
//! `--bench` measures with the camera moving and `--measure` with it
//! still. Both redraw every frame, print one line a second, and with
//! `--bench-seconds` exit after a set time with a final line. The method
//! and results are in `docs/benchmarks/p1-canvas.md`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Command-line options for measuring the frame rate.
///
/// No implicit group: `screenshot::Options` already uses the name.
#[derive(clap::Args, Debug, Clone, Default)]
#[group(skip)]
pub struct Options {
    /// Measure the frame rate with the camera moving (performance gate),
    /// and print the numbers to stdout once a second.
    #[arg(long, group = "measuring")]
    pub bench: bool,
    /// Like --bench, but the camera stays still: measures the cost of
    /// drawing the patch as it is.
    #[arg(long, group = "measuring")]
    pub measure: bool,
    /// With --bench or --measure: exit after this many seconds, with a
    /// final summary line.
    #[arg(long, value_name = "N", requires = "measuring")]
    pub bench_seconds: Option<f64>,
}

impl Options {
    /// True when the app starts measuring.
    pub fn measuring(&self) -> bool {
        self.bench || self.measure
    }
}

/// The frame rate and frame times, from a `FrameMeter`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub fps: f64,
    pub average_ms: f64,
    pub worst_ms: f64,
}

impl Stats {
    /// The stats bar text.
    pub fn summary(&self) -> String {
        format!(
            "{:.0} fps | frame {:.1} ms (worst {:.1} ms)",
            self.fps, self.average_ms, self.worst_ms
        )
    }

    /// One stdout line: `key=value` pairs, so a script can split on spaces
    /// and `=`.
    pub fn line(&self) -> String {
        format!(
            "fps={:.1} avg_ms={:.2} worst_ms={:.2}",
            self.fps, self.average_ms, self.worst_ms
        )
    }
}

/// Frame timing over the last second, and the worst frame since the start.
#[derive(Default)]
pub struct FrameMeter {
    times: VecDeque<Instant>,
    seen: u64,
    worst: Duration,
}

/// The frames skipped at the start, which include start-up work.
const WARM_UP: u64 = 10;

impl FrameMeter {
    pub fn record(&mut self, now: Instant) {
        self.seen += 1;
        if let Some(&last) = self.times.back()
            && self.seen > WARM_UP
        {
            self.worst = self.worst.max(now - last);
        }
        self.times.push_back(now);
        while self.times.front().is_some_and(|&t| now - t > Duration::from_secs(1)) {
            self.times.pop_front();
        }
    }

    pub fn stats(&self) -> Stats {
        let frames = self.times.len().saturating_sub(1);
        let average_ms = match (self.times.front(), self.times.back()) {
            (Some(first), Some(last)) if frames > 0 => (*last - *first).as_secs_f64() * 1000.0 / frames as f64,
            _ => 0.0,
        };
        Stats {
            fps: frames as f64,
            average_ms,
            worst_ms: self.worst.as_secs_f64() * 1000.0,
        }
    }
}

/// Prints the meter's numbers once a second, and says when the run is over.
pub struct Report {
    start: Instant,
    next: Instant,
    end: Option<Instant>,
    /// Frames after the warm-up: the first and the last, and how many.
    first: Option<Instant>,
    last: Option<Instant>,
    frames: u64,
    seen: u64,
}

impl Report {
    /// A report for these options, or `None` when the app does not measure
    /// from the command line.
    pub fn new(options: &Options, now: Instant) -> Option<Self> {
        options.measuring().then(|| Self {
            start: now,
            next: now + Duration::from_secs(1),
            end: options.bench_seconds.map(|s| now + Duration::from_secs_f64(s.max(0.0))),
            first: None,
            last: None,
            frames: 0,
            seen: 0,
        })
    }

    /// Counts a frame. Returns a line to print when a second has passed.
    pub fn frame(&mut self, meter: &FrameMeter, now: Instant) -> Option<String> {
        self.seen += 1;
        if self.seen > WARM_UP {
            self.first.get_or_insert(now);
            self.last = Some(now);
            self.frames += 1;
        }
        (now >= self.next).then(|| {
            // Skip any whole seconds without a frame, so the lines do not
            // come in a burst after a stall.
            while self.next <= now {
                self.next += Duration::from_secs(1);
            }
            meter.stats().line()
        })
    }

    /// True once `--bench-seconds` have passed.
    pub fn done(&self, now: Instant) -> bool {
        self.end.is_some_and(|end| now >= end)
    }

    /// The final line: averages over the whole run after the warm-up.
    pub fn summary(&self, meter: &FrameMeter, now: Instant) -> String {
        let intervals = self.frames.saturating_sub(1);
        let span = match (self.first, self.last) {
            (Some(first), Some(last)) => (last - first).as_secs_f64(),
            _ => 0.0,
        };
        let (fps, average_ms) = if intervals > 0 && span > 0.0 {
            (intervals as f64 / span, span * 1000.0 / intervals as f64)
        } else {
            (0.0, 0.0)
        };
        let total = Stats {
            fps,
            average_ms,
            worst_ms: meter.stats().worst_ms,
        };
        format!(
            "final {} frames={} seconds={:.1}",
            total.line(),
            self.frames,
            (now - self.start).as_secs_f64()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds frames 10 ms apart, starting at `start`.
    fn run(meter: &mut FrameMeter, report: &mut Report, start: Instant, count: u32) -> Vec<String> {
        (0..count)
            .filter_map(|i| {
                let now = start + Duration::from_millis(10 * u64::from(i));
                meter.record(now);
                report.frame(meter, now)
            })
            .collect()
    }

    #[test]
    fn prints_a_line_a_second_in_a_stable_form() {
        let start = Instant::now();
        let options = Options {
            measure: true,
            ..Default::default()
        };
        let mut report = Report::new(&options, start).unwrap();
        let mut meter = FrameMeter::default();
        let lines = run(&mut meter, &mut report, start, 350);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[1], "fps=100.0 avg_ms=10.00 worst_ms=10.00");
        assert!(!report.done(start + Duration::from_secs(60)));
    }

    #[test]
    fn ends_after_the_given_seconds_with_a_summary() {
        let start = Instant::now();
        let options = Options {
            bench: true,
            bench_seconds: Some(2.0),
            ..Default::default()
        };
        let mut report = Report::new(&options, start).unwrap();
        let mut meter = FrameMeter::default();
        run(&mut meter, &mut report, start, 201);
        let end = start + Duration::from_secs(2);
        assert!(report.done(end));
        assert!(!report.done(end - Duration::from_millis(1)));
        assert_eq!(
            report.summary(&meter, end),
            "final fps=100.0 avg_ms=10.00 worst_ms=10.00 frames=191 seconds=2.0"
        );
    }

    #[test]
    fn no_report_without_a_flag() {
        assert!(Report::new(&Options::default(), Instant::now()).is_none());
    }
}
