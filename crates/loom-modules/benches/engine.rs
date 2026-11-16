//! Engine benchmarks (NFR-PERF-08, ADR-0002 validation).
//!
//! Compares the same chain of modules run two ways:
//! - `chain`: acyclic, so it runs a whole block per module call.
//! - `loop`: the last module feeds back into the first, so the engine runs
//!   the chain one sample at a time.
//!
//! The difference is the cost of per-sample feedback regions.
//!
//! The `modulation` groups measure parameter modulation (#139): an SVF
//! whose cutoff is moved by a sine LFO, and one parameter with 0 to 16
//! assignments, to show how the engine's fill scales. The
//! `modulation/changes` group sends assignment changes every block, as a
//! depth drag does: one change, and 16 in one batch, so the cost of the
//! setting blends and of the pass after a batch shows (live patching spec,
//! rule 9, #762).
//!
//! The `modes` group measures sub-patch modes (sub-patches.md, Open
//! points): one voice alone, then eight as a select, a layer, and a stack.
//! Every member of a select runs, so a select of 8 should cost about what
//! a layer of 8 does.
//!
//! The `patch` group runs example patches with a note held:
//! `subtractive.ron`, `fm-feedback.ron`, and `modulation.ron` with one note,
//! and `polysynth.ron` and `polysynth-large.ron` with [`POLYSYNTH_NOTES`].
//!
//! After criterion's groups, `polysynth` prints the polysynth's share of real
//! time, in two lines: the shipped example, and its Quietest variant with the
//! `level` cable ([`polysynth_share`], polyphony.md, The shipped example).
//! `polysynth-large` prints the large reference patch's share in a third
//! line ([`polysynth_large_share`], polyphony.md, The large reference patch).
//! The lines report and never fail, since the number depends on the machine.
//!
//! Run with: `cargo bench -p loom-modules`, or only the polysynth with
//! `cargo bench -p loom-modules --bench engine -- polysynth`, which prints all
//! three lines, or `-- polysynth-large` for the large patch's line alone.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group};
use loom_core::{EngineConfig, EngineParts, MidiEvent, ModuleId, create_engine};
use loom_patch::Patch;
use std::hint::black_box;
use std::time::Instant;

const BLOCK: usize = 128;

/// `n` add modules in a chain, fed by a constant, ending at the audio output.
/// With `feedback`, the last adder also feeds the first adder's `b` input.
fn chain(n: usize, feedback: bool, kind: &str) -> EngineParts {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let c = &mut parts.controller;
    let source = c.add_module("loom.math.constant").unwrap();
    c.set_param_named(source, "value", 0.001).unwrap();
    let mut previous = (source, "out");
    let mut first = None;
    for _ in 0..n {
        let module = c.add_module(kind).unwrap();
        c.connect_named(previous.0, previous.1, module, "a").unwrap();
        first.get_or_insert(module);
        previous = (module, "out");
    }
    if feedback {
        c.connect_named(previous.0, previous.1, first.unwrap(), "b").unwrap();
    }
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(previous.0, previous.1, out, "in").unwrap();
    let report = c.commit().unwrap();
    assert_eq!(report.loops.len(), usize::from(feedback));
    parts
}

fn feedback_regions(c: &mut Criterion) {
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    for kind in ["loom.math.add", "loom.filter.svf"] {
        let short = kind.rsplit('.').next().unwrap();
        let mut group = c.benchmark_group(format!("feedback/{short}"));
        for n in [1usize, 10, 50] {
            group.throughput(Throughput::Elements((n * BLOCK) as u64));
            for feedback in [false, true] {
                // The SVF's input is named "in", not "a"; use the add chain
                // shape with a mul in front for it.
                let mut parts = if kind == "loom.filter.svf" {
                    svf_chain(n, feedback)
                } else {
                    chain(n, feedback, kind)
                };
                let label = if feedback { "loop" } else { "chain" };
                group.bench_with_input(BenchmarkId::new(label, n), &n, |b, _| {
                    b.iter(|| {
                        parts.engine.process(&mut left, &mut right);
                        black_box(&left);
                    })
                });
            }
        }
        group.finish();
    }
}

