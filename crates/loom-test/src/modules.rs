//! Test-only modules that engine, app, and canvas tests share.
//!
//! The built-in modules do not cover every panel widget or parameter
//! mapping, so a test that needs one uses these instead of declaring its
//! own descriptor. Their IDs start with `test.`, so they never clash with a
//! real module. Add them to a registry with [`register`].
//!
//! - [`PANEL`] (`test.panel`) has every widget kind on its panel, every
//!   parameter mapping, and one output of each polarity.
//! - [`LINEAR`], [`LOG`], [`STEPS`], and [`SMOOTH`] are mirrors: one
//!   parameter each, and one output that writes what the engine gave that
//!   parameter, so a test can read it.

use loom_core::{
    Channels, DisplayKind, DisplaySpec, LoomError, Module, ModuleConfig, ModuleDescriptor, ModuleRole, Panel,
    ParamOptions, ParamSpec, Polarity, PortDefault, PortIo, PortSpec, ProcessContext, Registry, SignalType, Widget,
};

/// Every test module.
pub static ALL: &[&ModuleDescriptor] = &[&PANEL, &LINEAR, &LOG, &STEPS, &SMOOTH];

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
        polarity,
        channels: Channels::PerChannel,
    }
}

/// Index of `level` in [`PANEL`]: the parameter its outputs write.
const LEVEL: usize = 0;
/// Index of `on` in [`PANEL`]: the parameter its light shows.
const ON: usize = 4;

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
///
/// Outputs `uni` (unipolar) and `bi` (bipolar) both write `level`. The
/// light `lit` shows `on`, modulated, as of the last frame of the block.
/// The meter reads `uni`.
pub static PANEL: ModuleDescriptor = ModuleDescriptor {
    id: "test.panel",
    version: 1,
    name: "Test Panel",
    category: "Test",
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
    ],
    role: ModuleRole::Dsp(|_| {
        Box::new(TestPanel {
            params: [0.0; 9],
            lit: 0.0,
        })
    }),
    panel: Panel::new(
        15,
        8,
        &[
            Widget::knob(0, 0, "level"),
            Widget::knob(3, 0, "cutoff"),
            Widget::knob(6, 0, "steps"),
            Widget::knob(9, 0, "glide"),
            Widget::meter(12, 0, "uni"),
            Widget::toggle(0, 3, "on"),
            Widget::radio(3, 3, "choice"),
            Widget::light(6, 3, "lit"),
            Widget::button(6, 5, "gate", "held", "mode"),
        ],
    ),
    displays: &[DisplaySpec {
        name: "lit",
        label: "Lit",
        description: "Shows `on`, modulated, as of the last frame of the block.",
        kind: DisplayKind::Level,
    }],
};

struct TestPanel {
    params: [f32; 9],
    /// `on` at the last frame of the last block, for the light.
    lit: f32,
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
            let value = level.map_or(self.params[LEVEL], |samples| samples[i]);
            *u = value;
            *b = value;
        }
    }

    fn set_param(&mut self, index: usize, value: f32) {
        if let Some(slot) = self.params.get_mut(index) {
            *slot = value;
        }
    }

    fn displays(&self, values: &mut [f32]) {
        values[0] = self.lit;
    }
}

/// Writes its only parameter, or that parameter's modulated buffer, to `out`.
struct Mirror {
    value: f32,
}

impl Module for Mirror {
    fn process(&mut self, _ctx: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let samples = io.modulated(0);
        let ([], [out]) = io.split::<0, 1>();
        for (i, sample) in out.iter_mut().enumerate() {
            *sample = samples.map_or(self.value, |values| values[i]);
        }
    }

    fn set_param(&mut self, _index: usize, value: f32) {
        self.value = value;
    }
}

fn mirror(_config: &ModuleConfig) -> Box<dyn Module> {
    Box::new(Mirror { value: 0.0 })
}

macro_rules! mirror_module {
    ($(#[$doc:meta])* $name:ident, $id:literal, $param:expr) => {
        $(#[$doc])*
        pub static $name: ModuleDescriptor = ModuleDescriptor {
            id: $id,
            version: 1,
            name: $id,
            category: "Test",
            description: "Writes its parameter, or the modulated buffer.",
            inputs: &[],
            outputs: &[PortSpec {
                name: "out",
                label: "Out",
                signal: SignalType::Modulation,
                description: "The parameter, or its modulated value.",
                default: PortDefault::Value(0.0),
                polarity: Polarity::Bipolar,
                channels: Channels::PerChannel,
            }],
            params: &[$param],
            role: ModuleRole::Dsp(mirror),
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
mod tests {
    use super::*;
    use loom_core::{ParamMapping, WidgetKind};

    #[test]
    fn every_test_module_registers() {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        assert_eq!(registry.len(), ALL.len());
        for descriptor in ALL {
            assert!(descriptor.id.starts_with("test."), "{}", descriptor.id);
        }
    }

    #[test]
    fn the_panel_has_every_widget_kind() {
        let kinds: Vec<WidgetKind> = PANEL.panel.widgets.iter().map(|w| w.kind).collect();
        let knob = |name: &str| {
            let w = PANEL.panel.widgets.iter().find(|w| w.bind == name).unwrap();
            assert_eq!(w.kind, WidgetKind::Knob, "{name}");
            PANEL.params[PANEL.param_index(name).unwrap()]
        };
        assert!(!knob("level").options.integer);
        assert!(knob("steps").options.integer);
        assert!(kinds.contains(&WidgetKind::Toggle));
        assert!(kinds.contains(&WidgetKind::Radio));
        assert!(kinds.contains(&WidgetKind::Button {
            mode: "mode",
            hold: "held"
        }));
        assert!(kinds.contains(&WidgetKind::Light));
        assert!(kinds.contains(&WidgetKind::Meter));
    }

    #[test]
    fn the_panel_has_every_mapping_and_polarity() {
        let params = PANEL.params;
        assert!(params.iter().any(|p| p.options.mapping == ParamMapping::Linear));
        assert!(params.iter().any(|p| p.options.mapping == ParamMapping::Logarithmic));
        assert!(params.iter().any(|p| p.options.integer));
        assert!(params.iter().any(|p| p.options.smooth_seconds > 0.0));
        let polarities: Vec<Polarity> = PANEL.outputs.iter().map(|p| p.polarity).collect();
        assert_eq!(polarities, [Polarity::Unipolar, Polarity::Bipolar]);
    }

    #[test]
    fn the_panel_writes_level_and_lights_on() {
        let factory = match PANEL.role {
            ModuleRole::Dsp(factory) => factory,
            _ => unreachable!(),
        };
        let mut module = factory(&ModuleConfig {
            sample_rate: 48_000.0,
            max_block: 4,
        });
        module.set_param(LEVEL, 0.75);
        module.set_param(ON, 1.0);
        let (mut uni, mut bi) = (vec![0.0; 4], vec![0.0; 4]);
        loom_core::testing::run(module.as_mut(), 48_000.0, &[], &[], &mut [&mut uni, &mut bi], &[]);
        assert_eq!(uni, [0.75; 4]);
        assert_eq!(bi, [0.75; 4]);
        let mut lights = [0.0; 1];
        module.displays(&mut lights);
        assert_eq!(lights, [1.0]);
    }
}
