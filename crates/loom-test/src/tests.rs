use super::*;
use loom_core::{
    Channels, Module, ParamOptions, ParamSpec, PortIo, PortSpec, ProcessContext, SignalType, TransportSettings,
};
use std::path::PathBuf;

/// A port for test descriptors.
const fn port(name: &'static str, signal: SignalType) -> PortSpec {
    PortSpec {
        name,
        label: name,
        signal,
        description: "A test port.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// A parameter for test descriptors.
const fn param(name: &'static str, min: f32, max: f32, default: f32, unit: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min,
        max,
        default,
        unit,
        options: ParamOptions::DEFAULT,
    }
}

/// Output indices of [`ECHO`].
mod out {
    pub const A: usize = 0;
    pub const BAR: usize = 1;
    pub const BEAT: usize = 2;
    pub const PLAY: usize = 3;
    pub const VALUE: usize = 4;
    pub const CONNECTED: usize = 5;
    pub const MIDI: usize = 6;
    pub const GAIN: usize = 7;
}

/// Copies its inputs to its outputs and reports what it sees.
static ECHO: ModuleDescriptor = ModuleDescriptor {
    id: "test.echo",
    version: 1,
    name: "Echo",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Copies inputs to outputs.",
    inputs: &[
        port("a", SignalType::Modulation),
        port("bar", SignalType::Phase).with_default(PortDefault::BarPhase),
        port("beat", SignalType::Phase).with_default(PortDefault::BeatPhase),
        port("play", SignalType::Gate).with_default(PortDefault::Playing),
        port("value", SignalType::Modulation).with_default(PortDefault::Value(0.25)),
    ],
    outputs: &[
        port("a", SignalType::Modulation),
        port("bar", SignalType::Phase),
        port("beat", SignalType::Phase),
        port("play", SignalType::Gate),
        port("value", SignalType::Modulation),
        port("connected", SignalType::Gate),
        port("midi", SignalType::Gate),
        port("gain", SignalType::Modulation),
    ],
    params: &[param("gain", 0.0, 2.0, 1.0, "")],
    role: ModuleRole::Dsp(|_| Box::new(Echo { gain: 0.0 })),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

struct Echo {
    gain: f32,
}

impl Module for Echo {
    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for port in 0..5 {
            let input = io.input(port).to_vec();
            io.output(port).copy_from_slice(&input);
        }
        let connected = if io.is_connected(0) { 1.0 } else { 0.0 };
        io.output(out::CONNECTED).fill(connected);
        let midi = io.output(out::MIDI);
        midi.fill(0.0);
        for (frame, _) in ctx.midi() {
            midi[frame] = 1.0;
        }
        io.output(out::GAIN).fill(self.gain);
    }

    fn set_param(&mut self, _: usize, value: f32) {
        self.gain = value;
    }
}

/// Outputs the length of each call: wrong on purpose.
static CALL_LENGTH: ModuleDescriptor = ModuleDescriptor {
    id: "test.call_length",
    version: 1,
    name: "Call length",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Depends on block size.",
    inputs: &[],
    outputs: &[port("out", SignalType::Modulation)],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(CallLength)),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

struct CallLength;

impl Module for CallLength {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let frames = io.frames() as f32;
        io.output(0).fill(frames);
    }
}

/// Outputs NaN at frame 10.
static NAN_AT_TEN: ModuleDescriptor = ModuleDescriptor {
    id: "test.nan",
    version: 1,
    name: "NaN",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Not finite.",
    inputs: &[],
    outputs: &[port("out", SignalType::Audio)],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(NanAtTen { frame: 0 })),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

struct NanAtTen {
    frame: usize,
}

impl Module for NanAtTen {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for value in io.output(0) {
            *value = if self.frame == 10 { f32::NAN } else { 0.0 };
            self.frame += 1;
        }
    }
}

/// A reference file in the temp folder, removed when the test ends, also
/// when it panics.
struct TempReference(PathBuf);

impl TempReference {
    fn new(name: &str) -> Self {
        Self(std::env::temp_dir().join(format!("loom-test-{}-{name}.wav", std::process::id())))
    }
}

