//! Static module descriptions.

mod display;
mod format;
mod lint;
mod panel;
mod param;
mod port;

pub use display::{DisplayFormat, DisplayKind, DisplayRange, DisplaySpec, MAX_DISPLAYS};
pub use format::number_text;
pub use lint::{MissingText, SpecKind, Undocumented, assert_documented};
pub use panel::{ButtonMode, Panel, Widget, WidgetKind};
pub use param::{NotANumber, ParamMapping, ParamOptions, ParamRange, ParamSpec, ShownWhen};
pub use port::{Channels, Polarity, PortDefault, PortRange, PortSpec};

use crate::module::Module;

/// Settings passed to a module factory.
#[derive(Debug, Clone, Copy)]
pub struct ModuleConfig {
    /// Sample rate in Hz.
    pub sample_rate: f32,
    /// Largest number of frames the module will be asked to process at once.
    pub max_block: usize,
}

/// How the engine runs a module.
#[derive(Clone, Copy)]
pub enum ModuleRole {
    /// A normal module created by this factory.
    Dsp(fn(&ModuleConfig) -> Box<dyn Module>),
    /// A one-sample delay, handled by the engine itself. It can break
    /// feedback loops without adding extra delay (ADR-0002).
    /// Must have exactly one input and one output, and no parameters or
    /// display values.
    UnitDelay,
    /// Sends its two inputs (left, right) to the engine output.
    /// Must have exactly two inputs and no outputs, and no parameters or
    /// display values.
    AudioOutput,
}

impl std::fmt::Debug for ModuleRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleRole::Dsp(_) => f.write_str("Dsp"),
            ModuleRole::UnitDelay => f.write_str("UnitDelay"),
            ModuleRole::AudioOutput => f.write_str("AudioOutput"),
        }
    }
}

/// One of the eight groups a module belongs to (ADR-0029). It sets the
/// tile's header colour and the module's place in the browser. The
/// descriptor's `category` stays as a finer label for search and help.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Family {
    /// Oscillators, noise, and other sound makers.
    Sources,
    /// Filters, shapers, delays, and other processors.
    FiltersAndEffects,
    /// Envelopes, LFOs, and random sources that move parameters.
    Modulation,
    /// The transport, clocks, phase, gates, and sequencers.
    TimeAndRhythm,
    /// Pitches, scales, and note effects.
    NotesAndPitch,
    /// Arithmetic, comparison, and gate logic.
    LogicAndMath,
    /// Levels, mixing, routing, and utilities that shape any signal.
    LevelAndRouting,
    /// Audio and MIDI in and out, displays, and controls for the player.
    Interface,
}

impl Family {
    /// Every family, in browser order.
    pub const ALL: [Self; 8] = [
        Self::Sources,
        Self::FiltersAndEffects,
        Self::Modulation,
        Self::TimeAndRhythm,
        Self::NotesAndPitch,
        Self::LogicAndMath,
        Self::LevelAndRouting,
        Self::Interface,
    ];

    /// The name users see.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sources => "Sources",
            Self::FiltersAndEffects => "Filters and effects",
            Self::Modulation => "Modulation",
            Self::TimeAndRhythm => "Time and rhythm",
            Self::NotesAndPitch => "Notes and pitch",
            Self::LogicAndMath => "Logic and math",
            Self::LevelAndRouting => "Level and routing",
            Self::Interface => "Interface",
        }
    }

    /// The family's place in [`Family::ALL`], which also indexes its
    /// colour in the theme's palette.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Everything the engine and editor need to know about a module type.
#[derive(Debug)]
pub struct ModuleDescriptor {
    /// Stable, namespaced ID, for example `"loom.osc.sine"`.
    pub id: &'static str,
    /// Version of the module's ports and parameters. Increase it when they
    /// change in a way that needs a patch migration.
    pub version: u32,
    /// Display name.
    pub name: &'static str,
    /// A finer label than the family, for search and help.
    pub category: &'static str,
    /// The group that sets the tile's colour and browser place (ADR-0029).
    pub family: Family,
    /// One-line description.
    pub description: &'static str,
    /// Inputs (at most 64).
    pub inputs: &'static [PortSpec],
    /// Outputs.
    pub outputs: &'static [PortSpec],
    /// Parameters.
    pub params: &'static [ParamSpec],
    /// How the engine runs it.
    pub role: ModuleRole,
    /// What the module's tile shows between its port columns (ADR-0013).
    pub panel: Panel,
    /// The values the module publishes for displays, at most
    /// [`MAX_DISPLAYS`]. It writes them in
    /// [`Module::displays`](crate::Module::displays).
    pub displays: &'static [DisplaySpec],
}