/// Like `chain`, but with SVF filters (a heavier module).
fn svf_chain(n: usize, feedback: bool) -> EngineParts {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let c = &mut parts.controller;
    let source = c.add_module("loom.osc.noise").unwrap();
    let mix = c.add_module("loom.math.add").unwrap();
    c.connect_named(source, "out", mix, "a").unwrap();
    let mut previous = mix;
    let mut last_port = "out";
    for _ in 0..n {
        let module = c.add_module("loom.filter.svf").unwrap();
        c.connect_named(previous, last_port, module, "in").unwrap();
        previous = module;
        last_port = "lowpass";
    }
    if feedback {
        c.connect_named(previous, last_port, mix, "b").unwrap();
    }
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(previous, last_port, out, "in").unwrap();
    c.commit().unwrap();
    parts
}

/// The notes the polysynth holds in its benchmarks: two four-note chords,
/// C, E, G, and B from an octave below middle C and from middle C, so every
/// one of its 8 voices plays.
const POLYSYNTH_NOTES: [u8; 8] = [48, 52, 55, 59, 60, 64, 67, 71];

/// An example patch from `examples/patches`, loaded into a new engine.
fn example(name: &str) -> EngineParts {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/patches")
        .join(name);
    let patch = Patch::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    patch.load_into(&mut parts.controller).unwrap();
    parts
}

/// Sends a note-on on frame 0 for each of `notes`.
fn hold(parts: &mut EngineParts, notes: &[u8]) {
    for &note in notes {
        assert!(parts.midi.send(MidiEvent::note_on(0, 0, note, 100)));
    }
}