impl Drop for TempReference {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn ramp(offset: f32) -> Harness {
    Harness::new(&ECHO)
        .sample_rate(1_000.0)
        .frames(300)
        .input("a", Signal::from_fn(move |frame| frame as f32 * 0.01 + offset))
}

#[test]
fn renders_inputs_and_parameters_for_the_given_length() {
    let render = Harness::new(&ECHO)
        .sample_rate(1_000.0)
        .seconds(0.5)
        .input("a", Signal::from_fn(|frame| frame as f32))
        .param("gain", 5.0)
        .render();
    assert_eq!(render.frames(), 500);
    assert_eq!(render.sample_rate(), 1_000.0);
    assert!(render.output("a").iter().enumerate().all(|(i, &v)| v == i as f32));
    // Clamped to the range, as the controller clamps it.
    assert!(render.output("gain").iter().all(|&v| v == 2.0));
    assert!(render.outputs()[out::CONNECTED].iter().all(|&v| v == 1.0));
}

#[test]
fn parameters_start_at_their_defaults() {
    let render = Harness::new(&ECHO).frames(10).render();
    assert!(render.output("gain").iter().all(|&v| v == 1.0));
}

#[test]
fn unconnected_inputs_read_their_port_defaults() {
    let settings = TransportSettings {
        tempo: 133.0,
        ..TransportSettings::default()
    };
    let harness = Harness::new(&ECHO).transport(settings).frames(5_000);
    let transport = TransportInfo::new(DEFAULT_SAMPLE_RATE, settings, 0.0);
    for block in BLOCK_SIZES {
        let render = harness.render_with_block(block);
        let outputs = render.outputs();
        let mut bar = outputs[out::BAR].iter().enumerate();
        assert!(bar.all(|(frame, &v)| v == transport.bar_phase_at(frame)));
        let mut beat = outputs[out::BEAT].iter().enumerate();
        assert!(beat.all(|(frame, &v)| v == transport.beat_phase_at(frame)));
        assert!(outputs[out::A].iter().all(|&v| v == 0.0));
        assert!(outputs[out::PLAY].iter().all(|&v| v == 1.0));
        assert!(outputs[out::VALUE].iter().all(|&v| v == 0.25));
        assert!(outputs[out::CONNECTED].iter().all(|&v| v == 0.0));
    }
    harness.check_block_sizes();
}

#[test]
fn a_stopped_transport_holds_still() {
    let render = Harness::new(&ECHO).frames(100).render();
    assert!(render.outputs()[out::BAR].iter().all(|&v| v == 0.0));
    assert!(render.outputs()[out::PLAY].iter().all(|&v| v == 0.0));
}

#[test]
fn midi_lands_on_its_frame_at_every_block_size() {
    let frames = [0, 76, 77, 300, 511, 512, 999];
    let harness = Harness::new(&ECHO)
        .frames(1_000)
        .midi(frames.map(|frame| MidiEvent::note_on(frame, 0, 60, 100)));
    for block in BLOCK_SIZES {
        let render = harness.render_with_block(block);
        let midi = &render.outputs()[out::MIDI];
        for (frame, &value) in midi.iter().enumerate() {
            let expected = if frames.contains(&(frame as u32)) { 1.0 } else { 0.0 };
            assert_eq!(value, expected, "frame {frame}, block size {block}");
        }
    }
    harness.check_block_sizes();
}

#[test]
#[should_panic(expected = "past the end of the render")]
fn midi_past_the_end_fails() {
    Harness::new(&ECHO)
        .frames(10)
        .midi([MidiEvent::note_on(10, 0, 60, 100)])
        .render();
}

#[test]
#[should_panic(expected = "with block size 1, but 512 with block size 512")]
fn block_size_dependence_fails() {
    Harness::new(&CALL_LENGTH).frames(1_000).check_block_sizes();
}

#[test]
#[should_panic(expected = "test.nan: output `out` is NaN at frame 10")]
fn non_finite_output_fails() {
    Harness::new(&NAN_AT_TEN).frames(100).check_block_sizes();
}

#[test]
fn a_written_reference_matches() {
    let file = TempReference::new("matches");
    let harness = ramp(0.0);
    let render = harness.render();
    harness.compare_or_update(&render, &file.0, true);
    harness.compare_or_update(&render, &file.0, false);

    // One channel per output, at the render's sample rate.
    let spec = hound::WavReader::open(&file.0).expect("written").spec();
    assert_eq!(usize::from(spec.channels), ECHO.outputs.len());
    assert_eq!(spec.sample_rate, 1_000);
    assert_eq!(spec.sample_format, hound::SampleFormat::Float);
}

#[test]
fn differences_within_the_tolerance_pass() {
    let file = TempReference::new("within");
    let harness = ramp(0.0);
    harness.compare_or_update(&harness.render(), &file.0, true);
    let close = ramp(5e-6);
    close.compare_or_update(&close.render(), &file.0, false);
}

#[test]
#[should_panic(expected = "test.echo: output `a` differs from")]
fn differences_beyond_the_tolerance_fail() {
    let file = TempReference::new("beyond");
    let harness = ramp(0.0);
    harness.compare_or_update(&harness.render(), &file.0, true);
    let changed = ramp(1e-3);
    changed.compare_or_update(&changed.render(), &file.0, false);
}

#[test]
fn the_tolerance_can_be_set() {
    let file = TempReference::new("tolerance");
    let harness = ramp(0.0);
    harness.compare_or_update(&harness.render(), &file.0, true);
    let changed = ramp(1e-3).tolerance(1e-2);
    changed.compare_or_update(&changed.render(), &file.0, false);
}

#[test]
fn updating_overwrites_a_different_reference() {
    let file = TempReference::new("update");
    let harness = ramp(0.0);
    harness.compare_or_update(&harness.render(), &file.0, true);
    let changed = ramp(1.0);
    let render = changed.render();
    changed.compare_or_update(&render, &file.0, true);
    changed.compare_or_update(&render, &file.0, false);
}

#[test]
#[should_panic(expected = "Create it with `LOOM_UPDATE_REFERENCES=1 cargo test`")]
fn a_missing_reference_fails() {
    let file = TempReference::new("missing");
    let harness = ramp(0.0);
    harness.compare_or_update(&harness.render(), &file.0, false);
}

#[test]
#[should_panic(expected = "300 frames long; the render has 8 outputs at 1000 Hz, 200 frames long")]
fn a_reference_of_another_length_fails() {
    let file = TempReference::new("length");
    let harness = ramp(0.0);
    harness.compare_or_update(&harness.render(), &file.0, true);
    let shorter = ramp(0.0).frames(200);
    shorter.compare_or_update(&shorter.render(), &file.0, false);
}

#[test]
#[should_panic(expected = "at 1000 Hz, 300 frames long; the render has 8 outputs at 2000 Hz")]
fn a_reference_at_another_sample_rate_fails() {
    let file = TempReference::new("rate");
    let harness = ramp(0.0);
    harness.compare_or_update(&harness.render(), &file.0, true);
    let faster = ramp(0.0).sample_rate(2_000.0);
    faster.compare_or_update(&faster.render(), &file.0, false);
}

#[test]
#[should_panic(expected = "test.echo: no input `b`")]
fn unknown_inputs_fail() {
    let _ = Harness::new(&ECHO).input("b", Signal::constant(1.0));
}

#[test]
#[should_panic(expected = "test.echo: no parameter `level`")]
fn unknown_parameters_fail() {
    let _ = Harness::new(&ECHO).param("level", 1.0);
}

#[test]
#[should_panic(expected = "the harness tests DSP modules")]
fn unit_delays_are_refused() {
    static DELAY: ModuleDescriptor = ModuleDescriptor {
        id: "test.delay",
        version: 1,
        name: "Delay",
        category: "Test",
        family: loom_core::Family::Interface,
        description: "",
        inputs: &[port("in", SignalType::Audio)],
        outputs: &[port("out", SignalType::Audio)],
        params: &[],
        role: ModuleRole::UnitDelay,
        panel: loom_core::Panel::EMPTY,
        displays: &[],
    };
    let _ = Harness::new(&DELAY);
}

/// Has a port and a parameter with no description: missing on purpose.
static UNDOCUMENTED: ModuleDescriptor = ModuleDescriptor {
    id: "test.undocumented",
    version: 1,
    name: "Undocumented",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Leaves out port and parameter text.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[port("out", SignalType::Audio)],
    params: &[ParamSpec {
        name: "gain",
        label: "",
        description: " ",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }],
    role: ModuleRole::Dsp(|_| Box::new(CallLength)),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

#[test]
#[should_panic(
    expected = "test.undocumented: input `in` has no description; parameter `gain` has no label; \
                           parameter `gain` has no description. Give every port and parameter"
)]
fn undocumented_ports_and_parameters_fail() {
    let _ = Harness::new(&UNDOCUMENTED).frames(4).render();
}

