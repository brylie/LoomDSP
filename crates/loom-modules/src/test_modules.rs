//! Test-only modules that engine, app, and canvas tests share.
//!
//! The built-in modules do not cover every panel widget or parameter
//! mapping, so a test that needs one uses these instead of declaring its
//! own descriptor. Their IDs start with `test.`, so they never clash with a
//! real module. Add them to a registry with [`register`]. They build only
//! with the `test-modules` feature, so a plugin build does not carry them.
//!
//! - [`PANEL`] (`test.panel`) has every widget kind on its panel, every
//!   parameter mapping, and one output of each polarity.
//! - [`READOUT`] (`test.readout`) has a readout of each format, and a meter on
//!   a display value that runs below and above zero.
//! - [`LINEAR`], [`LOG`], [`STEPS`], and [`SMOOTH`] are mirrors: one
//!   parameter each, and one output that writes what the engine gave that
//!   parameter, so a test can read it.

use loom_core::{
    Channels, DisplayFormat, DisplayKind, DisplayRange, DisplaySpec, LoomError, Module, ModuleConfig, ModuleDescriptor,
    ModuleRole, Panel, ParamOptions, ParamSpec, Polarity, PortDefault, PortIo, PortSpec, ProcessContext, Registry,
    SignalType, Widget,
};
use loom_rt::Smoothed;

/// Every test module.
pub static ALL: &[&ModuleDescriptor] = &[&PANEL, &READOUT, &LINEAR, &LOG, &STEPS, &SMOOTH];

// Every test port and parameter has a label and a description, like the
// built-ins, or this crate does not compile.
const _: () = loom_core::assert_documented(ALL);

/// Adds every test module to a registry. The registry checks their panels.
pub fn register(registry: &mut Registry) -> Result<(), LoomError> {
    for descriptor in ALL {
        registry.register(descriptor)?;
    }
    Ok(())
}

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

const fn output(name: &'static str, polarity: Polarity) -> PortSpec {
    PortSpec {
        name,
        label: name,
        signal: SignalType::Modulation,
        description: "The parameter `level`, or its modulated value.",
        default: PortDefault::Value(0.0),
        range: match polarity {
            Polarity::Unipolar => loom_core::PortRange::Span(0.0, 1.0),
            Polarity::Bipolar => loom_core::PortRange::Signal,
        },
        channels: Channels::PerChannel,
    }
}

/// Index of `level` in [`PANEL`]: the parameter its outputs write.
const LEVEL: usize = 0;
/// Index of `on` in [`PANEL`]: the parameter its light shows.
const ON: usize = 4;
/// Index of `glide` in [`PANEL`]: its smoothed parameter.
const GLIDE: usize = PANEL.param_index("glide").unwrap();

