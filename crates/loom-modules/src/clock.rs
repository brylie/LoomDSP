//! Clock: a phase, trigger, and gate at a musical rate (MT-CLOCK-02,
//! FR-LIB-27).
//!
//! The phase is computed from the transport's absolute position, never by
//! adding a step per frame. So it stays aligned with bar 1 after a stop, a
//! tempo change, or a reset, and it is the same at any block size.
//! Spec: `docs/design/clock.md`, the Clock row of Modules and Clock details.
//! Background: `docs/learn/phase-and-cycles.md`.

use loom_core::signal::{WrapDetector, trigger};
use loom_core::{
    Channels, DisplayKind, DisplaySpec, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, TriggerFlag, Widget,
};

/// The rate names, in order from 0. Each length comes straight, dotted, and
/// triplet, from 4 bars down to 1/32.
const RATE_NAMES: &[&str] = &[
    "4 bars",
    "4 bars dotted",
    "4 bars triplet",
    "2 bars",
    "2 bars dotted",
    "2 bars triplet",
    "1 bar",
    "1 bar dotted",
    "1 bar triplet",
    "1/2",
    "1/2 dotted",
    "1/2 triplet",
    "1/4",
    "1/4 dotted",
    "1/4 triplet",
    "1/8",
    "1/8 dotted",
    "1/8 triplet",
    "1/16",
    "1/16 dotted",
    "1/16 triplet",
    "1/32",
    "1/32 dotted",
    "1/32 triplet",
];

/// Index of the default rate, 1/4.
const QUARTER: usize = 12;

