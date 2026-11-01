use super::*;
use loom_core::{Channels, Module, ParamOptions, ParamSpec, PortIo, PortSpec, ProcessContext, SignalType};
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
#[should_panic(expected = "test.sum: after NaN on input `in` (frames 1000 to 1007), output `out` is still NaN")]
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
#[should_panic(expected = "but the non-finite check needs more than 1488")]
fn a_short_render_cannot_show_recovery() {
    Harness::new(&ECHO).frames(1_000).check_non_finite_recovery();
}

/// The recovery window is 10 ms at any sample rate, so a render at 96 kHz
/// needs 960 recovery frames, not 480.
#[test]
#[should_panic(expected = "but the non-finite check needs more than 1968")]
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
}

impl Count {
    /// A count at 0 with gain 1.
    fn new(copies: bool, snaps: bool) -> Self {
        Self {
            frames: 0.0,
            gain: 1.0,
            target: 1.0,
            copies,
            snaps,
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
        self.frames = 0.0;
        if self.snaps {
            self.gain = self.target;
        }
    }

    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        self.copies && loom_core::copy_state(self, other)
    }
}

/// A [`Count`] descriptor with the given copying and snapping.
macro_rules! count {
    ($name:ident, $id:literal, $copies:literal, $snaps:literal) => {
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
            role: ModuleRole::Dsp(|_| Box::new(Count::new($copies, $snaps))),
            panel: loom_core::Panel::EMPTY,
            displays: &[],
        };
    };
}

count!(COUNT, "test.count", true, true);
count!(COUNT_NO_COPY, "test.count-no-copy", false, true);
count!(COUNT_NO_SNAP, "test.count-no-snap", true, false);

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

/// A module whose `reset` snaps its glide passes the reset check.
#[test]
fn a_reset_that_snaps_passes() {
    Harness::new(&COUNT)
        .frames(200)
        .param("gain", 2.0)
        .check_reset_snaps_smoothing();
}

/// A module that still glides after `reset` fails the reset check.
#[test]
#[should_panic(expected = "test.count-no-snap: output `out` at frame 1")]
fn a_reset_that_leaves_a_glide_fails() {
    Harness::new(&COUNT_NO_SNAP)
        .frames(200)
        .param("gain", 2.0)
        .check_reset_snaps_smoothing();
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
