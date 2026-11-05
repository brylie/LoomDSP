//! Panels: the widgets on a module tile and the checks that keep them valid.

use super::{DisplayKind, DisplaySpec, MAX_DISPLAYS, ModuleDescriptor, ParamMapping, ParamSpec, ShownWhen};

/// Most values a select list can offer. A list this long still scrolls and
/// types ahead in reasonable time, and a larger range belongs on a number.
pub const MAX_SELECT_VALUES: usize = 128;

/// A module's panel: the controls and displays on its tile, laid out on the
/// canvas grid (ADR-0013). Spec: `docs/design/panels.md`, Module panels on
/// tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panel {
    /// Width in grid units.
    pub width: u8,
    /// Height in grid units.
    pub height: u8,
    /// Widgets, in keyboard focus order.
    pub widgets: &'static [Widget],
}

impl Panel {
    /// A panel with nothing on it.
    pub const EMPTY: Self = Self::new(0, 0, &[]);

    /// A panel of `width` by `height` grid units.
    pub const fn new(width: u8, height: u8, widgets: &'static [Widget]) -> Self {
        Self { width, height, widgets }
    }
}

/// One control or display on a panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Widget {
    /// What it is.
    pub kind: WidgetKind,
    /// Column of its top-left corner, in grid units from the panel's left.
    pub x: u8,
    /// Row of its top-left corner, in grid units from the panel's top.
    pub y: u8,
    /// What it shows: a parameter name for controls, a display value name
    /// for lights, an output name for meters and scopes. Private, because a
    /// widget can bind a second parameter: a lookup by name asks
    /// [`Widget::binds`], so it cannot miss the second one.
    pub(super) bind: &'static str,
    /// The tile's caption, when it differs from the bound item's label,
    /// such as "1" under each mixer strip's level (panels.md, Panel layout,
    /// principle 10). Hover text and the inspector keep the full label.
    /// `Some("")` shows no caption. Set with [`Widget::captioned`].
    pub caption: Option<&'static str>,
    /// When a display widget shows: only while this choice parameter holds
    /// one of these choices. `None` always shows. A control follows its
    /// parameter's `shown_when` instead. Set with [`Widget::shown_when`].
    pub shown_when: Option<ShownWhen>,
}

