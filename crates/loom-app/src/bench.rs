//! Frame-rate measurement: the stats bar numbers, and a report on stdout
//! for scripts and agents.
//!
//! `--bench` redraws every frame with the camera moving, `--measure` with
//! it still, and `--measure-idle` draws only what the app would draw
//! anyway. All three print one line a second from the canvas counters
//! (`loom_canvas::stats`), and with `--bench-seconds` exit after a set time
//! with a final line. The method and results are in
//! `docs/benchmarks/p1-canvas.md`.

use iced::futures::Stream;
use iced::futures::channel::mpsc;
use loom_canvas::stats::{self, FrameCauses, RenderCounts};
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
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
    /// Print the same numbers without forcing a redraw: counts only the
    /// frames the app draws by itself. An idle patch should draw none.
    #[arg(long, group = "measuring")]
    pub measure_idle: bool,
    /// With --bench, --measure, or --measure-idle: exit after this many
    /// seconds, with a final summary line.
    #[arg(long, value_name = "N", requires = "measuring", value_parser = seconds)]
    pub bench_seconds: Option<f64>,
}

/// The longest run `--bench-seconds` takes: a day. Longer is surely a typo,
/// and infinity or NaN would not make a duration at all.
const MAX_SECONDS: f64 = 86_400.0;

fn seconds(text: &str) -> Result<f64, String> {
    let s: f64 = text
        .parse()
        .map_err(|_| format!("`{text}` is not a number of seconds"))?;
    if s.is_finite() && (0.0..=MAX_SECONDS).contains(&s) {
        Ok(s)
    } else {
        Err(format!("give a number of seconds from 0 to {MAX_SECONDS}"))
    }
}

impl Options {
    /// True when the app redraws every frame from the start.
    pub fn redraws(&self) -> bool {
        self.bench || self.measure
    }