#[test]
fn undocumented_ports_and_parameters_can_be_allowed() {
    let render = Harness::new(&UNDOCUMENTED).frames(4).allow_undocumented().render();
    assert_eq!(render.frames(), 4);
}

#[test]
fn signals() {
    let phase = Signal::phase(440.0).render(48_000, 48_000.0);
    assert!(phase.iter().all(|&p| (0.0..1.0).contains(&p)));
    assert_eq!(phase[0], 0.0);
    assert!((phase[1] - 440.0 / 48_000.0).abs() < 1e-7);

    let gate = Signal::gate(0.01, 0.02).render(30, 1_000.0);
    let high: Vec<usize> = (0..30).filter(|&i| gate[i] == 1.0).collect();
    assert_eq!(high, (10..20).collect::<Vec<_>>());

    assert_eq!(Signal::constant(0.5).render(3, 1.0), vec![0.5; 3]);
    assert_eq!(Signal::samples([1.0, 2.0, 3.0]).render(2, 1.0), vec![1.0, 2.0]);
}

#[test]
#[should_panic(expected = "Signal::samples has 2 samples, but the render is 3 frames long")]
fn too_few_samples_fail() {
    Signal::samples([1.0, 2.0]).render(3, 1.0);
}

/// Sums its input forever: keeps any NaN it is given. Wrong on purpose.
static SUM: ModuleDescriptor = ModuleDescriptor {
    id: "test.sum",
    version: 1,
    name: "Sum",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Keeps a running sum.",
    inputs: &[port("in", SignalType::Modulation)],
    outputs: &[port("out", SignalType::Modulation)],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Sum(0.0))),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

/// The running sum of [`SUM`].
struct Sum(f32);

impl Module for Sum {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([input], [out]) = io.split::<1, 1>();
        for (out, &x) in out.iter_mut().zip(input) {
            self.0 += x;
            *out = self.0;
        }
    }
}

/// A module that keeps a NaN in its state fails, naming the input, the
/// value, and the output.
#[test]
#[should_panic(expected = "test.sum: after NaN on input `in` (frames 1068 to 1075), output `out` is still NaN")]
fn a_module_that_keeps_a_non_finite_input_fails() {
    Harness::new(&SUM).frames(2_000).check_non_finite_recovery();
}

