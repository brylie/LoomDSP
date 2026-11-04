//! EQ (MT-EQ-01): a low cut, a low shelf, a mid band, a high shelf, and a high
//! cut, in that order. Spec: `docs/design/dynamics-eq-crossover.md`. The
//! algorithms are in [`loom_dsp::filter::eq`].

use crate::cached::Cached;
use loom_core::signal::pitch;
use loom_core::{
    Channels, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel, ParamOptions, ParamSpec, PortDefault, PortIo,
    PortSpec, ProcessContext, SignalType, Widget,
};
use loom_dsp::filter::eq::{BUTTERWORTH_Q, Coefficients, Cut, CutCoefficients, CutSide, CutSlope, Section, Shape};
use loom_rt::Smoothed;

/// A frequency parameter, logarithmic and smoothed.
const fn frequency(
    name: &'static str,
    label: &'static str,
    description: &'static str,
    min: f32,
    max: f32,
    default: f32,
) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description,
        min,
        max,
        default,
        unit: "Hz",
        options: ParamOptions::DEFAULT,
    }
    .logarithmic()
    .smooth(0.005)
}

/// A gain parameter, from -24 to 24 dB and smoothed.
const fn gain(name: &'static str, label: &'static str, description: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description,
        min: -24.0,
        max: 24.0,
        default: 0.0,
        unit: "dB",
        options: ParamOptions::DEFAULT,
    }
    .smooth(0.005)
}

/// A cut's slope parameter.
const fn slope(name: &'static str, label: &'static str, description: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label,
        description,
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
    .choices(&["Off", "12", "24"])
}

/// `loom.eq.eq`: five sections that shape the balance of frequencies.
pub static EQ: ModuleDescriptor = ModuleDescriptor {
    id: "loom.eq.eq",
    version: 1,
    name: "EQ",
    category: "EQ",
    family: loom_core::Family::FiltersAndEffects,
    description: "Shapes the balance of frequencies with two cuts, two shelves, and a mid band.",
    inputs: &[PortSpec {
        name: "in",
        label: "In",
        signal: SignalType::Audio,
        description: "The signal to shape.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    outputs: &[PortSpec {
        name: "out",
        label: "Out",
        signal: SignalType::Audio,
        description: "The shaped signal.",
        default: PortDefault::Value(0.0),
        range: loom_core::PortRange::Signal,
        channels: Channels::PerChannel,
    }],
    params: &[
        frequency(
            "low_cut",
            "Low cut",
            "Removes what is below this frequency.",
            10.0,
            1_000.0,
            40.0,
        ),
        slope("low_slope", "Low slope", "How steep the low cut is, or Off."),
        frequency(
            "low_freq",
            "Low freq",
            "The low shelf changes what is below this frequency.",
            20.0,
            2_000.0,
            120.0,
        ),
        gain("low_gain", "Low gain", "How much the low shelf raises or lowers."),
        frequency(
            "mid_freq",
            "Mid freq",
            "The centre of the mid band.",
            40.0,
            16_000.0,
            1_000.0,
        ),
        gain(
            "mid_gain",
            "Mid gain",
            "How much the mid band raises or lowers at its centre.",
        ),
        ParamSpec {
            name: "mid_q",
            label: "Mid Q",
            description: "How narrow the mid band is.",
            min: 0.1,
            max: 10.0,
            default: 1.0,
            unit: "",
            options: ParamOptions::DEFAULT,
        }
        .logarithmic()
        .smooth(0.005),
        frequency(
            "high_freq",
            "High freq",
            "The high shelf changes what is above this frequency.",
            1_000.0,
            20_000.0,
            8_000.0,
        ),
        gain("high_gain", "High gain", "How much the high shelf raises or lowers."),
        frequency(
            "high_cut",
            "High cut",
            "Removes what is above this frequency.",
            1_000.0,
            22_000.0,
            16_000.0,
        ),
        slope("high_slope", "High slope", "How steep the high cut is, or Off."),
    ],
    role: ModuleRole::Dsp(|config| Box::new(Eq::new(config))),
    panel: Panel::new(
        15,
        16,
        &[
            Widget::shape(0, 0, 15, "out", "Response", response_preview),
            Widget::segmented(0, 5, 6, "low_slope"),
            Widget::segmented(9, 5, 6, "high_slope"),
            Widget::knob(0, 7, "low_cut"),
            Widget::knob(3, 7, "low_freq"),
            Widget::knob(3, 10, "low_gain"),
            Widget::knob(6, 7, "mid_freq"),
            Widget::knob(6, 10, "mid_gain"),
            Widget::knob(6, 13, "mid_q"),
            Widget::knob(9, 7, "high_freq"),
            Widget::knob(9, 10, "high_gain"),
            Widget::knob(12, 7, "high_cut"),
        ],
    ),
    displays: &[],
};