/// `test.panel`: one of every panel widget and parameter mapping.
///
/// | Parameter | Mapping | Widget |
/// |---|---|---|
/// | `level` | linear, 0 to 1 | knob |
/// | `cutoff` | logarithmic, 20 to 20 000 Hz | knob |
/// | `steps` | whole-number, 0 to 10 | knob |
/// | `glide` | linear, smoothed over 50 ms | knob |
/// | `on` | whole-number, 0 to 1 | toggle |
/// | `choice` | whole-number, 0 to 2, named `a`, `b`, `c` | radio |
/// | `gate` | whole-number, 0 to 1 | button |
/// | `mode` | whole-number, 0 to 1 | none (the button's mode) |
/// | `held` | whole-number, 0 to 1, transient | none (the button's hold) |
/// | `long` | whole-number, 0 to 15, named | select list that scrolls |
/// | `voices` | whole-number, 1 to 16, not named | select list of numbers |
/// | `tall` | logarithmic, 20 to 20 000 Hz | vertical fader |
/// | `wide` | linear, 0 to 1 | horizontal fader |
/// | `count` | whole-number, 0 to 1000 | number |
/// | `tempo` | linear, 20 to 300 BPM | number |
///
/// Outputs `uni` (unipolar) and `bi` (bipolar) both write `level`. The
/// light `lit` shows `on`, modulated, as of the last frame of the block.
/// The meter reads `uni`.
pub static PANEL: ModuleDescriptor = ModuleDescriptor {
    id: "test.panel",
    version: 1,
    name: "Test Panel",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "Every panel widget and parameter mapping, for tests.",
    inputs: &[],
    outputs: &[output("uni", Polarity::Unipolar), output("bi", Polarity::Bipolar)],
    params: &[
        param("level", 0.0, 1.0, 0.0, ""),
        param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic(),
        param("steps", 0.0, 10.0, 0.0, "").integer(),
        param("glide", 0.0, 1.0, 0.0, "").smooth(0.05),
        param("on", 0.0, 1.0, 0.0, "").integer(),
        param("choice", 0.0, 2.0, 0.0, "").integer().choices(&["a", "b", "c"]),
        param("gate", 0.0, 1.0, 0.0, "").integer(),
        param("mode", 0.0, 1.0, 0.0, "").integer(),
        param("held", 0.0, 1.0, 0.0, "").integer().transient(),
        param("long", 0.0, 15.0, 0.0, "").integer().choices(&[
            "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet", "Kilo",
            "Lima", "Mike", "November", "Oscar", "Papa",
        ]),
        param("voices", 1.0, 16.0, 1.0, "voices").integer(),
        param("pad_x", 0.0, 1.0, 0.5, ""),
        param("pad_y", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic(),
        param("span_low", -10.0, 10.0, 0.0, "").ranges(crate::math::WIDE_RANGES, crate::math::DEFAULT_RANGE),
        param("span_high", -10.0, 10.0, 1.0, "").ranges(crate::math::WIDE_RANGES, crate::math::DEFAULT_RANGE),
        param("tall_low", 0.0, 1.0, 0.25, ""),
        param("tall_high", 0.0, 1.0, 0.75, ""),
        param("tall", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic(),
        param("wide", 0.0, 1.0, 0.5, ""),
        param("count", 0.0, 1000.0, 7.0, "").integer(),
        param("tempo", 20.0, 300.0, 120.0, "BPM"),
    ],
    role: ModuleRole::Dsp(|config| {
        let glide = &PANEL.params[GLIDE];
        Box::new(TestPanel {
            params: [0.0; 21],
            lit: 0.0,
            glide: Smoothed::new(glide.default, config.sample_rate, glide.options.smooth_seconds),
        })
    }),
    panel: Panel::new(
        27,
        20,
        &[
            Widget::knob(0, 0, "level"),
            Widget::knob(3, 0, "cutoff"),
            Widget::knob(6, 0, "steps"),
            Widget::knob(9, 0, "glide"),
            Widget::meter(15, 0, "uni"),
            Widget::toggle(0, 3, "on"),
            Widget::radio(3, 3, "choice"),
            Widget::light(9, 3, "lit"),
            Widget::button(9, 5, "gate", "held", "mode"),
            Widget::scope(0, 8, "bi"),
            Widget::level_history(18, 0, "uni"),
            Widget::spectrum(18, 7, "bi"),
            Widget::xy_scope(18, 12, "bi", "uni"),
            Widget::select(9, 8, "long"),
            Widget::select(9, 10, "voices"),
            Widget::fader(15, 7, "tall"),
            Widget::fader_horizontal(9, 12, "wide"),
            Widget::number(0, 14, "count"),
            Widget::number(5, 14, "tempo"),
            Widget::xy_pad(12, 14, "pad_x", "pad_y", "Position"),
            Widget::range(0, 16, "span_low", "span_high", "Span"),
            Widget::range_vertical(10, 14, "tall_low", "tall_high", "Tall"),
        ],
    ),
    displays: &[DisplaySpec {
        name: "lit",
        label: "Lit",
        description: "Shows `on`, modulated, as of the last frame of the block.",
        kind: DisplayKind::Level,
        range: DisplayRange::UNIT,
    }],
};

struct TestPanel {
    params: [f32; 21],
    /// `on` at the last frame of the last block, for the light.
    lit: f32,
    /// The smoothed parameter, which no output shows.
    glide: Smoothed,
}

impl Module for TestPanel {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let level = io.modulated(LEVEL);
        self.lit = io
            .modulated(ON)
            .and_then(|samples| samples.last().copied())
            .unwrap_or(self.params[ON]);
        let ([], [uni, bi]) = io.split::<0, 2>();
        for (i, (u, b)) in uni.iter_mut().zip(bi.iter_mut()).enumerate() {
            self.glide.tick();
            let value = level.map_or(self.params[LEVEL], |samples| samples[i]);
            *u = value;
            *b = value;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(slot) = self.params.get_mut(index) {
            *slot = value;
        }
        if index == GLIDE {
            self.glide.set_target(value);
        }
    }

    fn param_glide(&self, index: usize) -> Option<f32> {
        if index == GLIDE { self.glide.gliding() } else { None }
    }

    fn reset(&mut self) {
        self.glide.reset(self.glide.target());
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.lit;
    }
}

/// `test.readout`: one readout of each display format, and a meter on a value
/// with a range around zero. Each display value shows the parameter of its
/// name, so a test sets what it reads.
pub static READOUT: ModuleDescriptor = ModuleDescriptor {
    id: "test.readout",
    version: 1,
    name: "Test Readout",
    category: "Test",
    family: loom_core::Family::Interface,
    description: "A readout of each format and a meter on a display value, for tests.",
    inputs: &[],
    outputs: &[],
    params: &[
        param("count", 0.0, 100.0, 7.0, "").integer(),
        param("pitch", -1.0, 1.0, 0.09, ""),
        param("input", 0.0, 2.0, 1.0, "").integer(),
        param("cents", -50.0, 50.0, 12.5, "cents"),
    ],
    role: ModuleRole::Dsp(|_| Box::new(TestReadout { values: [0.0; 4] })),
    panel: Panel::new(
        12,
        7,
        &[
            Widget::readout(0, 0, 3, "count"),
            Widget::readout(3, 0, 6, "cents"),
            Widget::readout(0, 2, 6, "pitch"),
            Widget::readout(0, 4, 6, "input"),
            Widget::meter(9, 0, "cents"),
        ],
    ),
    displays: &[
        DisplaySpec {
            name: "count",
            label: "Count",
            description: "A plain number.",
            kind: DisplayKind::Level,
            range: DisplayRange {
                min: 0.0,
                max: 100.0,
                unit: "",
                format: DisplayFormat::Number { decimals: 0 },
            },
        },
        DisplaySpec {
            name: "pitch",
            label: "Pitch",
            description: "A note name with cents.",
            kind: DisplayKind::Level,
            range: DisplayRange {
                min: -1.0,
                max: 1.0,
                unit: "",
                format: DisplayFormat::Note,
            },
        },
        DisplaySpec {
            name: "input",
            label: "Input",
            description: "A choice name.",
            kind: DisplayKind::Level,
            range: DisplayRange {
                min: 0.0,
                max: 2.0,
                unit: "",
                format: DisplayFormat::Choices(&["First", "Second", "Third"]),
            },
        },
        DisplaySpec {
            name: "cents",
            label: "Cents",
            description: "A value that runs below and above zero, with a unit.",
            kind: DisplayKind::Level,
            range: DisplayRange {
                min: -50.0,
                max: 50.0,
                unit: "cents",
                format: DisplayFormat::Number { decimals: 1 },
            },
        },
    ],
};

struct TestReadout {
    values: [f32; 4],
}

impl Module for TestReadout {
    fn process(&mut self, _ctx: &ProcessContext<'_>, _io: &mut PortIo<'_>) {}

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(slot) = self.values.get_mut(index) {
            *slot = value;
        }
    }

    fn displays(&self, values: &mut [f32]) {
        values.copy_from_slice(&self.values);
    }
}

/// Writes its only parameter, or that parameter's modulated buffer, to `out`.
/// A parameter with a smooth time glides to a new value, as a built-in does.
struct Mirror {
    value: f32,
    glide: Option<Smoothed>,
}

impl Module for Mirror {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let samples = io.modulated(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, sample) in out.iter_mut().enumerate() {
            let base = self.glide.as_mut().map_or(self.value, Smoothed::tick);
            *sample = samples.map_or(base, |values| values[i]);
        }
    }

    fn set_param(&mut self, _index: usize, value: f32) {
        self.value = value;
        if let Some(glide) = self.glide.as_mut() {
            glide.set_target(value);
        }
    }

    fn param_glide(&self, _index: usize) -> Option<f32> {
        self.glide.as_ref().and_then(Smoothed::gliding)
    }

    fn reset(&mut self) {
        if let Some(glide) = self.glide.as_mut() {
            glide.reset(glide.target());
        }
    }
}