/// A module with no state passes: its output is finite again as soon as
/// the input is. Unconnected inputs are checked too.
#[test]
fn a_module_without_state_recovers() {
    Harness::new(&ECHO).frames(2_000).check_non_finite_recovery();
    ramp(0.0).frames(2_000).check_non_finite_recovery();
}

/// The render must be long enough to show recovery.
#[test]
#[should_panic(expected = "but the non-finite check needs more than 1556")]
fn a_short_render_cannot_show_recovery() {
    Harness::new(&ECHO).frames(1_000).check_non_finite_recovery();
}

/// The recovery window is 10 ms at any sample rate, so a render at 96 kHz
/// needs 960 recovery frames, not 480.
#[test]
#[should_panic(expected = "but the non-finite check needs more than 2036")]
fn the_recovery_window_scales_with_the_sample_rate() {
    Harness::new(&ECHO)
        .sample_rate(96_000.0)
        .frames(1_900)
        .check_non_finite_recovery();
}

/// Two channels named for a patch's output, for the `check_channels` tests.
fn stereo(offset: f32) -> Vec<Vec<f32>> {
    vec![vec![0.25; 100], (0..100).map(|i| i as f32 * 0.01 + offset).collect()]
}

/// `check_channels` passes against the file `write` made from the same
/// samples (#93).
#[test]
fn check_channels_matches_its_own_file() {
    let file = TempReference::new("stereo");
    let samples = stereo(0.0);
    let channels = reference::Channels {
        id: "test.ron",
        names: vec!["left", "right"],
        samples: &samples,
        sample_rate: 1_000.0,
    };
    reference::write(&channels, &file.0);
    check_channels(
        "test.ron",
        &["left", "right"],
        &samples,
        1_000.0,
        &file.0,
        DEFAULT_TOLERANCE,
    );
}

/// A difference names the patch and the channel by the names given.
#[test]
#[should_panic(expected = "test.ron: output `right` differs from")]
fn a_channel_that_differs_is_named() {
    let file = TempReference::new("stereo-differs");
    let (before, after) = (stereo(0.0), stereo(0.5));
    let channels = |samples| reference::Channels {
        id: "test.ron",
        names: vec!["left", "right"],
        samples,
        sample_rate: 1_000.0,
    };
    reference::write(&channels(&before), &file.0);
    reference::compare(&channels(&after), &file.0, DEFAULT_TOLERANCE);
}

/// Counts frames. `copies` says whether it hands the count over. Its gain
/// glides toward its target by a tenth each frame, and `snaps` says whether
/// `reset` ends the glide.
#[derive(Clone)]
struct Count {
    frames: f32,
    gain: f32,
    target: f32,
    copies: bool,
    snaps: bool,
    clears: bool,
}

impl Count {
    /// A count at 0 with gain 1.
    fn new(copies: bool, snaps: bool, clears: bool) -> Self {
        Self {
            frames: 0.0,
            gain: 1.0,
            target: 1.0,
            copies,
            snaps,
            clears,
        }
    }
}

impl Module for Count {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        for value in io.output(0) {
            self.gain += (self.target - self.gain) * 0.1;
            *value = self.frames * self.gain;
            self.frames += 1.0;
        }
    }

    fn set_param(&mut self, _: usize, value: f32) {
        self.target = value;
    }

    fn reset(&mut self) {
        if self.clears {
            self.frames = 0.0;
        }
        if self.snaps {
            self.gain = self.target;
        }
    }

    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        self.copies && loom_core::copy_state(self, other)
    }
}

/// A [`Count`] descriptor with the given copying, snapping, and clearing.
macro_rules! count {
    ($name:ident, $id:literal, $copies:literal, $snaps:literal, $clears:literal) => {
        static $name: ModuleDescriptor = ModuleDescriptor {
            id: $id,
            version: 1,
            name: "Count",
            category: "Test",
            family: loom_core::Family::Interface,
            description: "Counts frames.",
            inputs: &[],
            outputs: &[port("out", SignalType::Modulation)],
            params: &[param("gain", 0.0, 4.0, 1.0, "")],
            role: ModuleRole::Dsp(|_| Box::new(Count::new($copies, $snaps, $clears))),
            panel: loom_core::Panel::EMPTY,
            displays: &[],
        };
    };
}

count!(COUNT, "test.count", true, true, true);
count!(COUNT_NO_COPY, "test.count-no-copy", false, true, true);
count!(COUNT_NO_SNAP, "test.count-no-snap", true, false, true);
count!(COUNT_NO_CLEAR, "test.count-no-clear", true, true, false);

/// A module that hands over all its state passes the copy check and says
/// it copied.
#[test]
fn a_module_that_copies_its_state_passes() {
    assert!(
        Harness::new(&COUNT)
            .frames(200)
            .param("gain", 2.0)
            .check_copy_state(100)
    );
}