impl Widget {
    /// The name this widget shows first: its parameter, or `x` on an XY pad
    /// and `low` on a range; a display value; or an output.
    pub const fn bind(&self) -> &'static str {
        self.bind
    }

    /// The second parameter of a widget that binds two: `y` on an XY pad and
    /// `high` on a range (panel-widgets.md, Two numbers).
    pub const fn second(&self) -> Option<&'static str> {
        match self.kind {
            WidgetKind::XyPad { y } => Some(y),
            WidgetKind::Range { high, .. } => Some(high),
            _ => None,
        }
    }

    /// True if this widget shows the item named `name`, as either of its
    /// parameters. A button's `mode` and `hold` do not count, because the
    /// button does not show them.
    pub fn binds(&self, name: &str) -> bool {
        self.bind == name || self.second() == Some(name)
    }

    /// The parameter names of a control: one, or two on an XY pad and a range.
    /// A button's `mode` and `hold` are left out, as in [`Widget::binds`].
    pub fn params(&self) -> impl Iterator<Item = &'static str> + use<> {
        let first = self.is_control().then_some(self.bind);
        first.into_iter().chain(self.second())
    }

    /// The same widget with its own tile caption (principle 10).
    pub const fn captioned(mut self, caption: &'static str) -> Self {
        self.caption = Some(caption);
        self
    }

    /// The same display widget, shown only while the choice parameter `param`
    /// holds one of `choices`, counted from 0. Its columns close up when it
    /// hides, so the panel gets narrower (panel-widgets.md, Level history).
    /// The registry refuses it on a control, which follows its parameter's
    /// `shown_when`.
    pub const fn shown_when(mut self, param: &'static str, choices: &'static [u32]) -> Self {
        self.shown_when = Some(ShownWhen { param, choices });
        self
    }

    /// An XY scope: output `x_output` across and output `y_output` up, from
    /// one paired probe (panel-widgets.md, XY scope).
    pub const fn xy_scope(x: u8, y: u8, x_output: &'static str, y_output: &'static str) -> Self {
        Self {
            kind: WidgetKind::XyScope { y: y_output },
            x,
            y,
            bind: x_output,
            caption: None,
            shown_when: None,
        }
    }

    /// A spectrum on output `output`: its frequency content, from a scope
    /// probe (panel-widgets.md, Spectrum).
    pub const fn spectrum(x: u8, y: u8, output: &'static str) -> Self {
        Self {
            kind: WidgetKind::Spectrum,
            x,
            y,
            bind: output,
            caption: None,
            shown_when: None,
        }
    }

    /// A level history on output `output`: its RMS and peak over the last
    /// seconds, from the same probe as a meter (panel-widgets.md, Level
    /// history).
    pub const fn level_history(x: u8, y: u8, output: &'static str) -> Self {
        Self {
            kind: WidgetKind::LevelHistory,
            x,
            y,
            bind: output,
            caption: None,
            shown_when: None,
        }
    }

    /// An XY pad: `horizontal` follows the point left and right, and `vertical`
    /// up and down. Neither parameter's label names the pair, so it takes its
    /// own caption (panel-widgets.md, XY pad).
    pub const fn xy_pad(x: u8, y: u8, horizontal: &'static str, vertical: &'static str, caption: &'static str) -> Self {
        Self {
            kind: WidgetKind::XyPad { y: vertical },
            x,
            y,
            bind: horizontal,
            caption: Some(caption),
            shown_when: None,
        }
    }

    /// A range: a horizontal track with two handles, for `low` and `high` on
    /// one scale. Neither parameter's label names the pair, so it takes its own
    /// caption (panel-widgets.md, Range).
    pub const fn range(x: u8, y: u8, low: &'static str, high: &'static str, caption: &'static str) -> Self {
        Self {
            kind: WidgetKind::Range { high, vertical: false },
            x,
            y,
            bind: low,
            caption: Some(caption),
            shown_when: None,
        }
    }

    /// A range on a vertical track.
    pub const fn range_vertical(x: u8, y: u8, low: &'static str, high: &'static str, caption: &'static str) -> Self {
        Self {
            kind: WidgetKind::Range { high, vertical: true },
            x,
            y,
            bind: low,
            caption: Some(caption),
            shown_when: None,
        }
    }

    /// A knob for parameter `param`.
    pub const fn knob(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Knob,
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// A vertical fader for any parameter: a track with a handle, 2 × 6 grid
    /// units (panel-widgets.md, Fader).
    pub const fn fader(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Fader { vertical: true },
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// A horizontal fader for any parameter, 6 × 2 grid units.
    pub const fn fader_horizontal(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Fader { vertical: false },
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// A number shown as text, dragged like a knob, for any parameter
    /// (panel-widgets.md, Number).
    pub const fn number(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Number,
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// A toggle for a whole-number parameter from 0 to 1.
    pub const fn toggle(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Toggle,
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// Radio buttons for a whole-number parameter with choice names
    /// ([`ParamSpec::choices`]), one button per name. The first is the
    /// minimum value. The names come from the parameter, so a panel cannot
    /// show other words than the inspector.
    pub const fn radio(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Radio,
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// A segmented control: one joined button per choice name, `width`
    /// grid units wide, for a parameter with 2 to 4 named values.
    pub const fn segmented(x: u8, y: u8, width: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Segmented { width },
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// A compact choice list for a whole-number parameter with named values.
    pub const fn select(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Select,
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// A light for display value `display`.
    pub const fn light(x: u8, y: u8, display: &'static str) -> Self {
        Self {
            kind: WidgetKind::Light,
            x,
            y,
            bind: display,
            caption: None,
            shown_when: None,
        }
    }

    /// A display value as text, `width` grid units wide: 3, or 6 for longer
    /// text (panel-widgets.md, Readout). The display value's range says how
    /// it is written.
    pub const fn readout(x: u8, y: u8, width: u8, display: &'static str) -> Self {
        Self {
            kind: WidgetKind::Readout { width },
            x,
            y,
            bind: display,
            caption: None,
            shown_when: None,
        }
    }

    /// A level meter on output `output`, fed by a port probe.
    pub const fn meter(x: u8, y: u8, output: &'static str) -> Self {
        Self {
            kind: WidgetKind::Meter,
            x,
            y,
            bind: output,
            caption: None,
            shown_when: None,
        }
    }

    /// A scope on output `output`: the signal over time, from a port probe
    /// (`docs/design/panels.md`, Drawing the scope trace).
    pub const fn scope(x: u8, y: u8, output: &'static str) -> Self {
        Self {
            kind: WidgetKind::Scope,
            x,
            y,
            bind: output,
            caption: None,
            shown_when: None,
        }
    }

    /// A read-only result shape above its controls. `sample` maps the module's
    /// parameter values, a horizontal position in 0..1, and the sample rate to a
    /// vertical position in 0..1. A digital filter's response depends on the
    /// sample rate, so a preview of one needs it. It runs on the UI thread,
    /// never in audio processing.
    pub const fn shape(
        x: u8,
        y: u8,
        width: u8,
        output: &'static str,
        label: &'static str,
        sample: fn(&[f32], f32, f32) -> f32,
    ) -> Self {
        Self {
            kind: WidgetKind::Shape { width, label, sample },
            x,
            y,
            bind: output,
            caption: None,
            shown_when: None,
        }
    }

    /// A large button for whole-number parameters from 0 to 1.
    ///
    /// `mode` names a whole-number parameter, from 0 to 1, that chooses the
    /// gesture: 0 flips `param` (the saved latch) on press, and 1 holds
    /// `hold` high while the button is held. `hold` is usually
    /// [transient](ParamSpec::transient), so a hold is never saved. The
    /// three are different parameters. The canvas does not read `mode`. The
    /// editor does, through [`ButtonMode`]. It does not have to be a widget.
    /// Spec: `docs/design/clock.md`, Button details.
    pub const fn button(x: u8, y: u8, param: &'static str, hold: &'static str, mode: &'static str) -> Self {
        Self {
            kind: WidgetKind::Button { mode, hold },
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// A button that is always momentary: `param` is high while it is held.
    /// It has no mode parameter (its `mode` is empty) and writes `param`
    /// only, which should be transient. The Transport's Reset uses it.
    pub const fn momentary(x: u8, y: u8, param: &'static str) -> Self {
        Self {
            kind: WidgetKind::Button { mode: "", hold: param },
            x,
            y,
            bind: param,
            caption: None,
            shown_when: None,
        }
    }

    /// True for widgets bound to a parameter.
    pub const fn is_control(&self) -> bool {
        !matches!(
            self.kind,
            WidgetKind::Light
                | WidgetKind::Readout { .. }
                | WidgetKind::Meter
                | WidgetKind::Scope
                | WidgetKind::LevelHistory
                | WidgetKind::Spectrum
                | WidgetKind::XyScope { .. }
                | WidgetKind::Shape { .. }
        )
    }

    /// True for widgets that can be bound to one of the module's outputs, fed
    /// by a port probe: meters, scopes, level histories, and spectra. A meter can show
    /// a display value instead: see [`ModuleDescriptor::meter_display`].
    pub const fn shows_output(&self) -> bool {
        matches!(
            self.kind,
            WidgetKind::Meter
                | WidgetKind::Scope
                | WidgetKind::LevelHistory
                | WidgetKind::Spectrum
                | WidgetKind::XyScope { .. }
        )
    }
}

/// The kinds of widget a panel can hold. More come with later phases.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum WidgetKind {
    /// A dial for any parameter.
    Knob,
    /// A straight track with a handle, for any parameter.
    Fader {
        /// True for a vertical track, false for a horizontal one.
        vertical: bool,
    },
    /// A value shown as text, dragged like a knob, for any parameter.
    Number,
    /// Off or on, for a whole-number parameter from 0 to 1.
    Toggle,
    /// One button per choice name, for a whole-number parameter that names
    /// its values. The names come from the parameter.
    Radio,
    /// A compact box that opens a list of named choices.
    Select,
    /// A row of joined buttons, one per choice name, for 2 to 4 named
    /// choices (panel-widgets.md, Segmented control).
    Segmented {
        /// Width in panel grid units. The segments share it equally.
        width: u8,
    },
    /// A display value as brightness, clamped to 0 to 1.
    Light,
    /// A display value as text.
    Readout {
        /// Width in panel grid units: 3, or 6 for longer text.
        width: u8,
    },
    /// The level of one of the module's outputs, from a port probe: an RMS
    /// bar and a peak line, -60 dB to 0 dB.
    Meter,
    /// One of the module's outputs over time, from a port probe: a
    /// triggered trace in a frame.
    Scope,
    /// One of the module's outputs over the last seconds, from a port probe:
    /// RMS as a filled shape and the peak as a line (panel-widgets.md, Level
    /// history).
    LevelHistory,
    /// The frequency content of one of the module's outputs, from a port probe
    /// (panel-widgets.md, Spectrum).
    Spectrum,
    /// Two of the module's outputs against each other, from one paired probe
    /// (panel-widgets.md, XY scope). The widget's `bind` is the output drawn
    /// across.
    XyScope {
        /// The output drawn up.
        y: &'static str,
    },
    /// Read-only shape sampled from the module's parameters on the UI thread.
    Shape {
        /// Width in panel grid units.
        width: u8,
        /// Caption shown beneath the read-only result.
        label: &'static str,
        /// Pure sampler: parameter values, horizontal position, and sample rate
        /// to a vertical position.
        sample: fn(&[f32], f32, f32) -> f32,
    },
    /// A square with a point, for two parameters at once (panel-widgets.md,
    /// XY pad). The widget's `bind` is the horizontal parameter.
    XyPad {
        /// The vertical parameter.
        y: &'static str,
    },
    /// A track with two handles, for a low and a high value on one scale
    /// (panel-widgets.md, Range). The widget's `bind` is the low parameter.
    Range {
        /// The high parameter.
        high: &'static str,
        /// True for a vertical track, false for a horizontal one.
        vertical: bool,
    },
    /// A large button for whole-number parameters from 0 to 1.
    ///
    /// The widget's `bind` is the toggle latch. `mode` names the parameter
    /// that chooses the gesture, and `hold` the parameter a momentary press
    /// holds high.
    Button {
        /// Whole-number parameter from 0 to 1: 0 flips the latch on press,
        /// 1 holds `hold` while pressed.
        mode: &'static str,
        /// Whole-number parameter from 0 to 1 that is high while a
        /// momentary press lasts. Usually transient.
        hold: &'static str,
    },
}

impl WidgetKind {
    /// True for a widget that shows one number on a position, and sets it
    /// through the parameter's mapping (panel-widgets.md, One number). It
    /// takes typed values.
    ///
    /// `WidgetKind` is `non_exhaustive`, so a new kind compiles at every
    /// `_` arm. Code that treats number controls alike asks this, not a list.
    pub const fn is_one_number(&self) -> bool {
        matches!(self, Self::Knob | Self::Fader { .. } | Self::Number)
    }

    /// True for a widget that draws a modulation assignment's reach as a
    /// range, and a live value on it: an arc or a bar (rule 3). Other controls
    /// show a dot.
    pub const fn has_range(&self) -> bool {
        matches!(self, Self::Knob | Self::Fader { .. }) || self.is_two_numbers()
    }

    /// True for a widget that sets two numbers through their parameters'
    /// mappings (panel-widgets.md, Two numbers). It takes no typed value.
    pub const fn is_two_numbers(&self) -> bool {
        matches!(self, Self::XyPad { .. } | Self::Range { .. })
    }

    /// True for a widget whose position is a parameter's place along its
    /// mapping, from 0 to 1, not an index of choices.
    pub const fn is_mapped(&self) -> bool {
        self.is_one_number() || self.is_two_numbers()
    }

    /// True for a control that takes a dropped output as a modulation
    /// assignment (modulation.md, Making an assignment).
    pub const fn takes_modulation(&self) -> bool {
        self.is_mapped() || matches!(self, Self::Toggle | Self::Radio | Self::Select | Self::Segmented { .. })
    }
}

impl PartialEq for WidgetKind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fader { vertical: a }, Self::Fader { vertical: b }) => a == b,
            (Self::XyPad { y: a }, Self::XyPad { y: b }) => a == b,
            (Self::Range { high: a, vertical: av }, Self::Range { high: b, vertical: bv }) => a == b && av == bv,
            (Self::Readout { width: a }, Self::Readout { width: b }) => a == b,
            (Self::Knob, Self::Knob)
            | (Self::Number, Self::Number)
            | (Self::Toggle, Self::Toggle)
            | (Self::Radio, Self::Radio)
            | (Self::Select, Self::Select)
            | (Self::Light, Self::Light)
            | (Self::Meter, Self::Meter)
            | (Self::Scope, Self::Scope)
            | (Self::LevelHistory, Self::LevelHistory)
            | (Self::Spectrum, Self::Spectrum) => true,
            (Self::XyScope { y: a }, Self::XyScope { y: b }) => a == b,
            (Self::Segmented { width: a }, Self::Segmented { width: b }) => a == b,
            // Kinds compare their visible form. Function addresses are not
            // stable identities for a shape sampler across codegen units.
            (
                Self::Shape {
                    width: a, label: al, ..
                },
                Self::Shape {
                    width: b, label: bl, ..
                },
            ) => a == b && al == bl,
            (Self::Button { mode: am, hold: ah }, Self::Button { mode: bm, hold: bh }) => am == bm && ah == bh,
            _ => false,
        }
    }
}

impl Eq for WidgetKind {}

/// How a [`WidgetKind::Button`] answers a gesture.
///
/// The canvas reports a press, a release, or an arrow. The editor uses this
/// to choose the parameter value, so the canvas stays a reporter of input
/// (ADR-0010). Spec: `docs/design/clock.md`, Button details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonMode {
    /// A press flips the gate. A release leaves it.
    Toggle,
    /// The gate is high only while the button is held.
    Momentary,
}

impl ButtonMode {
    /// `1` is momentary. Any lower value is toggle.
    ///
    /// A mode parameter is a whole number from 0 to 1, so these are the only
    /// values the editor stores.
    #[must_use]
    pub fn from_value(value: f32) -> Self {
        if value >= 1.0 { Self::Momentary } else { Self::Toggle }
    }

    /// The value to write when the button is pressed.
    ///
    /// Toggle flips `current`. Momentary goes high.
    #[must_use]
    pub fn press(self, current: f32) -> Option<f32> {
        Some(match self {
            Self::Toggle => {
                if current >= 0.5 {
                    0.0
                } else {
                    1.0
                }
            }
            Self::Momentary => 1.0,
        })
    }

    /// The value to write when the hold ends.
    ///
    /// Toggle keeps its gate, so this is `None`. Momentary goes low.
    #[must_use]
    pub fn release(self) -> Option<f32> {
        match self {
            Self::Toggle => None,
            Self::Momentary => Some(0.0),
        }
    }

    /// The value to write for an arrow key, if the key changes anything.
    ///
    /// `high` is up or right. Toggle sets 0 or 1. Momentary ignores arrows.
    /// A toggle that is already there returns `None`.
    #[must_use]
    pub fn arrow(self, current: f32, high: bool) -> Option<f32> {
        match self {
            Self::Momentary => None,
            Self::Toggle => {
                let next = if high { 1.0 } else { 0.0 };
                ((current >= 0.5) != high).then_some(next)
            }
        }
    }
}

impl ModuleDescriptor {
    /// Width and height of `widget` in grid units. Radio buttons are one row
    /// per choice name of their parameter, plus one for the label, and wide
    /// enough that the built-in choice names are not cut (#394).
    pub fn widget_size(&self, widget: &Widget) -> (u8, u8) {
        match widget.kind {
            WidgetKind::Knob => (3, 3),
            WidgetKind::Fader { vertical: true } | WidgetKind::Range { vertical: true, .. } => (2, 6),
            WidgetKind::Fader { vertical: false } | WidgetKind::Range { vertical: false, .. } => (6, 2),
            WidgetKind::XyPad { .. } => (6, 6),
            WidgetKind::Number => (5, 2),
            WidgetKind::Readout { width } => (width, 2),
            WidgetKind::Meter => (3, 6),
            WidgetKind::Scope => (9, 5),
            WidgetKind::LevelHistory => (9, 6),
            WidgetKind::Spectrum => (9, 5),
            WidgetKind::XyScope { .. } => (6, 6),
            WidgetKind::Shape { width, .. } => (width, 5),
            WidgetKind::Toggle | WidgetKind::Light => (3, 2),
            WidgetKind::Select => (6, 2),
            WidgetKind::Segmented { width } => (width, 2),
            WidgetKind::Button { .. } => (6, 3),
            WidgetKind::Radio => {
                let choices = self
                    .param_index(widget.bind)
                    .map_or(0, |index| self.params[index].options.choices.len());
                (6, u8::try_from(choices + 1).unwrap_or(u8::MAX))
            }
        }
    }

    /// True when `widget` shows for these parameter values: a control while
    /// its parameter does, and a display under its own condition. Parameter
    /// values that are missing count as the defaults.
    pub fn is_widget_shown(&self, widget: &Widget, values: &[f32]) -> bool {
        if widget.is_control() {
            return self.param_index(widget.bind).is_none_or(|i| self.is_shown(i, values));
        }
        let Some(when) = widget.shown_when else {
            return true;
        };
        let Some(mode) = self.param_index(when.param) else {
            return true;
        };
        let value = values.get(mode).copied().unwrap_or(self.params[mode].default);
        let choice = (value - self.params[mode].min).round();
        when.choices.iter().any(|&c| c as f32 == choice)
    }

    /// The display value a meter shows, or `None` for any other widget and for
    /// a meter on an output.
    pub fn meter_display(&self, widget: &Widget) -> Option<&DisplaySpec> {
        if widget.kind != WidgetKind::Meter || self.outputs.iter().any(|port| port.name == widget.bind) {
            return None;
        }
        self.displays.iter().find(|display| display.name == widget.bind)
    }

    /// The parameter a control widget writes and shows, given the module's
    /// parameter values. A button in momentary mode uses its `hold`; every
    /// other control uses its `bind`. `None` if a name is missing or the
    /// widget is not a control. Spec: `docs/design/clock.md`, Button details.
    pub fn control_param(&self, widget: &Widget, params: &[f32]) -> Option<usize> {
        if !widget.is_control() {
            return None;
        }
        if let WidgetKind::Button { hold, .. } = widget.kind
            && self.button_mode(widget, params)? == ButtonMode::Momentary
        {
            return self.param_index(hold);
        }
        self.param_index(widget.bind)
    }

    /// How a button widget answers a gesture, from its mode parameter. A
    /// button with no mode parameter ([`Widget::momentary`]) is always
    /// momentary. `None` for another widget or a missing name.
    pub fn button_mode(&self, widget: &Widget, params: &[f32]) -> Option<ButtonMode> {
        let WidgetKind::Button { mode, .. } = widget.kind else {
            return None;
        };
        if mode.is_empty() {
            return Some(ButtonMode::Momentary);
        }
        let value = params.get(self.param_index(mode)?).copied()?;
        Some(ButtonMode::from_value(value))
    }

    /// Checks the display values, the choice names, the ranges, and the panel: every widget binds to
    /// something this module has, fits its binding, lies inside the panel,
    /// and overlaps no other widget. The registry runs this check.
    pub fn check_panel(&self) -> Result<(), String> {
        if self.displays.len() > MAX_DISPLAYS {
            return Err(format!("more than {MAX_DISPLAYS} display values"));
        }
        for param in self.params {
            param.check_choices()?;
            param.check_ranges()?;
            if let Some(when) = param.options.shown_when {
                let problem = |text: &str| {
                    Err(format!(
                        "parameter `{}`: shown when `{}`: {text}",
                        param.name, when.param
                    ))
                };
                let Some(mode) = self.param_index(when.param).map(|i| &self.params[i]) else {
                    return problem("no such parameter");
                };
                // Choice names, or an on and off switch (0 or 1).
                let count = if mode.options.choices.is_empty() {
                    if mode.options.integer && mode.min == 0.0 && mode.max == 1.0 {
                        2
                    } else {
                        0
                    }
                } else {
                    mode.options.choices.len()
                };
                if count == 0 {
                    return problem("needs a parameter with choice names, or a whole number from 0 to 1");
                }
                if when.choices.is_empty() || when.choices.iter().any(|&c| c as usize >= count) {
                    return problem("names a choice the parameter does not have");
                }
            }
        }
        for param in self.params {
            if let Some(step) = param.options.key_step
                && (step <= 0.0
                    || param.options.integer
                    || param.options.mapping != ParamMapping::Linear
                    || !param.options.ranges.is_empty())
            {
                return Err(format!(
                    "parameter `{}`: a key step needs a linear parameter that is not a whole number and offers no control ranges, and a size above 0",
                    param.name
                ));
            }
        }
        let panel = &self.panel;
        for (i, widget) in panel.widgets.iter().enumerate() {
            let name = |problem: &str| format!("panel widget `{}` ({:?}): {problem}", widget.bind, widget.kind);
            if let Some(when) = widget.shown_when {
                if widget.is_control() {
                    return Err(name("a control follows its parameter's condition, not its own"));
                }
                let Some(mode) = self.param_index(when.param).map(|i| &self.params[i]) else {
                    return Err(name("shown when a parameter this module does not have"));
                };
                let count = mode.options.choices.len();
                if count == 0 || when.choices.is_empty() || when.choices.iter().any(|&c| c as usize >= count) {
                    return Err(name("shown when a choice the parameter does not have"));
                }
            }
            if widget.is_control() {
                let Some(index) = self.param_index(widget.bind) else {
                    return Err(name("no such parameter"));
                };
                // Copied before `param` is borrowed: a button names a second
                // parameter, and the two borrows cannot overlap.
                let gate = |p: &ParamSpec| p.options.integer && p.min == 0.0 && p.max == 1.0;
                if let WidgetKind::Button { mode, hold } = widget.kind
                    && mode.is_empty()
                {
                    // Always momentary: one parameter, the hold.
                    if hold != widget.bind {
                        return Err(name("a button without a mode must hold its own parameter"));
                    }
                } else if let WidgetKind::Button { mode, hold } = widget.kind {
                    if mode == widget.bind || mode == hold {
                        return Err(name("mode must be a different parameter"));
                    }
                    if hold == widget.bind {
                        return Err(name("hold must be a different parameter"));
                    }
                    let Some(mode_index) = self.param_index(mode) else {
                        return Err(name("no such mode parameter"));
                    };
                    if !gate(&self.params[mode_index]) {
                        return Err(name("mode must be a whole-number parameter from 0 to 1"));
                    }
                    let Some(hold_index) = self.param_index(hold) else {
                        return Err(name("no such hold parameter"));
                    };
                    if !gate(&self.params[hold_index]) {
                        return Err(name("hold must be a whole-number parameter from 0 to 1"));
                    }
                }
                let param = &self.params[index];
                let integer = param.options.integer;
                if let Some(second) = widget.second() {
                    let Some(other) = self.params.iter().find(|p| p.name == second) else {
                        return Err(name("no such second parameter"));
                    };
                    if second == widget.bind {
                        return Err(name("a widget with two values needs two different parameters"));
                    }
                    if widget.caption.is_none_or(|caption| caption.trim().is_empty()) {
                        return Err(name("a widget with two values needs its own caption"));
                    }
                    if param.options.shown_when != other.options.shown_when {
                        return Err(name("its two parameters must be shown for the same choices"));
                    }
                    // Its arrows move by a share of the track, which would skip
                    // whole numbers or never reach the next one.
                    if param.options.integer || other.options.integer {
                        return Err(name("a widget with two values cannot bind a whole-number parameter"));
                    }
                    // Both handles sit on one track, so one position must mean
                    // one value for both (panel-widgets.md, Range).
                    if matches!(widget.kind, WidgetKind::Range { .. })
                        && (param.min != other.min
                            || param.max != other.max
                            || param.options.mapping != other.options.mapping
                            || param.options.ranges != other.options.ranges
                            || param.options.default_range != other.options.default_range)
                    {
                        return Err(name(
                            "its two parameters must have the same range, mapping, and control ranges",
                        ));
                    }
                }
                match widget.kind {
                    WidgetKind::Toggle if !(integer && param.min == 0.0 && param.max == 1.0) => {
                        return Err(name("a toggle needs a whole-number parameter from 0 to 1"));
                    }
                    WidgetKind::Radio if !integer || param.options.choices.is_empty() => {
                        return Err(name("radio buttons need a whole-number parameter with choice names"));
                    }
                    WidgetKind::Segmented { width }
                        if !integer || !(2..=4).contains(&param.options.choices.len()) || width == 0 =>
                    {
                        return Err(name(
                            "a segmented control needs a width and 2 to 4 named whole-number choices",
                        ));
                    }
                    WidgetKind::Select if !integer || !(1..=MAX_SELECT_VALUES).contains(&param.value_count()) => {
                        return Err(name(
                            "a select list needs a whole-number parameter with 1 to 128 values",
                        ));
                    }
                    WidgetKind::Button { .. } if !(integer && param.min == 0.0 && param.max == 1.0) => {
                        return Err(name("a button needs a whole-number parameter from 0 to 1"));
                    }
                    _ => {}
                }
            } else if let WidgetKind::Readout { width } = widget.kind {
                let Some(display) = self.displays.iter().find(|display| display.name == widget.bind) else {
                    return Err(name("a readout needs a display value"));
                };
                // Reading a trigger takes it, so only the watcher may.
                if display.kind == DisplayKind::Trigger {
                    return Err(name("a readout cannot show a trigger, because reading one takes it"));
                }
                if width != 3 && width != 6 {
                    return Err(name("a readout is 3 or 6 grid units wide"));
                }
            } else if self.meter_display(widget).is_some() {
                // A meter on a display value (panel-widgets.md).
            } else if widget.shows_output() || matches!(widget.kind, WidgetKind::Shape { .. }) {
                if !self.outputs.iter().any(|port| port.name == widget.bind) {
                    return Err(name("no such output"));
                }
                if let WidgetKind::XyScope { y } = widget.kind
                    && (y == widget.bind || !self.outputs.iter().any(|port| port.name == y))
                {
                    return Err(name("an XY scope binds two different outputs of its own module"));
                }
                if let WidgetKind::Shape { width, label, .. } = widget.kind
                    && (width == 0 || label.trim().is_empty())
                {
                    return Err(name("a shape needs a width and a caption"));
                }
            } else if self.display_index(widget.bind).is_none() {
                return Err(name("no such display value"));
            }
            let (w, h) = self.widget_size(widget);
            if u16::from(widget.x) + u16::from(w) > u16::from(panel.width)
                || u16::from(widget.y) + u16::from(h) > u16::from(panel.height)
            {
                return Err(name("outside the panel"));
            }
            for other in &panel.widgets[..i] {
                let (ow, oh) = self.widget_size(other);
                let (x, y, w, h) = (u16::from(widget.x), u16::from(widget.y), u16::from(w), u16::from(h));
                let (ox, oy, ow, oh) = (u16::from(other.x), u16::from(other.y), u16::from(ow), u16::from(oh));
                let apart = x >= ox + ow || ox >= x + w || y >= oy + oh || oy >= y + h;
                if !apart {
                    return Err(name(&format!("overlaps `{}`", other.bind)));
                }
            }
        }
        Ok(())
    }
}