/// A mirror of the only parameter of `descriptor`.
fn mirror(descriptor: &ModuleDescriptor, config: &ModuleConfig) -> Box<dyn Module> {
    let spec = &descriptor.params[0];
    let seconds = spec.options.smooth_seconds;
    Box::new(Mirror {
        value: spec.default,
        glide: (seconds > 0.0).then(|| Smoothed::new(spec.default, config.sample_rate, seconds)),
    })
}

macro_rules! mirror_module {
    ($(#[$doc:meta])* $name:ident, $id:literal, $param:expr) => {
        $(#[$doc])*
        pub static $name: ModuleDescriptor = ModuleDescriptor {
            id: $id,
            version: 1,
            name: $id,
            category: "Test",
            family: loom_core::Family::Interface,
            description: "Writes its parameter, or the modulated buffer.",
            inputs: &[],
            outputs: &[PortSpec {
                name: "out",
                label: "Out",
                signal: SignalType::Modulation,
                description: "The parameter, or its modulated value.",
                default: PortDefault::Value(0.0),
                range: loom_core::PortRange::Signal,
                channels: Channels::PerChannel,
            }],
            params: &[$param],
            role: ModuleRole::Dsp(|config| mirror(&$name, config)),
            panel: Panel::EMPTY,
            displays: &[],
        };
    };
}

mirror_module!(
    /// `test.linear`: mirrors `level`, linear from 0 to 1.
    LINEAR,
    "test.linear",
    param("level", 0.0, 1.0, 0.0, "")
);
mirror_module!(
    /// `test.log`: mirrors `cutoff`, logarithmic from 20 to 20 000 Hz.
    LOG,
    "test.log",
    param("cutoff", 20.0, 20_000.0, 1_000.0, "Hz").logarithmic()
);
mirror_module!(
    /// `test.steps`: mirrors `steps`, whole-number from 0 to 10.
    STEPS,
    "test.steps",
    param("steps", 0.0, 10.0, 0.0, "").integer()
);
mirror_module!(
    /// `test.smooth`: mirrors `level`, linear from 0 to 1, smoothed over
    /// 50 ms.
    SMOOTH,
    "test.smooth",
    param("level", 0.0, 1.0, 0.0, "").smooth(0.05)
);

#[cfg(test)]
mod tests;