/// A module with state that does not copy it fails, naming the frame.
#[test]
#[should_panic(expected = "test.count-no-copy: output `out` at frame 100")]
fn a_module_that_keeps_its_state_fails_the_copy_check() {
    Harness::new(&COUNT_NO_COPY).frames(200).check_copy_state(100);
}

/// A module whose `reset` clears its count and snaps its glide passes.
#[test]
fn a_reset_that_clears_and_snaps_passes() {
    Harness::new(&COUNT).frames(200).param("gain", 2.0).check_reset();
}

/// A module that still glides after `reset` fails the reset check.
#[test]
#[should_panic(expected = "test.count-no-snap: output `out` at frame 1")]
fn a_reset_that_leaves_a_glide_fails() {
    Harness::new(&COUNT_NO_SNAP)
        .frames(200)
        .param("gain", 2.0)
        .check_reset();
}

/// A module that keeps what `process` changed fails the reset check, on the
/// first frame, which is what a player would hear after a panic.
#[test]
#[should_panic(expected = "test.count-no-clear: output `out` at frame 0")]
fn a_reset_that_keeps_state_fails() {
    Harness::new(&COUNT_NO_CLEAR).frames(200).check_reset();
}

/// The copy check names the block size, so a failure at one size is easy to
/// reproduce. Block size 1 runs first.
#[test]
#[should_panic(expected = "(block size 1, `copy_state_from` returned false)")]
fn the_copy_check_runs_at_block_size_one() {
    Harness::new(&COUNT_NO_COPY).frames(200).check_copy_state(100);
}

/// A phase counted from its value at the last rising edge of `reset`. With
/// `wraps` off, it stores the input as it comes, as Phase Reset once did: a huge
/// stored value leaves no fraction in later ones (#552).
#[derive(Clone)]
struct PhaseOffset {
    wraps: bool,
    offset: f32,
    high: bool,
}

impl Module for PhaseOffset {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([phase, reset], [out]) = io.split::<2, 1>();
        for ((out, &phase), &reset) in out.iter_mut().zip(phase).zip(reset) {
            let phase = if self.wraps { phase - phase.floor() } else { phase };
            if reset > 0.5 && !self.high {
                self.offset = phase;
            }
            self.high = reset > 0.5;
            let x = phase - self.offset;
            *out = x - x.floor();
        }
    }
}

/// A [`PhaseOffset`] descriptor that wraps its input or not.
macro_rules! phase_offset {
    ($name:ident, $id:literal, $wraps:literal) => {
        static $name: ModuleDescriptor = ModuleDescriptor {
            id: $id,
            version: 1,
            name: "Phase Offset",
            category: "Test",
            family: loom_core::Family::Interface,
            description: "Counts a phase from the last reset.",
            inputs: &[port("phase", SignalType::Phase), port("reset", SignalType::Gate)],
            outputs: &[port("phase", SignalType::Phase)],
            params: &[],
            role: ModuleRole::Dsp(|_| {
                Box::new(PhaseOffset {
                    wraps: $wraps,
                    offset: 0.0,
                    high: false,
                })
            }),
            panel: loom_core::Panel::EMPTY,
            displays: &[],
        };
    };
}

phase_offset!(PHASE_OFFSET, "test.phase-offset", true);
phase_offset!(STICKY_OFFSET, "test.sticky-offset", false);

/// A harness for a phase offset module on the extreme check's slow signal.
fn on_extreme_busy(descriptor: &'static ModuleDescriptor) -> Harness {
    Harness::new(descriptor)
        .frames(EXTREME_RENDER)
        .input("phase", Signal::extreme_busy())
        .input("reset", Signal::extreme_busy())
}

/// A module that keeps a captured phase small passes, though its offset differs
/// from a clean render's: the phase moves at the same speed.
#[test]
fn a_wrapped_capture_passes_the_extreme_check() {
    on_extreme_busy(&PHASE_OFFSET).check_extreme_recovery();
}

/// The first bug in Phase Reset (#531): subtracting a huge value of the other
/// sign overflows, and wrapping it gives NaN. A hold of one sign misses this,
/// because the stored value and the input cancel.
#[test]
#[should_panic(expected = "not finite")]
fn a_stored_huge_value_that_overflows_fails_the_extreme_check() {
    on_extreme_busy(&STICKY_OFFSET).check_extreme_stays_bounded_on("phase");
}

/// The second bug in Phase Reset (#531): after a huge capture, ordinary inputs
/// lose their fraction, so the output stops moving. Its values stay inside 0 to
/// 1, so only the comparison sees this, which needs the next edge to come after
/// it. The stored `f32::MAX` is held for the first pass, which has no overflow.
#[test]
#[should_panic(expected = "after 3.4028235e38 then 3.4028235e38 on input `phase`, output `phase`")]
fn a_stored_huge_value_that_loses_the_fraction_fails_the_extreme_check() {
    on_extreme_busy(&STICKY_OFFSET).check_extreme_recovery();
}

/// The check for one input looks at that input only, so a fault on another stays
/// out of it. The sticky module's faults are on `phase`.
#[test]
fn the_check_for_one_input_leaves_the_others_alone() {
    on_extreme_busy(&STICKY_OFFSET).check_extreme_recovery_on("reset");
}