/// The slope for a parameter value, or `None` for Off.
#[inline]
fn slope_of(value: f32) -> Option<CutSlope> {
    match value as u32 {
        0 => None,
        1 => Some(CutSlope::Db12),
        _ => Some(CutSlope::Db24),
    }
}

/// The highest corner frequency, as a share of the sample rate.
const TOP_OF_RATE: f32 = 0.49;

/// The curve on the panel: 20 Hz to 20 kHz across on a logarithmic scale, and
/// -24 to 24 dB up. It adds up the digital response of the same five sections
/// at this sample rate, so it shows what the filters do near the top of the range
/// (`docs/learn/filters-near-half-the-sample-rate.md`).
fn response_preview(params: &[f32], x: f32, sample_rate: f32) -> f32 {
    let at_hz = 20.0 * 1_000.0f32.powf(x.clamp(0.0, 1.0));
    // The same limit as the audio.
    let corner = |index: usize| params[index].clamp(1.0, sample_rate * TOP_OF_RATE);
    let cut = |side: CutSide, index: usize, slope: usize| {
        slope_of(params[slope]).map_or(0.0, |slope| side.gain_db(corner(index), slope, at_hz, sample_rate))
    };
    let shape = |shape: Shape| shape.gain_db(at_hz, sample_rate);
    let db = cut(CutSide::Low, Eq::LOW_CUT, Eq::LOW_SLOPE)
        + shape(low_shelf(corner(Eq::LOW_FREQ), params[Eq::LOW_GAIN]))
        + shape(Shape::Bell {
            hz: corner(Eq::MID_FREQ),
            gain_db: params[Eq::MID_GAIN],
            q: params[Eq::MID_Q],
        })
        + shape(high_shelf(corner(Eq::HIGH_FREQ), params[Eq::HIGH_GAIN]))
        + cut(CutSide::High, Eq::HIGH_CUT, Eq::HIGH_SLOPE);
    ((db + 24.0) / 48.0).clamp(0.0, 1.0)
}

/// One cut's state: its filters, how far each slope is blended in, and the
/// cutoff in pitch units.
#[derive(Clone, Copy)]
struct CutState {
    side: CutSide,
    filter: Cut,
    corner: Smoothed,
    slope: f32,
    /// How much of each slope is heard. The rest is the input.
    db12: Smoothed,
    db24: Smoothed,
    coefficients: Cached<f32, CutCoefficients>,
}

/// A smoothed frequency in pitch units, so a sweep sounds even.
fn pitch_smoother(index: usize, rate: f32) -> Smoothed {
    let param = &EQ.params[index];
    Smoothed::new(pitch::from_hz(param.default), rate, param.options.smooth_seconds)
}

impl CutState {
    /// A cut with the defaults of its parameters.
    fn new(side: CutSide, corner: usize, rate: f32) -> Self {
        let hz = EQ.params[corner].default;
        Self {
            side,
            filter: Cut::new(),
            corner: pitch_smoother(corner, rate),
            slope: 0.0,
            db12: Smoothed::new(0.0, rate, 0.005),
            db24: Smoothed::new(0.0, rate, 0.005),
            coefficients: Cached::new(hz, |hz| CutCoefficients::new(side, hz, rate)),
        }
    }

    /// Aims the blend at a slope value.
    #[inline]
    fn aim(&mut self, value: f32) {
        let slope = slope_of(value);
        self.db12
            .set_target(if slope == Some(CutSlope::Db12) { 1.0 } else { 0.0 });
        self.db24
            .set_target(if slope == Some(CutSlope::Db24) { 1.0 } else { 0.0 });
    }

    /// One frame.
    #[inline]
    fn tick(&mut self, x: f32, hz: f32, rate: f32) -> f32 {
        let side = self.side;
        let coefficients = self.coefficients.get(hz, |hz| CutCoefficients::new(side, hz, rate));
        let outputs = self.filter.tick(x, coefficients);
        let (db12, db24) = (self.db12.tick(), self.db24.tick());
        x * (1.0 - db12 - db24) + outputs.db12 * db12 + outputs.db24 * db24
    }

    /// Clears the filters and snaps the blend and the cutoff to their targets.
    fn reset(&mut self) {
        self.filter.reset();
        self.corner.reset(self.corner.target());
        self.db12.reset(self.db12.target());
        self.db24.reset(self.db24.target());
    }
}