    /// The stdout report, when one of the flags asks for it.
    pub fn reporter(&self) -> Option<Reporter> {
        (self.bench || self.measure || self.measure_idle).then(|| Reporter {
            seconds: self
                .bench_seconds
                .map(|s| Duration::from_secs_f64(s.clamp(0.0, MAX_SECONDS))),
        })
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
}

/// Frame timing for the stats bar: the last second, and the worst frame
/// since the start.
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

/// Prints a line a second to stdout from its own thread, and ends the run
/// after `--bench-seconds`.
///
/// It reads the canvas counters and sends the app nothing until the run
/// ends: in Iced every message redraws the window, so a timer in the app
/// would add the very frames an idle measurement looks for.
#[derive(Debug, Clone)]
pub struct Reporter {
    seconds: Option<Duration>,
}

impl Hash for Reporter {
    // One report per run: opening another patch keeps it going.
    fn hash<H: Hasher>(&self, state: &mut H) {
        "loom-app bench report".hash(state);
    }
}

/// Runs the report. Yields once, when `--bench-seconds` have passed.
pub fn stream(reporter: &Reporter) -> impl Stream<Item = ()> + use<> {
    let (mut tx, rx) = mpsc::channel(1);
    let seconds = reporter.seconds;
    std::thread::spawn(move || {
        let start = Instant::now();
        let first = RenderCounts::process();
        let first_causes = FrameCauses::process();
        let first_cpu = process_cpu();
        let end = seconds.map(|s| start + s);
        // The final line starts from the first line, so start-up work is
        // left out of its averages.
        let mut settled: Option<(Instant, RenderCounts, Duration)> = None;
        let mut last = (start, first, first_cpu);
        let mut next = start + Duration::from_secs(1);
        loop {
            let wake = end.map_or(next, |end| next.min(end));
            std::thread::sleep(wake.saturating_duration_since(Instant::now()));
            if tx.is_closed() {
                return;
            }
            let now = Instant::now();
            let counts = RenderCounts::process();
            let causes = FrameCauses::process().since(first_causes);
            let cpu = process_cpu();
            if end.is_some_and(|end| now >= end) {
                let (from, at, cpu_from) = settled.unwrap_or((start, first, first_cpu));
                let mut window = Window::new(first, at, counts, now - from);
                window.causes = causes;
                window.cpu = cpu_percent(cpu_from, cpu, now - from);
                println!("final {}", line(&window));
                let _ = tx.try_send(());
                return;
            }
            let mut window = Window::new(first, last.1, counts, now - last.0);
            window.causes = causes;
            window.cpu = cpu_percent(last.2, cpu, now - last.0);
            println!("{}", line(&window));
            settled.get_or_insert((now, counts, cpu));
            last = (now, counts, cpu);
            next += Duration::from_secs(1);
        }
    });
    rx
}

/// What a report line describes: frames over a stretch of time, and the
/// totals since the report started.
struct Window {
    frames: u64,
    elapsed: Duration,
    totals: RenderCounts,
    worst: Duration,
    causes: FrameCauses,
    cpu: f64,
}

impl Window {
    /// `start` is when the report began, `from` the start of this stretch,
    /// and `now` its end.
    fn new(start: RenderCounts, from: RenderCounts, now: RenderCounts, elapsed: Duration) -> Self {
        Self {
            frames: now.frames - from.frames,
            elapsed,
            totals: since(start, now),
            worst: stats::longest_frame_gap(),
            causes: FrameCauses::default(),
            cpu: 0.0,
        }
    }
}

fn since(a: RenderCounts, b: RenderCounts) -> RenderCounts {
    RenderCounts {
        frames: b.frames - a.frames,
        scene_builds: b.scene_builds - a.scene_builds,
        control_builds: b.control_builds - a.control_builds,
        scene_uploads: b.scene_uploads - a.scene_uploads,
        control_uploads: b.control_uploads - a.control_uploads,
        overlay_uploads: b.overlay_uploads - a.overlay_uploads,
        trace_builds: b.trace_builds - a.trace_builds,
        trace_uploads: b.trace_uploads - a.trace_uploads,
    }
}

/// One stdout line: `key=value` pairs, so a script can split on spaces and
/// `=`. `fps` and `avg_ms` cover the stretch; `worst_ms` is the longest gap
/// between two frames since the start; the counts are totals since the
/// report started.
fn line(w: &Window) -> String {
    let seconds = w.elapsed.as_secs_f64();
    let fps = if seconds > 0.0 { w.frames as f64 / seconds } else { 0.0 };
    let average_ms = if w.frames > 0 {
        seconds * 1000.0 / w.frames as f64
    } else {
        0.0
    };
    let t = &w.totals;
    format!(
        "fps={fps:.1} avg_ms={average_ms:.2} worst_ms={:.2} frames={} scene_builds={} control_builds={} \
         scene_uploads={} control_uploads={} overlay_uploads={} trace_uploads={} \
         causes=tick:{},live:{},meters:{},control:{},input:{},redraw:{},forced:{},other:{} cpu={:.1}",
        w.worst.as_secs_f64() * 1000.0,
        t.frames,
        t.scene_builds,
        t.control_builds,
        t.scene_uploads,
        t.control_uploads,
        t.overlay_uploads,
        t.trace_uploads,
        w.causes.0[0],
        w.causes.0[1],
        w.causes.0[2],
        w.causes.0[3],
        w.causes.0[4],
        w.causes.0[5],
        w.causes.0[6],
        w.causes.0[7],
        w.cpu
    )
}

/// Whole-process CPU use as a percent of one core over the report interval.
fn cpu_percent(from: Duration, to: Duration, wall: Duration) -> f64 {
    if wall.is_zero() {
        return 0.0;
    }
    100.0 * to.saturating_sub(from).as_secs_f64() / wall.as_secs_f64()
}

/// Total user and system CPU time used by the app process.
#[cfg(unix)]
fn process_cpu() -> Duration {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: `getrusage` writes a complete rusage on success, and the
    // pointer refers to live, properly aligned storage.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return Duration::ZERO;
    }
    // SAFETY: getrusage succeeded and initialized the whole structure.
    let usage = unsafe { usage.assume_init() };
    let micros = |tv: libc::timeval| {
        (tv.tv_sec as u64)
            .saturating_mul(1_000_000)
            .saturating_add(tv.tv_usec as u64)
    };
    Duration::from_micros(micros(usage.ru_utime).saturating_add(micros(usage.ru_stime)))
}