/// `loom.clock.clock`: a phase at a musical rate, read from the transport,
/// with a trigger on each wrap and a gate for the first part of each cycle.
pub static CLOCK: ModuleDescriptor = ModuleDescriptor {
    id: "loom.clock.clock",
    version: 1,
    name: "Clock",
    category: "Clock",
    description: "A phase, trigger, and gate at a musical rate, locked to the transport's bar 1.",
    inputs: &[],
    outputs: &[
        PortSpec {
            name: "phase",
            label: "Phase",
            signal: SignalType::Phase,
            description: "Rises from 0 to 1 once per cycle of the rate, counted from bar 1.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "trigger",
            label: "Trigger",
            signal: SignalType::Gate,
            description: "Fires once at the start of each cycle while the transport runs.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "gate",
            label: "Gate",
            signal: SignalType::Gate,
            description: "High for the first part of each cycle, as long as the gate width says.",
            default: PortDefault::Value(0.0),
            range: loom_core::PortRange::Signal,
            channels: Channels::PerChannel,
        },
    ],
    params: &[
        ParamSpec {
            name: "rate",
            label: "Rate",
            description: "The length of one cycle: 4 bars to 1/32, straight, dotted, or triplet.",
            min: 0.0,
            max: 23.0,
            default: QUARTER as f32,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(RATE_NAMES),
        ParamSpec {
            name: "width",
            label: "Width",
            description: "How much of each cycle the gate is high.",
            min: 1.0,
            max: 99.0,
            default: 50.0,
            unit: "%",
            options: ParamOptions::DEFAULT,
        },
    ],
    role: ModuleRole::Dsp(|_| Box::new(Clock::new())),
    panel: Panel::new(
        6,
        5,
        &[
            Widget::knob(0, 0, "rate"),
            Widget::knob(3, 0, "width"),
            Widget::light(0, 3, "fired"),
        ],
    ),
    displays: &[DisplaySpec {
        name: "fired",
        label: "Fired",
        description: "Flashes on each trigger.",
        kind: DisplayKind::Trigger,
    }],
};

/// Index of `rate`.
const RATE: usize = 0;
/// Index of `width`.
const WIDTH: usize = 1;

/// The length of one cycle of rate `index`, in quarter-note beats.
///
/// Bar rates follow the time signature; note rates do not, so 1/4 is one
/// beat in any metre. Dotted is 3/2 of straight, triplet 2/3.
fn rate_beats(index: usize, beats_per_bar: f64) -> f64 {
    let straight = match index / 3 {
        0 => 4.0 * beats_per_bar,
        1 => 2.0 * beats_per_bar,
        2 => beats_per_bar,
        3 => 2.0,
        4 => 1.0,
        5 => 0.5,
        6 => 0.25,
        _ => 0.125,
    };
    match index % 3 {
        0 => straight,
        1 => straight * 1.5,
        _ => straight * 2.0 / 3.0,
    }
}

/// A rate parameter value as an index into [`RATE_NAMES`]. NaN gives 0.
fn rate_index(value: f32) -> usize {
    (value.round().max(0.0) as usize).min(RATE_NAMES.len() - 1)
}

/// State for `loom.clock.clock`.
///
/// The phase needs no state: it comes from the transport. The state is for
/// the trigger, which must know the previous frame.
#[derive(Clone)]
struct Clock {
    rate: usize,
    width: f32,
    /// The rate the last frame used. A change re-arms the wrap detector.
    active_rate: usize,
    wrap: WrapDetector,
    /// Whether the trigger fired since the light last read it.
    fired: TriggerFlag,
}

impl Clock {
    fn new() -> Self {
        Self {
            rate: QUARTER,
            width: 50.0,
            active_rate: QUARTER,
            wrap: WrapDetector::new(),
            fired: TriggerFlag::default(),
        }
    }
}

impl Module for Clock {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let transport = ctx.transport();
        let playing = transport.playing();
        let beats_per_bar = transport.beats_per_bar();
        // A jump, or a stopped transport, starts the wrap rule afresh: the
        // next playing frame fires only when it is exactly on a cycle start.
        // So a reset fires once on bar 1 (rule 8), and starting again after
        // a stop does not fire a trigger halfway through a cycle.
        if transport.jumped() || !playing {
            self.wrap.reset();
        }
        let (rate_mod, width_mod) = (io.param(RATE), io.param(WIDTH));
        let ([], [phase_out, trigger_out, gate_out]) = io.split::<0, 3>();
        for i in 0..phase_out.len() {
            // A modulated value counts for this frame only.
            let rate = rate_mod.map_or(i, self.rate, rate_index);
            if rate != self.active_rate {
                // A new rate moves the phase at once. Re-arm, so the move
                // itself never fires a trigger.
                self.active_rate = rate;
                self.wrap.reset();
            }
            let phase = transport.phase_at(i, rate_beats(rate, beats_per_bar));
            let fired = playing && self.wrap.wrapped(phase);
            self.fired.record(fired);
            let width = width_mod.at_or(i, self.width).clamp(1.0, 99.0) * 0.01;
            phase_out[i] = phase;
            trigger_out[i] = trigger(fired);
            gate_out[i] = if phase < width { 1.0 } else { 0.0 };
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            RATE => self.rate = rate_index(value),
            WIDTH => self.width = value,
            _ => {}
        }
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.fired.take();
    }

    fn reset(&mut self) {
        self.wrap.reset();
        self.fired.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::testing::run_with_transport;
    use loom_core::{ModuleConfig, TimeSignature, TransportInfo, TransportSettings};

    const SAMPLE_RATE: f32 = 48_000.0;
    /// Frames per beat at 120 BPM and 48 kHz.
    const BEAT: usize = 24_000;

    /// A new Clock with its defaults and then `params` set.
    fn clock(params: &[(usize, f32)]) -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = CLOCK.role else {
            unreachable!("the Clock is a DSP module")
        };
        let mut module = create(&ModuleConfig {
            sample_rate: SAMPLE_RATE,
            max_block: 1024,
        });
        for (index, param) in CLOCK.params.iter().enumerate() {
            module.set_param(index, param.default);
        }
        for &(index, value) in params {
            module.set_param(index, value);
        }
        module
    }

    /// Runs one call of `frames` frames and returns phase, trigger, gate.
    fn call(module: &mut dyn Module, transport: TransportInfo, frames: usize) -> [Vec<f32>; 3] {
        let mut outputs = [vec![0.0; frames], vec![0.0; frames], vec![0.0; frames]];
        {
            let [a, b, c] = &mut outputs;
            let mut slices: Vec<&mut [f32]> = vec![a, b, c];
            run_with_transport(module, SAMPLE_RATE, transport, &[], &[], &mut slices, &[]);
        }
        outputs
    }

    /// Frames where `signal` is high.
    fn highs(signal: &[f32]) -> Vec<usize> {
        signal
            .iter()
            .enumerate()
            .filter(|&(_, &x)| x > 0.5)
            .map(|(i, _)| i)
            .collect()
    }

    fn running(time_signature: TimeSignature) -> TransportSettings {
        TransportSettings {
            time_signature,
            ..TransportSettings::default()
        }
    }

    /// Rates in beats: bars follow the metre, notes do not.
    #[test]
    fn rates_in_beats() {
        assert_eq!(rate_beats(0, 4.0), 16.0);
        assert_eq!(rate_beats(6, 3.5), 3.5);
        assert_eq!(rate_beats(7, 4.0), 6.0);
        assert_eq!(rate_beats(QUARTER, 3.5), 1.0);
        assert_eq!(rate_beats(13, 4.0), 1.5);
        assert!((rate_beats(14, 4.0) - 2.0 / 3.0).abs() < 1e-15);
        assert_eq!(rate_beats(21, 4.0), 0.125);
        assert_eq!(RATE_NAMES.len(), 24);
        assert_eq!(RATE_NAMES[QUARTER], "1/4");
        assert_eq!(rate_index(f32::NAN), 0);
        assert_eq!(rate_index(99.0), 23);
    }

    /// A playing transport from bar 1 fires on frame 0 and on every beat,
    /// and the gate is high for the first half of each beat.
    #[test]
    fn quarter_notes_fire_on_each_beat() {
        let mut module = clock(&[]);
        let transport = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.0);
        let [phase, trig, gate] = call(module.as_mut(), transport, 4 * BEAT + 1);
        assert_eq!(highs(&trig), [0, BEAT, 2 * BEAT, 3 * BEAT, 4 * BEAT]);
        assert_eq!(phase[BEAT / 2], 0.5);
        assert_eq!(gate[0], 1.0);
        assert_eq!(gate[BEAT / 2 - 1], 1.0);
        assert_eq!(gate[BEAT / 2], 0.0);
        assert_eq!(highs(&gate).len(), 4 * BEAT / 2 + 1);
    }

    /// The gate width sets the high part of each cycle.
    #[test]
    fn gate_width_sets_the_high_part() {
        let mut module = clock(&[(WIDTH, 25.0)]);
        let transport = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.0);
        let [_, _, gate] = call(module.as_mut(), transport, BEAT);
        assert_eq!(highs(&gate).len(), BEAT / 4);
    }

    /// A module added halfway through a cycle does not fire until the next
    /// cycle starts.
    #[test]
    fn joining_mid_cycle_waits_for_the_next_cycle() {
        let mut module = clock(&[]);
        let transport = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.5);
        let [_, trig, _] = call(module.as_mut(), transport, BEAT);
        assert_eq!(highs(&trig), [BEAT / 2]);
    }

    /// A stopped transport fires nothing and holds the phase, even across a
    /// reset. Starting from bar 1 fires once, on the first frame.
    #[test]
    fn stopped_transport_holds_and_start_from_bar_one_fires_once() {
        let mut module = clock(&[]);
        let stopped = TransportSettings {
            running: false,
            ..TransportSettings::default()
        };
        let held = TransportInfo::new(SAMPLE_RATE, stopped, 2.25);
        let [phase, trig, _] = call(module.as_mut(), held, 256);
        assert!(phase.iter().all(|&p| p == 0.25));
        assert!(highs(&trig).is_empty());
        // A reset while stopped jumps to 0, but nothing fires yet.
        let reset = TransportInfo::new(SAMPLE_RATE, stopped, 0.0).with_jump(true);
        let [phase, trig, _] = call(module.as_mut(), reset, 256);
        assert!(phase.iter().all(|&p| p == 0.0));
        assert!(highs(&trig).is_empty());
        // Start: bar 1 fires on its first frame.
        let start = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.0);
        let [_, trig, _] = call(module.as_mut(), start, 256);
        assert_eq!(highs(&trig), [0]);
    }

    /// Starting again where a stop left off, mid-cycle, fires no trigger.
    #[test]
    fn continuing_mid_cycle_does_not_fire() {
        let mut module = clock(&[]);
        let stopped = TransportSettings {
            running: false,
            ..TransportSettings::default()
        };
        call(module.as_mut(), TransportInfo::new(SAMPLE_RATE, stopped, 1.3), 64);
        let start = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 1.3);
        let [_, trig, _] = call(module.as_mut(), start, 64);
        assert!(highs(&trig).is_empty());
    }

    /// A reset while playing fires once on bar 1, however far the position
    /// jumped (rule 8).
    #[test]
    fn reset_while_playing_fires_once() {
        let mut module = clock(&[]);
        let settings = running(TimeSignature::FOUR_FOUR);
        call(module.as_mut(), TransportInfo::new(SAMPLE_RATE, settings, 0.3), 64);
        let reset = TransportInfo::new(SAMPLE_RATE, settings, 0.0).with_jump(true);
        let [_, trig, _] = call(module.as_mut(), reset, 64);
        assert_eq!(highs(&trig), [0]);
    }

    /// A jump that lands mid-cycle, such as a new time signature under a bar
    /// rate, fires nothing, even when the phase moves by more than half.
    #[test]
    fn a_jump_mid_cycle_does_not_fire() {
        let mut module = clock(&[(RATE, 6.0)]);
        call(
            module.as_mut(),
            TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 1.0),
            64,
        );
        // At position 1 the bar phase is 0.25 in 4/4 and 0.8 in 5/16 (1.25 beats a bar).
        let jumped = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::new(5, 16)), 1.0).with_jump(true);
        let [phase, trig, _] = call(module.as_mut(), jumped, 64);
        assert!((phase[0] - 0.8).abs() < 1e-6);
        assert!(highs(&trig).is_empty());
    }

    /// Turning the rate moves the phase at once but fires no trigger.
    #[test]
    fn a_rate_change_does_not_fire() {
        let mut module = clock(&[]);
        let settings = running(TimeSignature::FOUR_FOUR);
        call(module.as_mut(), TransportInfo::new(SAMPLE_RATE, settings, 0.9), 64);
        // 1/4 at 0.9 beats is phase 0.9; 1/2 is phase 0.45: a jump of more than half.
        module.set_param(RATE, 9.0);
        let [phase, trig, _] = call(module.as_mut(), TransportInfo::new(SAMPLE_RATE, settings, 0.9), 64);
        assert!((phase[0] - 0.45).abs() < 1e-6);
        assert!(highs(&trig).is_empty());
    }

    /// A triplet quarter wraps three times in two beats, and is back on the
    /// beat grid at bar 1 of each two beats.
    #[test]
    fn triplets_fire_three_times_in_two_beats() {
        let mut module = clock(&[(RATE, 14.0)]);
        let transport = TransportInfo::new(SAMPLE_RATE, running(TimeSignature::FOUR_FOUR), 0.0);
        let [_, trig, _] = call(module.as_mut(), transport, 2 * BEAT + 1);
        assert_eq!(highs(&trig), [0, 16_000, 32_000, 48_000]);
    }
}