/// A name that is no input fails, so a table of exceptions cannot go stale.
#[test]
#[should_panic(expected = "test.phase-offset: there is no input `nope`")]
fn the_check_for_one_input_needs_a_real_input() {
    on_extreme_busy(&PHASE_OFFSET).check_extreme_recovery_on("nope");
}

/// A render too short to show the next edge fails, so the check cannot be
/// shortened by accident.
#[test]
#[should_panic(expected = "the extreme check needs 48000")]
fn the_extreme_check_needs_its_whole_render() {
    Harness::new(&PHASE_OFFSET).frames(1_000).check_extreme_recovery();
}

/// A module with an output and no inputs.
static NO_INPUTS: ModuleDescriptor = ModuleDescriptor {
    id: "test.no-inputs",
    version: 1,
    name: "No Inputs",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Has no inputs.",
    inputs: &[],
    outputs: &[port("out", SignalType::Modulation)],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Silent)),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

/// Writes silence, which is what a new output buffer holds.
#[derive(Clone)]
struct Silent;

impl Module for Silent {
    fn process(&mut self, _: &ProcessContext<'_>, _: &mut PortIo<'_>) {}
}

/// Stores `in` on each rising edge of `trigger`, with or without a guard against
/// a non-finite value, as Sample and Hold does.
#[derive(Clone)]
struct Capture {
    guarded: bool,
    held: f32,
    high: bool,
}

impl Module for Capture {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([input, trigger], [out]) = io.split::<2, 1>();
        for ((out, &input), &trigger) in out.iter_mut().zip(input).zip(trigger) {
            if trigger > 0.5 && !self.high && (!self.guarded || input.is_finite()) {
                self.held = input;
            }
            self.high = trigger > 0.5;
            *out = self.held;
        }
    }
}

/// A [`Capture`] descriptor, guarded or not.
macro_rules! capture {
    ($name:ident, $id:literal, $guarded:literal) => {
        static $name: ModuleDescriptor = ModuleDescriptor {
            id: $id,
            version: 1,
            name: "Capture",
            category: "Test",
            family: loom_core::Family::Interface,
            description: "Stores its input on a trigger.",
            inputs: &[
                port("in", SignalType::Modulation),
                port("trigger", SignalType::Gate),
            ],
            outputs: &[port("out", SignalType::Modulation)],
            params: &[],
            role: ModuleRole::Dsp(|_| {
                Box::new(Capture {
                    guarded: $guarded,
                    held: 0.0,
                    high: false,
                })
            }),
            panel: loom_core::Panel::EMPTY,
            displays: &[],
        };
    };
}

capture!(GUARDED_CAPTURE, "test.guarded-capture", true);
capture!(UNGUARDED_CAPTURE, "test.unguarded-capture", false);

/// A harness for a capture module on the non-finite check's signal.
fn on_non_finite_busy(descriptor: &'static ModuleDescriptor) -> Harness {
    Harness::new(descriptor)
        .frames(4_000)
        .input("in", Signal::non_finite_busy())
        .input("trigger", Signal::non_finite_busy())
}

/// A module that stores a bad input on another input's edge fails, which is the
/// gap #564 closes. The bad frames must cover the edge for this to show.
#[test]
#[should_panic(expected = "test.unguarded-capture: after NaN on input `in`")]
fn a_module_that_stores_a_bad_input_on_a_trigger_fails() {
    on_non_finite_busy(&UNGUARDED_CAPTURE).check_non_finite_recovery();
}

/// The check's timing holds at the rates up to its limit, and not above it: at 96
/// kHz the next edge overwrites the bad value before the recovery time ends, so
/// this module passes. The spec lists this as a known limit.
#[test]
fn the_non_finite_signal_covers_rates_below_its_limit() {
    let check = |rate: f32| {
        let harness = on_non_finite_busy(&UNGUARDED_CAPTURE).sample_rate(rate);
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| harness.check_non_finite_recovery())).is_err()
    };
    assert!(check(44_100.0));
    assert!(check(88_200.0));
    assert!(
        !check(96_000.0),
        "the limit moved, so update the spec and the signal's comment"
    );
}

/// With the guard, the same module passes.
#[test]
fn a_module_that_guards_what_it_stores_passes() {
    on_non_finite_busy(&GUARDED_CAPTURE).check_non_finite_recovery();
}

/// Multiplies its two inputs, with no limit on the result.
#[derive(Clone)]
struct Product;

impl Module for Product {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([a, b], [out]) = io.split::<2, 1>();
        for ((out, &a), &b) in out.iter_mut().zip(a).zip(b) {
            *out = a * b;
        }
    }
}

/// The module for [`Product`].
static PRODUCT: ModuleDescriptor = ModuleDescriptor {
    id: "test.product",
    version: 1,
    name: "Product",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Multiplies two inputs.",
    inputs: &[port("a", SignalType::Modulation), port("b", SignalType::Modulation)],
    outputs: &[port("out", SignalType::Modulation)],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(Product)),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

