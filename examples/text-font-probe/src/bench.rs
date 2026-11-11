//! Times shaping many short labels with each candidate, as a canvas full
//! of tiles needs: the first time, and again after every label changed.
//!
//! Run it with `cargo run --release -- --bench`. Each case runs once to
//! warm the font caches, then five times; the median is printed.

use std::time::{Duration, Instant};

/// How many labels: a large patch's tile titles, port names, and values.
const COUNT: usize = 2_000;
/// Words the labels are made from.
const WORDS: [&str; 8] = [
    "Cutoff",
    "Resonance",
    "Oscillator",
    "Envelope",
    "Attack",
    "Release",
    "Feedback",
    "Mix",
];
/// The size of a tile label, in pixels.
const SIZE: f32 = 9.0;
/// The system interface family's name in the font database on macOS.
const SYSTEM_FAMILY: &str = ".SF NS";
/// Timed runs per case.
const RUNS: usize = 5;

/// `COUNT` labels, such as "Cutoff 17 Hz", all different.
fn labels(edit: bool) -> Vec<String> {
    (0..COUNT)
        .map(|i| {
            let unit = if edit { "dB" } else { "Hz" };
            format!("{} {} {unit}", WORDS[i % WORDS.len()], i)
        })
        .collect()
}

/// The median of `RUNS` timings of `case`, after one run to warm up.
fn median(mut case: impl FnMut() -> Duration) -> Duration {
    case();
    let mut times: Vec<Duration> = (0..RUNS).map(|_| case()).collect();
    times.sort();
    times[RUNS / 2]
}

/// Prints one row: the case, and microseconds per label.
fn row(name: &str, first: Duration, again: Duration) {
    let per = |d: Duration| d.as_secs_f64() * 1e6 / COUNT as f64;
    println!(
        "{name:<26} first {:>7.2} ms ({:>5.2} us/label)   again {:>7.2} ms ({:>5.2} us/label)",
        first.as_secs_f64() * 1e3,
        per(first),
        again.as_secs_f64() * 1e3,
        per(again)
    );
}

/// cosmic-text 0.15 with the given shaping: one buffer per label, then the
/// same buffers given new text.
fn cosmic_15(shaping: cosmic_text_15::Shaping, name: &str) {
    use cosmic_text_15::{Attrs, Buffer, Family, FontSystem, Metrics};
    let mut system = FontSystem::new();
    let attrs = Attrs::new().family(Family::SansSerif);
    let (start, edited) = (labels(false), labels(true));
    let mut buffers = Vec::new();
    let first = median(|| {
        let at = Instant::now();
        buffers = start
            .iter()
            .map(|label| {
                let mut buffer = Buffer::new(&mut system, Metrics::new(SIZE, SIZE * 1.3));
                buffer.set_size(&mut system, None, None);
                buffer.set_text(&mut system, label, &attrs, shaping, None);
                buffer.shape_until_scroll(&mut system, false);
                buffer
            })
            .collect();
        at.elapsed()
    });
    let mut flip = false;
    let again = median(|| {
        flip = !flip;
        let texts = if flip { &edited } else { &start };
        let at = Instant::now();
        for (buffer, label) in buffers.iter_mut().zip(texts) {
            buffer.set_text(&mut system, label, &attrs, shaping, None);
            buffer.shape_until_scroll(&mut system, false);
        }
        at.elapsed()
    });
    row(name, first, again);
}

/// cosmic-text 0.19 with the given shaping, the same way. With `present`,
/// the sans-serif family is set to the system font itself, so no label
/// needs a fallback search: a fair comparison with Parley's `system-ui`.
fn cosmic_19(shaping: cosmic_text_19::Shaping, present: bool, name: &str) {
    use cosmic_text_19::{Attrs, Buffer, Family, FontSystem, Metrics};
    let mut system = FontSystem::new();
    if present {
        system.db_mut().set_sans_serif_family(SYSTEM_FAMILY);
    }
    let attrs = Attrs::new().family(Family::SansSerif);
    let (start, edited) = (labels(false), labels(true));
    let mut buffers = Vec::new();
    let first = median(|| {
        let at = Instant::now();
        buffers = start
            .iter()
            .map(|label| {
                let mut buffer = Buffer::new(&mut system, Metrics::new(SIZE, SIZE * 1.3));
                buffer.set_size(None, None);
                buffer.set_text(label, &attrs, shaping, None);
                buffer.shape_until_scroll(&mut system, false);
                buffer
            })
            .collect();
        at.elapsed()
    });
    let mut flip = false;
    let again = median(|| {
        flip = !flip;
        let texts = if flip { &edited } else { &start };
        let at = Instant::now();
        for (buffer, label) in buffers.iter_mut().zip(texts) {
            buffer.set_text(label, &attrs, shaping, None);
            buffer.shape_until_scroll(&mut system, false);
        }
        at.elapsed()
    });
    row(name, first, again);
}

/// Parley 0.12 with `system-ui`: one layout per label. A changed label
/// needs a new layout, so "again" builds them all again.
fn parley() {
    let mut fonts = parley::FontContext::new();
    let mut layouts = parley::LayoutContext::<[u8; 4]>::new();
    let (start, edited) = (labels(false), labels(true));
    let mut build = |texts: &[String]| -> Duration {
        let at = Instant::now();
        let built: Vec<parley::Layout<[u8; 4]>> = texts
            .iter()
            .map(|label| {
                let mut builder = layouts.ranged_builder(&mut fonts, label, 1.0, true);
                builder.push_default(parley::StyleProperty::FontSize(SIZE));
                builder.push_default(parley::StyleProperty::FontFamily(
                    parley::GenericFamily::SystemUi.into(),
                ));
                let mut layout = builder.build(label);
                layout.break_all_lines(None);
                layout
            })
            .collect();
        let elapsed = at.elapsed();
        std::hint::black_box(built);
        elapsed
    };
    let first = median(|| build(&start));
    let mut flip = false;
    let again = median(|| {
        flip = !flip;
        build(if flip { &edited } else { &start })
    });
    row("parley 0.12 (system-ui)", first, again);
}

/// Runs every case.
pub fn run() {
    println!("{COUNT} labels at {SIZE} px, median of {RUNS} runs");
    cosmic_15(cosmic_text_15::Shaping::Basic, "cosmic-text 0.15 basic");
    cosmic_15(cosmic_text_15::Shaping::Advanced, "cosmic-text 0.15 advanced");
    cosmic_19(cosmic_text_19::Shaping::Basic, false, "cosmic-text 0.19 basic");
    cosmic_19(cosmic_text_19::Shaping::Advanced, false, "cosmic-text 0.19 advanced");
    cosmic_19(cosmic_text_19::Shaping::Basic, true, "0.19 basic, SF named");
    cosmic_19(cosmic_text_19::Shaping::Advanced, true, "0.19 advanced, SF named");
    parley();
}