impl ModuleDescriptor {
    /// Index of the input with this name.
    ///
    /// A `const fn`, so a module can name its ports by looking them up in
    /// its own descriptor at compile time. A wrong name is then a build
    /// error, and reordering the descriptor cannot leave an index behind:
    ///
    /// ```
    /// # static SVF: loom_core::ModuleDescriptor = loom_core::ModuleDescriptor {
    /// #     id: "doc.svf", version: 1, name: "SVF", category: "Doc", family: loom_core::Family::FiltersAndEffects, description: "",
    /// #     inputs: &[], outputs: &[], params: &[], role: loom_core::ModuleRole::UnitDelay,
    /// #     panel: loom_core::Panel::EMPTY, displays: &[],
    /// # };
    /// # const _: Option<usize> = SVF.input_index("in");
    /// // In a module: const IN: usize = SVF.input_index("in").unwrap();
    /// ```
    pub const fn input_index(&self, name: &str) -> Option<usize> {
        port_position(self.inputs, name)
    }

    /// Index of the output with this name. A `const fn`, like
    /// [`input_index`](Self::input_index).
    pub const fn output_index(&self, name: &str) -> Option<usize> {
        port_position(self.outputs, name)
    }

    /// Index of the parameter with this name. A `const fn`, like
    /// [`input_index`](Self::input_index).
    pub const fn param_index(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.params.len() {
            if same_name(self.params[i].name, name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// True when parameter `index` shows its widget for these values
    /// (ADR-0030): always, unless it is `shown_when` a choice that is not
    /// the current one.
    pub fn is_shown(&self, index: usize, values: &[f32]) -> bool {
        let Some(when) = self.params.get(index).and_then(|p| p.options.shown_when) else {
            return true;
        };
        let Some(mode) = self.param_index(when.param) else {
            return true;
        };
        let value = values.get(mode).copied().unwrap_or(self.params[mode].default);
        let choice = (value - self.params[mode].min).round();
        when.choices.iter().any(|&c| c as f32 == choice)
    }

    /// Index of a display value by name.
    pub fn display_index(&self, name: &str) -> Option<usize> {
        self.displays.iter().position(|d| d.name == name)
    }

    /// Parameter values as the patch keeps them: each transient parameter
    /// at its default, every other value as given. Undo, copy, and paste
    /// use this, so a live gesture is never an edit or copied
    /// (transient-params.md).
    pub fn saved_values(&self, values: &[f32]) -> Vec<f32> {
        values
            .iter()
            .enumerate()
            .map(|(index, &value)| match self.params.get(index) {
                Some(spec) if spec.is_transient() => spec.default,
                _ => value,
            })
            .collect()
    }

    /// The range a loaded module gets for parameter `index`, and whether the
    /// loader reports it, as [`ParamSpec::load_range`] says. A parameter on a
    /// range widget shares its range: the pair keeps a saved range they agree
    /// on that holds both values, and otherwise takes the smallest that does
    /// (control-ranges.md, Ranges on a range widget). `saved` and `value` give
    /// a parameter's saved range and its value, clamped to `min` and `max`.
    pub fn load_range(
        &self,
        index: usize,
        saved: impl Fn(&ParamSpec) -> Option<ParamRange>,
        value: impl Fn(&ParamSpec) -> f32,
    ) -> Option<(ParamRange, bool)> {
        let spec = self.params.get(index)?;
        let Some(other) = self.range_partner(index).and_then(|i| self.params.get(i)) else {
            return spec.load_range(saved(spec), value(spec));
        };
        let default = spec.default_range()?;
        let (a, b) = (value(spec), value(other));
        let holds = |range: &ParamRange| range.contains(a) && range.contains(b);
        let smallest = spec
            .options
            .ranges
            .iter()
            .copied()
            .find(|range| holds(range))
            .unwrap_or(ParamRange::new(spec.min, spec.max));
        Some(match (saved(spec), saved(other)) {
            (None, None) if holds(&default) => (default, false),
            (None, None) => (smallest, false),
            (Some(x), Some(y)) if x == y && spec.options.ranges.contains(&x) && holds(&x) => (x, false),
            _ => (smallest, true),
        })
    }

    /// The `"id@version"` string used in patch files.
    pub fn kind(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }

    /// Default parameter values.
    pub fn default_params(&self) -> Vec<f32> {
        self.params.iter().map(|p| p.default).collect()
    }
}

/// Index of the port named `name` in `ports`.
const fn port_position(ports: &[PortSpec], name: &str) -> Option<usize> {
    let mut i = 0;
    while i < ports.len() {
        if same_name(ports[i].name, name) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// `a == b`, in a `const fn`, where `str` equality is not available yet.
const fn same_name(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod tests;