/// A harness for [`Product`] on the extreme check's slow signal.
fn product() -> Harness {
    Harness::new(&PRODUCT)
        .frames(EXTREME_RENDER)
        .input("a", Signal::extreme_busy())
        .input("b", Signal::extreme_busy())
}

/// One huge input at a time stays finite here, which is the gap #575 closes.
#[test]
fn a_product_passes_with_one_huge_input_at_a_time() {
    product().check_extreme_recovery();
}

/// Both inputs huge overflow, and the failure says every input took the value.
#[test]
#[should_panic(expected = "test.product: after 3.4028235e38 then 3.4028235e38 on every input at once, output `out`")]
fn a_product_that_overflows_fails_with_every_input_huge() {
    product().check_extreme_recovery_all();
}

/// Each level of the all-input check looks for the overflow.
#[test]
#[should_panic(expected = "not finite")]
fn the_finite_level_sees_an_overflow_with_every_input_huge() {
    product().check_extreme_finite_all();
}

/// Each of the other all-input levels sees it too.
#[test]
#[should_panic(expected = "not finite")]
fn the_bounded_level_sees_an_overflow_with_every_input_huge() {
    product().check_extreme_stays_bounded_all();
}

/// A module with no inputs has nothing to send a huge value to, so it passes.
#[test]
fn the_all_input_check_passes_a_module_with_no_inputs() {
    Harness::new(&NO_INPUTS)
        .frames(EXTREME_RENDER)
        .check_extreme_recovery_all();
}

/// A harness that stores a huge input on one edge, and never again.
fn capture_once() -> Harness {
    Harness::new(&UNGUARDED_CAPTURE)
        .frames(EXTREME_RENDER)
        .input("trigger", Signal::gate(0.06, 0.07))
}

/// A value held for good stays finite, so the finite level passes it. That level
/// is for a module with its parameters turned up, which may ring for a long time.
#[test]
fn the_finite_level_allows_a_value_that_stays_huge() {
    capture_once().check_extreme_finite();
}

/// The bounded level refuses the same value.
#[test]
#[should_panic(expected = "it stayed huge")]
fn the_bounded_level_refuses_a_value_that_stays_huge() {
    capture_once().check_extreme_stays_bounded_on("in");
}

/// Passes one of its two inputs. Its copy takes the wrong one, which is the bug
/// the shared copy check missed with one signal on every input (#564).
#[derive(Clone)]
struct WrongRouteOnCopy {
    second: bool,
}

impl Module for WrongRouteOnCopy {
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        let copied = loom_core::copy_state(self, other);
        self.second = !self.second;
        copied
    }

    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([a, b], [out]) = io.split::<2, 1>();
        out.copy_from_slice(if self.second { b } else { a });
    }
}

/// The module for [`WrongRouteOnCopy`].
static WRONG_ROUTE: ModuleDescriptor = ModuleDescriptor {
    id: "test.wrong-route",
    version: 1,
    name: "Wrong Route",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Passes the wrong input after a copy.",
    inputs: &[port("a", SignalType::Modulation), port("b", SignalType::Modulation)],
    outputs: &[port("out", SignalType::Modulation)],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(WrongRouteOnCopy { second: false })),
    panel: loom_core::Panel::EMPTY,
    displays: &[],
};

/// With one signal on every input, a wrong route gives the same output, so the
/// check passes the bug. This is why the shared checks give each input its own.
#[test]
fn one_signal_on_every_input_hides_a_wrong_route() {
    let harness = Harness::new(&WRONG_ROUTE)
        .frames(200)
        .input("a", Signal::phase(100.0))
        .input("b", Signal::phase(100.0));
    assert!(harness.check_copy_state(100));
}

/// With each input at its own frequency, the same bug fails.
#[test]
#[should_panic(expected = "test.wrong-route: output `out`")]
fn each_input_at_its_own_frequency_shows_a_wrong_route() {
    Harness::new(&WRONG_ROUTE)
        .frames(200)
        .input("a", Signal::phase(100.0))
        .input("b", Signal::phase(130.0))
        .check_copy_state(100);
}

/// Seconds for frame `frame` at the default sample rate. Frame 5003 is one no block
/// size lands on.
fn at(frame: usize) -> f64 {
    frame as f64 / f64::from(DEFAULT_SAMPLE_RATE)
}

/// A parameter change lands on its frame at every block size.
#[test]
fn a_parameter_change_lands_on_its_frame() {
    let render = Harness::new(&ECHO)
        .frames(6_000)
        .param_at(at(5_003), "gain", 2.0)
        .check_block_sizes();
    let gain = render.output("gain");
    assert_ne!(gain[5_002], gain[5_003]);
    assert_eq!(gain[5_003], 2.0);
}

/// Changes on one frame apply in the order they were added.
#[test]
fn the_last_change_on_a_frame_wins() {
    let render = Harness::new(&ECHO)
        .frames(6_000)
        .param_at(at(5_003), "gain", 2.0)
        .param_at(at(5_003), "gain", 1.5)
        .check_block_sizes();
    assert_eq!(render.output("gain")[5_003], 1.5);
}

