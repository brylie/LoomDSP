//! Transport: the patch's one control surface for musical time
//! (MT-CLOCK-01, FR-LIB-25, FR-LIB-27).
//!
//! The engine owns the transport (ADR-0015). This module is how a patch
//! changes it: its parameters (tempo, time signature, run, reset) and its
//! `run` and `reset` inputs become [`TransportRequest`]s, which the engine
//! applies before a block. Changes from parameters apply at once; edges on
//! the inputs wait for the next step of the transport grid (rule 11), so
//! the output is the same at any block size. Its outputs put the transport
//! on cables.
//! Spec: `docs/design/clock.md`, Controls live on the Transport module,
//! Starting and stopping, and the Transport row of Modules.

use loom_core::signal::{EdgeDetector, WrapDetector, gate_is_high, trigger};
use loom_core::{
    Channels, DisplayKind, DisplaySpec, Module, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, Polarity,
    PortDefault, PortIo, PortSpec, ProcessContext, SignalType, TRANSPORT_MODULE, TimeSignature, TransportRequest,
    TriggerFlag, Widget,
};

/// The denominator names, in order from 0: powers of two.
const DENOMINATORS: &[&str] = &["1", "2", "4", "8", "16"];

/// `loom.clock.transport`: the patch's tempo, time signature, run, and
/// reset, on a panel and as inputs, and the transport's signals as outputs.
/// Every patch has exactly one.
pub static TRANSPORT: ModuleDescriptor = ModuleDescriptor {
    id: TRANSPORT_MODULE,
    version: 1,
    name: "Transport",
    category: "Clock",
    description: "The patch's tempo, time signature, and play state. Every patch has exactly one.",
    inputs: &[
        PortSpec {
            name: "run",
            label: "Run",
            signal: SignalType::Gate,
            description: "High runs, low stops. While connected, it decides over the Run toggle.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::Downmix,
        },
        PortSpec {
            name: "reset",
            label: "Reset",
            signal: SignalType::Gate,
            description: "A rising edge returns to bar 1.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::Downmix,
        },
    ],
    outputs: &[
        PortSpec {
            name: "bar-phase",
            label: "Bar",
            signal: SignalType::Phase,
            description: "Rises from 0 to 1 once per bar.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "beat-phase",
            label: "Beat",
            signal: SignalType::Phase,
            description: "Rises from 0 to 1 once per quarter-note beat.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "playing",
            label: "Playing",
            signal: SignalType::Gate,
            description: "High while the transport runs.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "bar",
            label: "On bar",
            signal: SignalType::Gate,
            description: "Fires at the start of each bar while the transport runs.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "beat",
            label: "On beat",
            signal: SignalType::Gate,
            description: "Fires at the start of each beat while the transport runs.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::PerChannel,
        },
        PortSpec {
            name: "jump",
            label: "Jump",
            signal: SignalType::Gate,
            description: "Fires when the position jumps: a reset or a new time signature.",
            default: PortDefault::Value(0.0),
            polarity: Polarity::Unipolar,
            channels: Channels::PerChannel,
        },
    ],
    params: &[
        ParamSpec {
            name: "tempo",
            label: "Tempo",
            description: "Quarter-note beats per minute.",
            min: 20.0,
            max: 300.0,
            default: 120.0,
            unit: "BPM",
            options: ParamOptions::DEFAULT,
        },
        ParamSpec {
            name: "numerator",
            label: "Beats",
            description: "The top number of the time signature: how many beats in a bar.",
            min: 1.0,
            max: 32.0,
            default: 4.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "denominator",
            label: "Note",
            description: "The bottom number of the time signature: the note value of one beat.",
            min: 0.0,
            max: 4.0,
            default: 2.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .choices(DENOMINATORS),
        ParamSpec {
            name: "run",
            label: "Run",
            description: "On runs the transport, off stops it and holds the position.",
            min: 0.0,
            max: 1.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer(),
        ParamSpec {
            name: "reset",
            label: "Reset",
            description: "Returns to bar 1 when pressed. Never saved.",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .integer()
        .transient(),
    ],
    role: ModuleRole::Dsp(|_| Box::new(Transport::new())),
    panel: Panel::new(
        15,
        5,
        &[
            Widget::toggle(0, 0, "run"),
            Widget::light(3, 0, "beat"),
            Widget::momentary(0, 2, "reset"),
            Widget::knob(6, 0, "tempo"),
            Widget::knob(9, 0, "numerator"),
            Widget::knob(12, 0, "denominator"),
        ],
    ),
    displays: &[
        DisplaySpec {
            name: "running",
            label: "Running",
            description: "1 while the transport runs. The Run toggle shows it while a cable drives Run.",
            kind: DisplayKind::Level,
        },
        DisplaySpec {
            name: "beat",
            label: "Beat",
            description: "Flashes on each beat.",
            kind: DisplayKind::Trigger,
        },
    ],
};

/// Parameter indices.
const TEMPO: usize = 0;
const NUMERATOR: usize = 1;
const DENOMINATOR: usize = 2;
const RUN: usize = 3;
const RESET: usize = 4;

/// Input indices.
const RUN_IN: usize = 0;
const RESET_IN: usize = 1;

/// State for `loom.clock.transport`.
///
/// Requests are made only for changes, so a cable removed from `run` leaves
/// the transport as it was, and the controller's own transport calls still
/// work in between.
struct Transport {
    tempo: f32,
    numerator: f32,
    denominator: f32,
    run: f32,
    /// The last values sent to the engine. `None` until the first request,
    /// which sends everything, so a loaded patch sets the transport.
    sent_tempo: Option<f32>,
    sent_signature: Option<TimeSignature>,
    sent_run: Option<bool>,
    /// The Reset button was pressed since the last request.
    reset_pressed: bool,
    /// Whether `run` had a cable in the last call. While it has, the input
    /// decides and the Run parameter is not sent.
    run_connected: bool,
    /// The run gate's level at the last frame, while connected.
    run_gate: Option<bool>,
    /// What the inputs found, waiting for the next grid step.
    pending_run: Option<bool>,
    pending_reset: bool,
    reset_in: EdgeDetector,
    bar: WrapDetector,
    beat: WrapDetector,
    running: bool,
    /// Whether a beat fired since the light last read it.
    beat_fired: TriggerFlag,
}

impl Transport {
    fn new() -> Self {
        Self {
            tempo: 120.0,
            numerator: 4.0,
            denominator: 2.0,
            run: 1.0,
            sent_tempo: None,
            sent_signature: None,
            sent_run: None,
            reset_pressed: false,
            run_connected: false,
            run_gate: None,
            pending_run: None,
            pending_reset: false,
            reset_in: EdgeDetector::new(),
            bar: WrapDetector::new(),
            beat: WrapDetector::new(),
            running: false,
            beat_fired: TriggerFlag::default(),
        }
    }

    /// The time signature the parameters set.
    fn time_signature(&self) -> TimeSignature {
        let numerator = self.numerator.round().clamp(1.0, 32.0) as u32;
        let index = self.denominator.round().clamp(0.0, 4.0) as u32;
        TimeSignature::new(numerator, 1 << index)
    }
}

impl Module for Transport {
    fn process(&mut self, ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let transport = ctx.transport();
        let playing = transport.playing();
        self.running = playing;
        // As for the Clock: a jump or a stop starts the wrap rule afresh, so
        // a reset fires bar 1 once and no skipped bars fire (rule 8).
        if transport.jumped() || !playing {
            self.bar.reset();
            self.beat.reset();
        }
        let run_connected = io.is_connected(RUN_IN);
        let reset_connected = io.is_connected(RESET_IN);
        if !run_connected {
            // On the next connection the gate decides at once.
            self.run_gate = None;
        }
        self.run_connected = run_connected;
        let ([run_in, reset_in], [bar_phase, beat_phase, playing_out, bar_out, beat_out, jump_out]) =
            io.split::<2, 6>();
        for i in 0..bar_phase.len() {
            if run_connected {
                let high = gate_is_high(run_in[i]);
                if self.run_gate != Some(high) {
                    self.run_gate = Some(high);
                    self.pending_run = Some(high);
                }
            }
            if reset_connected && self.reset_in.rising(reset_in[i]) {
                self.pending_reset = true;
            }
            let bar = transport.bar_phase_at(i);
            let beat = transport.beat_phase_at(i);
            let bar_fired = playing && self.bar.wrapped(bar);
            let beat_fired = playing && self.beat.wrapped(beat);
            self.beat_fired.record(beat_fired);
            bar_phase[i] = bar;
            beat_phase[i] = beat;
            playing_out[i] = if playing { 1.0 } else { 0.0 };
            bar_out[i] = trigger(bar_fired);
            beat_out[i] = trigger(beat_fired);
            jump_out[i] = trigger(i == 0 && transport.jumped());
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            TEMPO => self.tempo = value,
            NUMERATOR => self.numerator = value,
            DENOMINATOR => self.denominator = value,
            RUN => self.run = value,
            // A press, not a release, resets: the reset lands on time.
            RESET if value >= 0.5 => self.reset_pressed = true,
            _ => {}
        }
    }

    fn transport_request(&mut self, grid: bool) -> TransportRequest {
        let mut request = TransportRequest::default();
        if self.sent_tempo != Some(self.tempo) {
            self.sent_tempo = Some(self.tempo);
            request.tempo = Some(f64::from(self.tempo));
        }
        let signature = self.time_signature();
        if self.sent_signature != Some(signature) {
            self.sent_signature = Some(signature);
            request.time_signature = Some(signature);
        }
        let run = self.run >= 0.5;
        if self.sent_run != Some(run) {
            self.sent_run = Some(run);
            // While a cable drives run, it decides (Starting and stopping).
            if !self.run_connected {
                request.running = Some(run);
            }
        }
        request.reset = std::mem::take(&mut self.reset_pressed);
        // Edges on the inputs wait for the grid (rule 11).
        if grid {
            if let Some(run) = self.pending_run.take() {
                request.running = Some(run);
            }
            request.reset |= std::mem::take(&mut self.pending_reset);
        }
        request
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = if self.running { 1.0 } else { 0.0 };
        values[1] = self.beat_fired.take();
    }

    fn reset(&mut self) {
        // A panic keeps the transport (panic.md). Only the edge detectors
        // and the light start fresh.
        self.reset_in.reset();
        self.bar.reset();
        self.beat.reset();
        self.beat_fired.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::ModuleConfig;

    fn transport() -> Box<dyn Module> {
        let ModuleRole::Dsp(create) = TRANSPORT.role else {
            unreachable!("the Transport is a DSP module")
        };
        let mut module = create(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 1024,
        });
        for (index, param) in TRANSPORT.params.iter().enumerate() {
            module.set_param(index, param.default);
        }
        module
    }

    /// The first request sends every setting, so a loaded patch sets the
    /// transport. Later requests send only changes.
    #[test]
    fn requests_send_changes_only() {
        let mut module = transport();
        let first = module.transport_request(false);
        assert_eq!(first.tempo, Some(120.0));
        assert_eq!(first.time_signature, Some(TimeSignature::FOUR_FOUR));
        assert_eq!(first.running, Some(true));
        assert!(!first.reset);
        assert!(module.transport_request(true).is_empty());
        module.set_param(TEMPO, 96.0);
        module.set_param(DENOMINATOR, 3.0);
        module.set_param(NUMERATOR, 7.0);
        let next = module.transport_request(false);
        assert_eq!(next.tempo, Some(96.0));
        assert_eq!(next.time_signature, Some(TimeSignature::new(7, 8)));
        assert_eq!(next.running, None);
    }

    /// A press of Reset asks for one reset; holding it asks for no more.
    #[test]
    fn reset_press_asks_once() {
        let mut module = transport();
        module.transport_request(false);
        module.set_param(RESET, 1.0);
        assert!(module.transport_request(false).reset);
        assert!(!module.transport_request(false).reset);
        module.set_param(RESET, 0.0);
        assert!(!module.transport_request(false).reset);
    }
}