fn patches(c: &mut Criterion) {
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut group = c.benchmark_group("patch");
    group.throughput(Throughput::Elements(BLOCK as u64));
    for name in [
        "subtractive.ron",
        "fm-feedback.ron",
        "modulation.ron",
        "polysynth.ron",
        "polysynth-large.ron",
    ] {
        let mut parts = example(name);
        let notes: &[u8] = if name.starts_with("polysynth") {
            &POLYSYNTH_NOTES
        } else {
            &[60]
        };
        hold(&mut parts, notes);
        group.bench_function(name, |b| {
            b.iter(|| {
                parts.engine.process(&mut left, &mut right);
                black_box(&left);
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("modulation/changes");
    group.throughput(Throughput::Elements(BLOCK as u64));
    for (label, changed) in [("one", 1usize), ("batch_of_16", 16)] {
        let mut parts = assigned_svf(16);
        let ids: Vec<_> = parts.controller.graph().assignments().map(|(id, _)| id).collect();
        let mut step = 0u32;
        group.bench_function(label, |b| {
            b.iter(|| {
                // A new depth every block keeps a blend running.
                step = step.wrapping_add(1);
                let depth = 0.01 + (step % 8) as f32 * 0.001;
                for &id in &ids[..changed] {
                    parts.controller.set_assignment_depth(id, depth).unwrap();
                }
                parts.engine.process(&mut left, &mut right);
                black_box(&left);
            })
        });
    }
    group.finish();
}

/// How an SVF's cutoff is moved in [`modulated_svf`].
#[derive(Clone, Copy)]
enum Cutoff {
    /// Not moved: the knob stays one value per block.
    Still,
    /// The LFO is cabled to the filter's cutoff input (pitch units).
    Cable,
    /// The LFO is assigned to the cutoff parameter (a per-sample buffer).
    Assigned,
}

/// Noise into an SVF lowpass, with a sine LFO at about 2 Hz. The LFO runs
/// in every case and goes to the right output, so only the cutoff path
/// differs between cases.
fn modulated_svf(cutoff: Cutoff) -> EngineParts {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let c = &mut parts.controller;
    let noise = c.add_module("loom.osc.noise").unwrap();
    let svf = c.add_module("loom.filter.svf").unwrap();
    c.set_param_named(svf, "cutoff", 800.0).unwrap();
    c.connect_named(noise, "out", svf, "in").unwrap();
    let phase = c.add_module("loom.phase.generator").unwrap();
    c.set_param_named(phase, "octave", -7.0).unwrap();
    let lfo = c.add_module("loom.osc.sine").unwrap();
    c.connect_named(phase, "phase", lfo, "phase").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    let join = c.add_module("loom.mix.join").unwrap();
    c.connect_named(join, "out", out, "in").unwrap();
    c.connect_named(svf, "lowpass", join, "left").unwrap();
    c.connect_named(lfo, "out", join, "right").unwrap();
    match cutoff {
        Cutoff::Still => {}
        Cutoff::Cable => {
            c.connect_named(lfo, "out", svf, "cutoff").unwrap();
        }
        Cutoff::Assigned => {
            c.add_assignment_named(lfo, "out", svf, "cutoff", 0.1).unwrap();
        }
    }
    c.commit().unwrap();
    parts
}

/// Noise into an SVF lowpass, with 16 constants. The first `n` constants
/// are assigned to the cutoff, so the fill adds `n` sources per sample.
/// All 16 run in every case, so the module work stays the same.
fn assigned_svf(n: usize) -> EngineParts {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let c = &mut parts.controller;
    let noise = c.add_module("loom.osc.noise").unwrap();
    let svf = c.add_module("loom.filter.svf").unwrap();
    c.set_param_named(svf, "cutoff", 800.0).unwrap();
    c.connect_named(noise, "out", svf, "in").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(svf, "lowpass", out, "in").unwrap();
    for i in 0..16 {
        let source = c.add_module("loom.math.constant").unwrap();
        c.set_param_named(source, "value", 0.5).unwrap();
        if i < n {
            c.add_assignment_named(source, "out", svf, "cutoff", 0.01).unwrap();
        }
    }
    c.commit().unwrap();
    parts
}

/// Benchmarks a modulated SVF cutoff against an unmodulated one, and the
/// fill with 0, 1, 4, and 16 assignments on one parameter (#139).
fn modulation(c: &mut Criterion) {
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];

    let mut group = c.benchmark_group("modulation/svf_cutoff");
    group.throughput(Throughput::Elements(BLOCK as u64));
    for (label, cutoff) in [
        ("still", Cutoff::Still),
        ("cable", Cutoff::Cable),
        ("assigned", Cutoff::Assigned),
    ] {
        let mut parts = modulated_svf(cutoff);
        group.bench_function(label, |b| {
            b.iter(|| {
                parts.engine.process(&mut left, &mut right);
                black_box(&left);
            })
        });
    }
    group.finish();

    let mut group = c.benchmark_group("modulation/assignments");
    group.throughput(Throughput::Elements(BLOCK as u64));
    for n in [0usize, 1, 4, 16] {
        let mut parts = assigned_svf(n);
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, _| {
            b.iter(|| {
                parts.engine.process(&mut left, &mut right);
                black_box(&left);
            })
        });
    }
    group.finish();
}

/// A sine into Dynamics at its default settings, to the audio output.
fn dynamics_on_a_sine() -> EngineParts {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let c = &mut parts.controller;
    let phase = c.add_module("loom.phase.generator").unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    c.connect_named(phase, "phase", sine, "phase").unwrap();
    let dynamics = c.add_module("loom.dyn.dynamics").unwrap();
    c.connect_named(sine, "out", dynamics, "in").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(dynamics, "out", out, "in").unwrap();
    c.commit().unwrap();
    parts
}

/// Benchmarks Dynamics with its settings still, so a change to how it takes its
/// outputs shows (#616). #559 adds the modulated cases and the other modules.
fn dynamics(c: &mut Criterion) {
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut group = c.benchmark_group("dynamics");
    group.throughput(Throughput::Elements(BLOCK as u64));
    let mut parts = dynamics_on_a_sine();
    group.bench_function("still", |b| {
        b.iter(|| {
            parts.engine.process(&mut left, &mut right);
            black_box(&left);
        })
    });
    group.finish();
}

/// One voice definition, `key`: a phase into a saw into an SVF.
fn voice(key: &str) -> String {
    format!(
        r#""{key}": SubPatch(
            metadata: Metadata(name: "{key}"),
            modules: [
                PatchModule(id: 1, kind: "loom.phase.generator@1"),
                PatchModule(id: 2, kind: "loom.osc.saw@1"),
                PatchModule(id: 3, kind: "loom.filter.svf@1", params: {{"cutoff": 800.0}}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [
                PatchCable(from: (1, "phase"), to: (2, "phase")),
                PatchCable(from: (2, "out"), to: (3, "in")),
                PatchCable(from: (3, "lowpass"), to: (4, "in")),
            ],
        ),"#
    )
}

/// A patch whose instance plays `mode` over `n` voices: a single voice for
/// `None`, else a layer or select of `n` member voices, or a stack of `n`
/// copies of one.
fn mode_patch(mode: Option<&str>, n: usize) -> EngineParts {
    let outer = match mode {
        None => voice("outer"),
        Some("Stack") => voice("outer").replace(
            r#"metadata: Metadata(name: "outer"),"#,
            &format!(r#"metadata: Metadata(name: "outer"), mode: Stack, copies: {n},"#),
        ),
        Some(mode) => {
            let members: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
            let modules: String = (1..=n)
                .map(|i| format!(r#"PatchModule(id: {i}, kind: "loom.struct.subpatch@1", subpatch: Some("v{i}")),"#))
                .collect();
            let voices: String = (1..=n).map(|i| voice(&format!("v{i}"))).collect();
            format!(
                r#""outer": SubPatch(metadata: Metadata(name: "outer"), mode: {mode}, members: [{}], modules: [{modules}]),{voices}"#,
                members.join(", ")
            )
        }
    };
    let text = format!(
        r#"Patch(version: 1, subpatches: {{ {outer} }}, modules: [
            PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("outer")),
            PatchModule(id: 2, kind: "loom.io.audio-out@1"),
            PatchModule(id: 3, kind: "loom.clock.transport@1"),
        ], cables: [PatchCable(from: (1, "out"), to: (2, "in"))])"#
    );
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    Patch::from_ron(&text)
        .expect("the bench patch parses")
        .load_into(&mut parts.controller)
        .expect("the bench patch loads");
    parts
}

/// Benchmarks a voice alone and eight voices as a select, a layer, and a
/// stack (sub-patches.md, Open points: CPU in select mode).
fn modes(c: &mut Criterion) {
    let mut left = vec![0.0f32; BLOCK];
    let mut right = vec![0.0f32; BLOCK];
    let mut group = c.benchmark_group("modes");
    group.throughput(Throughput::Elements(BLOCK as u64));
    for (name, mode, n) in [
        ("single", None, 1),
        ("select_8", Some("Select"), 8),
        ("layer_8", Some("Layer"), 8),
        ("stack_8", Some("Stack"), 8),
    ] {
        let mut parts = mode_patch(mode, n);
        group.bench_function(name, |b| {
            b.iter(|| {
                parts.engine.process(&mut left, &mut right);
                black_box(&left);
            })
        });
    }
    group.finish();
}

criterion_group!(benches, feedback_regions, patches, modulation, dynamics, modes);

/// MIDI Note's module ID in `polysynth.ron`.
const POLYSYNTH_NOTE: ModuleId = ModuleId(1);

/// The Envelope's module ID in `polysynth.ron`.
const POLYSYNTH_ENVELOPE: ModuleId = ModuleId(8);

/// Blocks played before timing: 1 s at 48 kHz and 128 frames, so the
/// envelopes reach their sustain.
const WARM_UP_BLOCKS: usize = 375;

/// Blocks timed: 10 s at 48 kHz and 128 frames.
const TIMED_BLOCKS: usize = 3_750;

/// The share of real time the NFR-PERF-01 budget allows, in percent.
const BUDGET_PERCENT: f64 = 25.0;

/// Criterion's options that take a value, so the word after one is not
/// read as the name filter.
const VALUE_OPTIONS: [&str; 20] = [
    "-c",
    "--color",
    "--colour",
    "-s",
    "--save-baseline",
    "-b",
    "--baseline",
    "--baseline-lenient",
    "--format",
    "--profile-time",
    "--load-baseline",
    "--sample-size",
    "--warm-up-time",
    "--measurement-time",
    "--nresamples",
    "--noise-threshold",
    "--confidence-level",
    "--significance-level",
    "--plotting-backend",
    "--output-format",
];

/// True when the arguments ask for a timed run that includes `name`: they
/// hold `--bench`, hold neither `--list`, `--test`, nor `--ignored`, and
/// hold either no name filter or one that matches `name`. Criterion's help
/// says it skips benchmarks whose names do not contain the filter, so a
/// filter matches when `name` contains it, or equals it with `--exact`. So
/// `cargo test --benches`, a listing, and `cargo bench -- modes` skip it.
/// The filter is read as a plain word, not a regular expression. Criterion
/// reads it as one, so `cargo bench -- 'poly.*'` skips these lines, which
/// do not contain the text `poly.*`, while Criterion still runs
/// `patch/polysynth.ron`.
fn runs(name: &str, args: &[String]) -> bool {
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    if !has("--bench") || has("--list") || has("--test") || has("--ignored") {
        return false;
    }
    let mut filter = None;
    let mut words = args.iter();
    while let Some(arg) = words.next() {
        if arg.starts_with('-') {
            if VALUE_OPTIONS.contains(&arg.as_str()) {
                words.next();
            }
        } else if filter.is_none() {
            filter = Some(arg.as_str());
        }
    }
    match filter {
        None => true,
        Some(filter) if has("--exact") => name == filter,
        Some(filter) => name.contains(filter),
    }
}

/// Plays `parts` with [`POLYSYNTH_NOTES`] held: a warm-up, a check that it
/// is not silent, so a broken patch cannot report a false low number, then
/// [`TIMED_BLOCKS`] blocks timed with one clock. Prints one line, headed
/// `label`, with the mean time per block and its share of the block's real
/// time.
fn time_polysynth(label: &str, mut parts: EngineParts) {
    let config = EngineConfig::default();
    hold(&mut parts, &POLYSYNTH_NOTES);
    let mut left = vec![0.0f32; config.max_block];
    let mut right = vec![0.0f32; config.max_block];
    let mut loudest = 0.0f32;
    for _ in 0..WARM_UP_BLOCKS {
        parts.engine.process(&mut left, &mut right);
        loudest = left.iter().chain(&right).fold(loudest, |most, s| most.max(s.abs()));
    }
    assert!(loudest > 0.01, "{label}: the warm-up is silent (peak {loudest})");
    let start = Instant::now();
    for _ in 0..TIMED_BLOCKS {
        parts.engine.process(&mut left, &mut right);
        black_box(&left);
        black_box(&right);
    }
    let per_block = start.elapsed().as_secs_f64() / TIMED_BLOCKS as f64;
    let real_time = config.max_block as f64 / f64::from(config.sample_rate);
    println!(
        "{label}: 8 voices, 8 notes held, {} kHz, {} frames: {:.1} µs per block, {:.2}% of real time (budget {BUDGET_PERCENT}%)",
        config.sample_rate / 1000.0,
        config.max_block,
        per_block * 1e6,
        100.0 * per_block / real_time,
    );
}

/// The polysynth's cost as a share of real time (polyphony.md, rule 29, and
/// The shipped example): the shipped example, then the variant with the
/// Envelope cabled to MIDI Note's `level` and Mode at Quietest, which closes
/// one feedback loop. Runs only for a timed run that includes `polysynth`
/// ([`runs`]).
fn polysynth_share() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !runs("polysynth", &args) {
        return;
    }
    let parts = example("polysynth.ron");
    let report = parts.controller.last_report();
    assert!(report.loops.is_empty(), "the shipped polysynth has no feedback loop");
    let pitch = parts.controller.output_ref(POLYSYNTH_NOTE, "pitch").unwrap();
    assert_eq!(report.width(pitch).voices, 8, "MIDI Note's pitch is 8 voices");
    time_polysynth("polysynth", parts);

    let mut parts = example("polysynth.ron");
    let c = &mut parts.controller;
    c.connect_named(POLYSYNTH_ENVELOPE, "out", POLYSYNTH_NOTE, "level")
        .unwrap();
    // Quietest, the second of MIDI Note's modes.
    c.set_param_named(POLYSYNTH_NOTE, "mode", 1.0).unwrap();
    let report = c.commit().unwrap();
    assert_eq!(report.loops.len(), 1, "the level cable closes one loop");
    time_polysynth("polysynth (Quietest, level loop)", parts);
}

/// The Voice Assigner's module ID in `polysynth-large.ron`.
const LARGE_ASSIGN: ModuleId = ModuleId(2);

/// The large reference patch's cost as a share of real time (polyphony.md,
/// rule 29, and The large reference patch), with [`POLYSYNTH_NOTES`] held.
/// Runs only for a timed run that includes `polysynth-large` ([`runs`]).
fn polysynth_large_share() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !runs("polysynth-large", &args) {
        return;
    }
    let parts = example("polysynth-large.ron");
    let report = parts.controller.last_report();
    assert!(
        report.loops.is_empty(),
        "the large reference patch has no feedback loop"
    );
    let pitch = parts.controller.output_ref(LARGE_ASSIGN, "pitch").unwrap();
    assert_eq!(report.width(pitch).voices, 8, "the Voice Assigner's pitch is 8 voices");
    time_polysynth("polysynth-large", parts);
}

/// Criterion's groups, then the polysynth's share of real time, then
/// criterion's summary: what `criterion_main!` does, with one call added.
fn main() {
    benches();
    polysynth_share();
    polysynth_large_share();
    Criterion::default().configure_from_args().final_summary();
}