/// A change past the end of the render fails, so a test cannot lose one.
#[test]
#[should_panic(expected = "test.echo: a scheduled change at frame 6000 is past the end of the render (6000 frames)")]
fn a_change_past_the_end_fails() {
    Harness::new(&ECHO)
        .frames(6_000)
        .param_at(at(6_000), "gain", 2.0)
        .render();
}

/// An unknown name fails as `.param` does.
#[test]
#[should_panic(expected = "test.echo: no parameter `nope`")]
fn a_change_needs_a_real_parameter() {
    let _ = Harness::new(&ECHO).param_at(0.0, "nope", 1.0);
}

/// A reset cannot replay a schedule, so the check says so.
#[test]
#[should_panic(expected = "`check_reset` does not support")]
fn the_reset_check_refuses_scheduled_changes() {
    Harness::new(&ECHO)
        .frames(200)
        .param_at(at(100), "gain", 2.0)
        .check_reset();
}

/// A copy cannot replay a schedule, so the check says so.
#[test]
#[should_panic(expected = "`check_copy_state` does not support")]
fn the_copy_check_refuses_scheduled_changes() {
    Harness::new(&ECHO)
        .frames(200)
        .transport_at(at(50), TransportSettings::default())
        .check_copy_state(100);
}

/// A playing transport at `tempo`, in 4/4.
fn at_tempo(tempo: f64) -> TransportSettings {
    TransportSettings {
        tempo,
        ..TransportSettings::default()
    }
}

/// A tempo change keeps the position: the beat phase moves on from where it was,
/// at the new speed. The unconnected input follows the same timeline.
#[test]
fn a_tempo_change_keeps_the_position() {
    let render = Harness::new(&ECHO)
        .frames(6_000)
        .transport(at_tempo(120.0))
        .transport_at(at(5_003), at_tempo(60.0))
        .check_block_sizes();
    let beat = render.output("beat");
    let (before, after) = (beat[5_002] - beat[5_001], beat[5_004] - beat[5_003]);
    assert!((before / after - 2.0).abs() < 1e-2, "{before} then {after}");
    assert!((beat[5_003] - beat[5_002]).abs() < 1e-3);
}

/// Stopping holds the position, and the playing input follows.
#[test]
fn a_stop_holds_the_position() {
    let stopped = TransportSettings {
        running: false,
        ..TransportSettings::default()
    };
    let render = Harness::new(&ECHO)
        .frames(6_000)
        .transport(TransportSettings::default())
        .transport_at(at(5_003), stopped)
        .check_block_sizes();
    let (beat, play) = (render.output("beat"), render.output("play"));
    assert_eq!((play[5_002], play[5_003]), (1.0, 0.0));
    assert_eq!(beat[5_003], beat[5_999]);
}

// The recovery time ends before the next edge, or the edge could hide a bad
// value. 480 frames is RECOVERY_SECONDS at 48 kHz.
const _: () = assert!(BAD_START <= BAD_EDGE && BAD_EDGE < BAD_START + BAD_FRAMES);
const _: () = assert!(BAD_START + BAD_FRAMES + 480 < BAD_NEXT_EDGE);

/// The wave must rise inside the bad frames, and rise again after the recovery.
#[test]
fn the_non_finite_timing_covers_the_edges() {
    let wave = Signal::non_finite_busy().render(BAD_NEXT_EDGE + 1, DEFAULT_SAMPLE_RATE);
    let edges: Vec<usize> = (1..wave.len())
        .filter(|&i| wave[i] > 0.5 && wave[i - 1] <= 0.5)
        .collect();
    assert_eq!(edges, [112, BAD_EDGE, BAD_NEXT_EDGE]);
}

// The settling time ends before the next edge, or nothing is compared.
const _: () = assert!(EXTREME_START + EXTREME_FRAMES + EXTREME_SETTLE < EXTREME_NEXT_EDGE);

/// Holding the value across the edge is what makes a capture see it: the wave
/// must rise inside the hold, and the second rise must come after the settling.
#[test]
fn the_extreme_timing_covers_the_edges() {
    let wave = Signal::extreme_busy().render(EXTREME_RENDER, DEFAULT_SAMPLE_RATE);
    let edges: Vec<usize> = (1..wave.len())
        .filter(|&i| wave[i] > 0.5 && wave[i - 1] <= 0.5)
        .collect();
    assert_eq!(edges.len(), 2, "{edges:?}");
    assert!((EXTREME_START..EXTREME_FLIP).contains(&edges[0]), "{edges:?}");
    assert!(wave[EXTREME_START] <= 0.5);
    assert_eq!(edges[1], EXTREME_NEXT_EDGE);
}

/// The repeatable check passes for a module that copies the same way twice.
#[test]
fn a_repeatable_copy_passes() {
    Harness::new(&COUNT).frames(200).check_copy_state_repeatable(100);
}

/// The repeatable check fails when nothing was copied.
#[test]
#[should_panic(expected = "test.count-no-copy: `copy_state_from` returned false")]
fn a_repeatable_check_needs_a_copy() {
    Harness::new(&COUNT_NO_COPY)
        .frames(200)
        .check_copy_state_repeatable(100);
}