/// State for `loom.eq.eq`.
#[derive(Clone)]
struct Eq {
    sample_rate: f32,
    low_cut: CutState,
    high_cut: CutState,
    low: Section,
    mid: Section,
    high: Section,
    low_freq: Smoothed,
    mid_freq: Smoothed,
    high_freq: Smoothed,
    low_gain: Smoothed,
    mid_gain: Smoothed,
    high_gain: Smoothed,
    mid_q: Smoothed,
    low_coefficients: Cached<Shape, Coefficients>,
    mid_coefficients: Cached<Shape, Coefficients>,
    high_coefficients: Cached<Shape, Coefficients>,
}

/// A low shelf.
const fn low_shelf(hz: f32, gain_db: f32) -> Shape {
    Shape::LowShelf {
        hz,
        gain_db,
        q: BUTTERWORTH_Q,
    }
}

/// A high shelf.
const fn high_shelf(hz: f32, gain_db: f32) -> Shape {
    Shape::HighShelf {
        hz,
        gain_db,
        q: BUTTERWORTH_Q,
    }
}

impl Eq {
    const LOW_CUT: usize = EQ.param_index("low_cut").unwrap();
    const LOW_SLOPE: usize = EQ.param_index("low_slope").unwrap();
    const LOW_FREQ: usize = EQ.param_index("low_freq").unwrap();
    const LOW_GAIN: usize = EQ.param_index("low_gain").unwrap();
    const MID_FREQ: usize = EQ.param_index("mid_freq").unwrap();
    const MID_GAIN: usize = EQ.param_index("mid_gain").unwrap();
    const MID_Q: usize = EQ.param_index("mid_q").unwrap();
    const HIGH_FREQ: usize = EQ.param_index("high_freq").unwrap();
    const HIGH_GAIN: usize = EQ.param_index("high_gain").unwrap();
    const HIGH_CUT: usize = EQ.param_index("high_cut").unwrap();
    const HIGH_SLOPE: usize = EQ.param_index("high_slope").unwrap();

    fn new(config: &ModuleConfig) -> Self {
        let rate = config.sample_rate;
        let params = EQ.params;
        let plain = |index: usize| Smoothed::new(params[index].default, rate, params[index].options.smooth_seconds);
        let default = |index: usize| params[index].default;
        let compute = move |shape| Coefficients::new(shape, rate);
        Self {
            sample_rate: rate,
            low_cut: CutState::new(CutSide::Low, Self::LOW_CUT, rate),
            high_cut: CutState::new(CutSide::High, Self::HIGH_CUT, rate),
            low: Section::new(),
            mid: Section::new(),
            high: Section::new(),
            low_freq: pitch_smoother(Self::LOW_FREQ, rate),
            mid_freq: pitch_smoother(Self::MID_FREQ, rate),
            high_freq: pitch_smoother(Self::HIGH_FREQ, rate),
            low_gain: plain(Self::LOW_GAIN),
            mid_gain: plain(Self::MID_GAIN),
            high_gain: plain(Self::HIGH_GAIN),
            mid_q: plain(Self::MID_Q),
            low_coefficients: Cached::new(low_shelf(default(Self::LOW_FREQ), default(Self::LOW_GAIN)), compute),
            mid_coefficients: Cached::new(
                Shape::Bell {
                    hz: default(Self::MID_FREQ),
                    gain_db: default(Self::MID_GAIN),
                    q: default(Self::MID_Q),
                },
                compute,
            ),
            high_coefficients: Cached::new(high_shelf(default(Self::HIGH_FREQ), default(Self::HIGH_GAIN)), compute),
        }
    }
}