/// Total user and system CPU time used by the app process on Windows.
#[cfg(windows)]
fn process_cpu() -> Duration {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut created = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exited = created;
    let mut kernel = created;
    let mut user = created;
    // SAFETY: all four output pointers refer to writable FILETIME values.
    if unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Duration::ZERO;
    }
    let ticks = |v: FILETIME| (u64::from(v.dwHighDateTime) << 32) | u64::from(v.dwLowDateTime);
    Duration::from_nanos((ticks(user).saturating_add(ticks(kernel))).saturating_mul(100))
}

/// Unsupported desktop platforms have no process CPU timer yet.
#[cfg(not(any(unix, windows)))]
fn process_cpu() -> Duration {
    Duration::ZERO
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_key_value_pairs() {
        let w = Window {
            frames: 120,
            elapsed: Duration::from_secs(1),
            totals: RenderCounts {
                frames: 600,
                scene_builds: 1,
                control_builds: 2,
                scene_uploads: 1,
                control_uploads: 2,
                overlay_uploads: 0,
                trace_builds: 3,
                trace_uploads: 3,
            },
            worst: Duration::from_millis(9),
            causes: FrameCauses([2, 70, 1, 0, 3, 0, 44, 0]),
            cpu: 4.1,
        };
        assert_eq!(
            line(&w),
            "fps=120.0 avg_ms=8.33 worst_ms=9.00 frames=600 scene_builds=1 control_builds=2 \
             scene_uploads=1 control_uploads=2 overlay_uploads=0 trace_uploads=3 \
             causes=tick:2,live:70,meters:1,control:0,input:3,redraw:0,forced:44,other:0 cpu=4.1"
        );
    }

    #[test]
    fn an_idle_line_has_no_frames() {
        let w = Window {
            frames: 0,
            elapsed: Duration::from_secs(1),
            totals: RenderCounts::default(),
            worst: Duration::ZERO,
            causes: FrameCauses::default(),
            cpu: 0.0,
        };
        assert!(line(&w).starts_with("fps=0.0 avg_ms=0.00 worst_ms=0.00 frames=0 "));
    }

    #[test]
    fn bench_seconds_must_be_a_usable_duration() {
        assert_eq!(seconds("10"), Ok(10.0));
        assert_eq!(seconds("0.5"), Ok(0.5));
        for bad in ["inf", "NaN", "-1", "1e30", "ten"] {
            assert!(seconds(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn report_only_with_a_flag() {
        assert!(Options::default().reporter().is_none());
        let idle = Options {
            measure_idle: true,
            ..Default::default()
        };
        assert!(idle.reporter().is_some());
        assert!(!idle.redraws());
    }

    /// A measured run reports a settled interval and then ends on time.
    #[test]
    fn measured_run_reports_and_finishes() {
        use iced::futures::StreamExt;
        let reporter = Reporter {
            seconds: Some(Duration::from_millis(2_100)),
        };
        let started = Instant::now();
        let mut updates = stream(&reporter);
        assert_eq!(iced::futures::executor::block_on(updates.next()), Some(()));
        assert!(started.elapsed() >= Duration::from_secs(2));
    }

    /// A zero wall interval has no CPU percentage; later CPU time is a
    /// percentage of one core.
    #[test]
    fn cpu_percent_uses_the_report_interval() {
        assert_eq!(
            cpu_percent(Duration::ZERO, Duration::from_millis(1), Duration::ZERO),
            0.0
        );
        assert_eq!(
            cpu_percent(
                Duration::from_millis(10),
                Duration::from_millis(60),
                Duration::from_millis(100)
            ),
            50.0
        );
    }
}