impl Module for Eq {
    /// Takes the running state of another instance (ADR-0022).
    fn copy_state_from(&mut self, other: &dyn Module) -> bool {
        loom_core::copy_state(self, other)
    }

    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let params = [
            io.param(Self::LOW_CUT),
            io.param(Self::LOW_SLOPE),
            io.param(Self::LOW_FREQ),
            io.param(Self::LOW_GAIN),
            io.param(Self::MID_FREQ),
            io.param(Self::MID_GAIN),
            io.param(Self::MID_Q),
            io.param(Self::HIGH_FREQ),
            io.param(Self::HIGH_GAIN),
            io.param(Self::HIGH_CUT),
            io.param(Self::HIGH_SLOPE),
        ];
        let rate = self.sample_rate;
        let top = rate * TOP_OF_RATE;
        // Hertz for a frame: the modulated value, or the glide. A bad value uses
        // the parameter's default for that frame, as NaN would stay in a filter.
        let hz_of = |index: usize, value: f32| {
            let hz = pitch::to_hz(value);
            let hz = if hz.is_nan() { EQ.params[index].default } else { hz };
            hz.clamp(1.0, top)
        };
        let ([input], [out]) = io.split::<1, 1>();
        for (i, (out, &x)) in out.iter_mut().zip(input).enumerate() {
            let x = loom_dsp::guard(x);
            let glide = |param: &loom_core::Param<'_>, smoothed: &mut Smoothed, index: usize| {
                hz_of(index, param.map_or(i, smoothed.tick(), pitch::from_hz))
            };
            self.low_cut.aim(params[1].at_or(i, self.low_cut.slope));
            let low_cut = glide(&params[0], &mut self.low_cut.corner, Self::LOW_CUT);
            let low_freq = glide(&params[2], &mut self.low_freq, Self::LOW_FREQ);
            let low_gain = params[3].at_or(i, self.low_gain.tick());
            let mid_freq = glide(&params[4], &mut self.mid_freq, Self::MID_FREQ);
            let mid_gain = params[5].at_or(i, self.mid_gain.tick());
            let mid_q = params[6].at_or(i, self.mid_q.tick());
            let high_freq = glide(&params[7], &mut self.high_freq, Self::HIGH_FREQ);
            let high_gain = params[8].at_or(i, self.high_gain.tick());
            let high_cut = glide(&params[9], &mut self.high_cut.corner, Self::HIGH_CUT);
            self.high_cut.aim(params[10].at_or(i, self.high_cut.slope));

            let compute = |shape| Coefficients::new(shape, rate);
            let y = self.low_cut.tick(x, low_cut, rate);
            let y = self
                .low
                .tick(y, self.low_coefficients.get(low_shelf(low_freq, low_gain), compute));
            let bell = Shape::Bell {
                hz: mid_freq,
                gain_db: mid_gain,
                q: mid_q,
            };
            let y = self.mid.tick(y, self.mid_coefficients.get(bell, compute));
            let y = self
                .high
                .tick(y, self.high_coefficients.get(high_shelf(high_freq, high_gain), compute));
            *out = self.high_cut.tick(y, high_cut, rate);
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        match index {
            Self::LOW_CUT => self.low_cut.corner.set_target(pitch::from_hz(value)),
            Self::LOW_SLOPE => self.low_cut.slope = value,
            Self::LOW_FREQ => self.low_freq.set_target(pitch::from_hz(value)),
            Self::LOW_GAIN => self.low_gain.set_target(value),
            Self::MID_FREQ => self.mid_freq.set_target(pitch::from_hz(value)),
            Self::MID_GAIN => self.mid_gain.set_target(value),
            Self::MID_Q => self.mid_q.set_target(value),
            Self::HIGH_FREQ => self.high_freq.set_target(pitch::from_hz(value)),
            Self::HIGH_GAIN => self.high_gain.set_target(value),
            Self::HIGH_CUT => self.high_cut.corner.set_target(pitch::from_hz(value)),
            Self::HIGH_SLOPE => self.high_cut.slope = value,
            _ => {}
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        match index {
            // The frequencies glide in pitch. The engine glides in the knob's
            // position, which is also logarithmic, so the paths match.
            Self::LOW_CUT => self.low_cut.corner.gliding().map(pitch::to_hz),
            Self::LOW_FREQ => self.low_freq.gliding().map(pitch::to_hz),
            Self::LOW_GAIN => self.low_gain.gliding(),
            Self::MID_FREQ => self.mid_freq.gliding().map(pitch::to_hz),
            Self::MID_GAIN => self.mid_gain.gliding(),
            Self::MID_Q => self.mid_q.gliding(),
            Self::HIGH_FREQ => self.high_freq.gliding().map(pitch::to_hz),
            Self::HIGH_GAIN => self.high_gain.gliding(),
            Self::HIGH_CUT => self.high_cut.corner.gliding().map(pitch::to_hz),
            _ => None,
        }
    }

    fn reset(&mut self) {
        // The cuts aim from their slopes first, so the blends snap to them.
        let (low, high) = (self.low_cut.slope, self.high_cut.slope);
        self.low_cut.aim(low);
        self.high_cut.aim(high);
        self.low_cut.reset();
        self.high_cut.reset();
        self.low.reset();
        self.mid.reset();
        self.high.reset();
        for smoothed in [
            &mut self.low_freq,
            &mut self.mid_freq,
            &mut self.high_freq,
            &mut self.low_gain,
            &mut self.mid_gain,
            &mut self.high_gain,
            &mut self.mid_q,
        ] {
            smoothed.reset(smoothed.target());
        }
    }
}
