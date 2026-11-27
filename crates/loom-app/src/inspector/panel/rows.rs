//! A module's, an unknown module's, and a slot's inspector as `loom-ui`
//! elements (plan 3.2, row 4d.2), drawn from the same rows as the UI
//! snapshot. Every part's UI ID follows the nesting on screen: an ID under
//! another is inside it (ui-layer.md, section 4). Where Iced's IDs did not,
//! they moved (the 4d.2 rename table): the mapping line is
//! `<row>/midi/line/…`, its settings `<row>/midi/settings/…`, Add
//! modulation and its sources `<row>/modulation/…`, a choice's radios
//! `<row>/value/<choice>`, and a slot's part-only rows
//! `<slot>/<part>/others/…`.

use super::super::{
    AssignmentRow, DrivesGroup, ExposureRow, MappingRow, ModuleRows, ParamControl, ParamMode, ParamRow, RangeRow,
    SlotRows, SourceRow, UnknownLink, UnknownRows, ValueField, encoding_name, encoding_of, mode_name,
};
use crate::style::{self, Role as ButtonRole};
use loom_core::{MidiEncoding, MidiMode};
use loom_ui::access::{Access, Action, Role, States, Value};
use loom_ui::{Align, Drag, Element, Length, Padding, Select, SelectStyle, Slider};
use loom_ui_core::{Color, IdKey, UiId};

/// The colours and looks the rows are painted in, from the style tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Looks {
    pub(super) text: Color,
    pub(super) quiet: Color,
    pub(super) field: style::FieldLook,
    pub(super) select: SelectStyle,
    pub(super) small_select: SelectStyle,
    pub(super) quiet_select: SelectStyle,
    pub(super) danger: Color,
    pub(super) drop: Color,
    pub(super) card_fill: Color,
    pub(super) card_border: Color,
}

/// What the rows need beyond their own model: their looks, and the row a
/// dragged output is over, while one is dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Shown {
    pub(super) looks: Looks,
    pub(super) drop_row: Option<(u32, usize)>,
}

/// The key each button and field of the rows takes first: Escape gives
/// the keys back to the canvas, as from the toolbar.
const ESCAPE: [loom_ui_core::Key; 1] = [loom_ui_core::Key::Escape];

/// The width of a parameter's value field, as Iced's was.
const VALUE_FIELD_WIDTH: f32 = 84.0;

/// The width of a MIDI range field.
const RANGE_FIELD_WIDTH: f32 = 72.0;

/// The width of the MIDI mode and encoding selects.
const MIDI_SELECT_WIDTH: f32 = 120.0;

/// The width of the Range select.
const RANGE_SELECT_WIDTH: f32 = 96.0;

/// The width of a modulation card's depth text.
const DEPTH_TEXT_WIDTH: f32 = 40.0;

/// A column whose children start at its left edge, as Iced's did: a part
/// that fills the width still fills it, and a button or a select that
/// takes its own width keeps it, not stretched across the column.
fn col(children: impl IntoIterator<Item = Element>) -> Element {
    Element::column(children).align_x(Align::Start)
}

/// The last key of `id`: what an element is keyed by under its parent.
fn last(id: &UiId) -> IdKey {
    id.keys().last().cloned().expect("an inspector part's UI ID has keys")
}

/// A module's inspector: its name and Help, its description, Reset clip,
/// each parameter's row, Drives, and its ports.
pub(super) fn module(m: &ModuleRows, shown: Shown) -> Element {
    let looks = shown.looks;
    let mut parts = vec![
        Element::row([
            Element::text(m.name.clone(), style::HEADING_TEXT)
                .colour(looks.text)
                .width(Length::FILL),
            button("help", "Help", &format!("Help for {}", m.name), ButtonRole::Secondary),
        ])
        .align_y(Align::Center)
        .width(Length::FILL),
        body(m.description, looks.text)
            .key("description")
            .access(Access::label(m.description)),
    ];
    if m.resets_clip {
        parts.push(button("reset_clip", "Reset clip", "Reset clip", ButtonRole::Secondary));
    }
    if m.params.is_empty() {
        parts.push(
            body("No parameters.", looks.quiet)
                .key("no_params")
                .access(Access::label("No parameters.")),
        );
    }
    if !m.params.is_empty() {
        let rows = m.params.iter().map(|p| param_row(m.id, p.label, p, None, shown));
        parts.push(rows_column(rows, "param"));
    }
    if !m.drives.is_empty() {
        parts.push(drives(&m.drives, looks));
    }
    if !m.ports.is_empty() {
        parts.push(ports(m, looks));
    }
    col(parts).spacing(6.0).width(Length::FILL)
}

/// A column of rows keyed `key`, so each row's ID is `…/<key>/<name>`.
fn rows_column(rows: impl IntoIterator<Item = Element>, key: &'static str) -> Element {
    col(rows).spacing(6.0).width(Length::FILL).key(key)
}

/// **Drives**: one group per output, each line a button that selects its
/// target (`<t>/drives/<output>/<assignment>`).
fn drives(groups: &[DrivesGroup], looks: Looks) -> Element {
    let mut parts = vec![Element::text("Drives", style::CONTROL_TEXT).colour(looks.text)];
    for group in groups {
        let mut lines = vec![Element::text(group.output, style::SMALL_TEXT).colour(looks.quiet)];
        for line in &group.lines {
            let name = format!("{}: {}", group.output, line.text);
            let colour = if line.enabled { looks.text } else { looks.quiet };
            lines.push(
                Element::text(line.text.clone(), style::SMALL_TEXT)
                    .colour(colour)
                    .padding(Padding::symmetric(2.0, 8.0))
                    .width(Length::FILL)
                    .looks(ButtonRole::Plain.looks())
                    .key(line.id.0)
                    .focusable()
                    .takes_first(ESCAPE)
                    .access(Access::new(Role::Button, name).action(Action::Click)),
            );
        }
        parts.push(col(lines).spacing(4.0).width(Length::FILL).key(group.output));
    }
    col(parts)
        .spacing(4.0)
        .width(Length::FILL)
        .key("drives")
        .access(Access::new(Role::Group, "Drives"))
}

/// One line per port under Inputs and Outputs, its description in hover
/// text (`<t>/port/<direction>/<label>`).
fn ports(m: &ModuleRows, looks: Looks) -> Element {
    let mut groups: Vec<(&'static str, Vec<Element>)> = Vec::new();
    for port in &m.ports {
        if groups.last().is_none_or(|(direction, _)| *direction != port.direction) {
            groups.push((port.direction, Vec::new()));
        }
        let words = format!("{} {}: {}", port.direction, port.label, port.details);
        let line = Element::row([
            Element::text(port.label, style::SMALL_TEXT)
                .colour(looks.text)
                .width(Length::FILL),
            Element::text(port.details.clone(), style::SMALL_TEXT).colour(looks.quiet),
        ])
        .width(Length::FILL)
        .hover_text(port.description.clone())
        .key(port.label)
        .access(Access::label(words).description(port.description.clone()));
        groups.last_mut().expect("pushed above").1.push(line);
    }
    let columns = groups.into_iter().flat_map(|(direction, lines)| {
        let heading = if direction == "Input" { "Inputs" } else { "Outputs" };
        [
            Element::text(heading, style::SMALL_TEXT).colour(looks.quiet),
            col(lines).width(Length::FILL).key(direction),
        ]
    });
    col(columns).spacing(2.0).width(Length::FILL).key("port")
}

/// One parameter's row (`<row>` is its UI ID): its label line, with the
/// Expose buttons inside a sub-patch and the value field at the right, its
/// value control and Range, its MIDI mapping, and its modulation. `label`
/// is the parameter's label, or the surface name on a sub-patch tile.
pub(super) fn param_row(module: u32, label: &str, p: &ParamRow, own: Option<&ExposureRow>, shown: Shown) -> Element {
    let looks = shown.looks;
    let name = match &p.unused {
        Some(unused) => format!("{label}, {unused}"),
        None => label.to_string(),
    };
    let mut access = Access::new(Role::Group, name);
    if !p.description.is_empty() {
        access = access.description(p.description);
    }
    let mut parts = Vec::new();
    match &p.mode {
        ParamMode::MidiClock => {
            let words = format!("{label}   {}   ·   MIDI clock", p.value);
            let heard = format!("{label} {}, controlled by MIDI clock", p.value);
            parts.push(body(&words, looks.text).key("clock").access(Access::label(heard)));
        }
        ParamMode::Learn { armed, detail } => {
            parts.push(midi(module, p, label, Some((*armed, detail.as_str())), looks));
        }
        ParamMode::Edit {
            position,
            step,
            control,
            assignments,
            adding,
        } => {
            parts.push(head(label, p, own, looks));
            let ghost = p.mapping_options.as_ref().and_then(|row| row.ghost);
            let value = value_control(label, p, *position, *step, control, ghost, looks);
            parts.push(match &p.range {
                Some(range) => Element::row([value, range_select(range, looks)])
                    .spacing(6.0)
                    .align_y(Align::Center)
                    .width(Length::FILL),
                None => value,
            });
            if let Some(field) = p.field.as_ref().and_then(|field| field.error.clone()) {
                parts.push(
                    small(&field, looks.quiet)
                        .key("field_error")
                        .access(Access::label(field)),
                );
            }
            if p.mapping.is_some() {
                parts.push(midi(module, p, label, None, looks));
            }
            if p.accepts_drop || !assignments.is_empty() {
                parts.extend(modulation(assignments, adding.as_deref(), p.accepts_drop, looks));
            }
        }
    }
    let mut row = col(parts).spacing(2.0).width(Length::FILL);
    // A dragged output over a row that takes it lights the row
    // (modulation.md); the row hears the pointer come and go.
    if p.accepts_drop {
        row = row.reports_hover();
        if shown.drop_row == Some((module, p.index)) {
            row = row.background(looks.drop).radius(style::DROP_ROW_RADIUS);
        }
    }
    row.key(last(&p.ui)).access(access)
}

/// The label line: the label, with the description and the patch name in
/// hover text, the Expose buttons, the "not used" note, and the value
/// field or the value as text.
fn head(label: &str, p: &ParamRow, own: Option<&ExposureRow>, looks: Looks) -> Element {
    let hover = match &p.name {
        Some(name) => format!("{}\n{name}", p.description),
        None => p.description.to_string(),
    };
    let mut line = vec![
        Element::text(label.to_string(), style::BODY_TEXT)
            .colour(looks.text)
            .hover_text(hover),
    ];
    let exposures: Vec<(&ExposureRow, bool)> = own
        .map(|exposure| (exposure, true))
        .into_iter()
        .chain(p.exposure.as_ref().map(|exposure| (exposure, false)))
        .collect();
    if !exposures.is_empty() {
        let buttons = exposures
            .into_iter()
            .map(|(exposure, own)| expose(exposure, own, looks));
        // The buttons wrap too: two surfaces, each with the name it is
        // exposed as, can be wider than the column on their own.
        line.push(
            Element::row(buttons)
                .spacing(6.0)
                .align_y(Align::Center)
                .wrap()
                .key("expose"),
        );
    }
    line.push(Element::space(Length::FILL, Length::Shrink));
    if let Some(unused) = &p.unused {
        line.push(Element::text(unused.clone(), style::SMALL_TEXT).colour(looks.quiet));
    }
    line.push(match &p.field {
        Some(field) => value_field(label, field, &p.value, looks),
        None => Element::text(p.value.clone(), style::BODY_TEXT)
            .colour(looks.quiet)
            .key("shown")
            .access(Access::label(p.value.clone())),
    });
    // It wraps, so a row inside a sub-patch with two Expose buttons and
    // its value field still shows them all in the column's width.
    Element::row(line)
        .spacing(6.0)
        .align_y(Align::Center)
        .width(Length::FILL)
        .wrap()
}

/// The Expose or Unexpose button for its surface, keyed by that role, with
/// the name it is exposed as before it.
fn expose(exposure: &ExposureRow, own: bool, looks: Looks) -> Element {
    let label = exposure.button_label();
    let key = if own { "own" } else { "enclosing" };
    let (name, shown_name) = match &exposure.name {
        Some(name) => (format!("{label}, exposed as {name}"), Some(format!("as {name}"))),
        None => (label.clone(), None),
    };
    let button = Element::text(label, style::SMALL_TEXT)
        .padding(Padding::all(1.0))
        .looks(ButtonRole::Plain.looks())
        .key(key)
        .focusable()
        .takes_first(ESCAPE)
        .access(Access::new(Role::Button, name).action(Action::Click));
    match shown_name {
        Some(words) => Element::row([Element::text(words, style::SMALL_TEXT).colour(looks.quiet), button])
            .spacing(4.0)
            .align_y(Align::Center),
        None => button,
    }
}

/// The value field: the typed value, with a danger outline and its reason
/// in hover text while the text is not a value (panel-widgets.md, Typing
/// a value).
fn value_field(label: &str, field: &ValueField, value: &str, looks: Looks) -> Element {
    let (border, width) = match &field.error {
        Some(_) => (looks.danger, style::INVALID_BORDER),
        None => (looks.field.border, style::RULE),
    };
    let mut access = Access::new(Role::TextInput, format!("{label} value"));
    if let Some(reason) = &field.error {
        access = access.description(reason.clone());
    }
    let mut element = Element::field(field.text.clone(), style::BODY_TEXT)
        .placeholder(value.to_string(), looks.quiet)
        .padding(Padding::all(1.0))
        .background(looks.field.fill)
        .border(width, border)
        .radius(looks.field.radius)
        .width(Length::Fixed(VALUE_FIELD_WIDTH))
        .key("field")
        .takes_first(ESCAPE)
        .access(access);
    if let Some(reason) = &field.error {
        element = element.hover_text(reason.clone());
    }
    element
}

/// The value control (`<row>/value`): a slider, its choices as radio
/// buttons (`<row>/value/<choice>`), or a select.
fn value_control(
    label: &str,
    p: &ParamRow,
    position: f32,
    step: f32,
    control: &ParamControl,
    ghost: Option<f32>,
    looks: Looks,
) -> Element {
    match control {
        ParamControl::Slider => {
            // A whole-number parameter steps one value by key and by drag;
            // a continuous one 1% of the travel by key, 0.1% with Shift, and
            // drags freely (the decisions row of 2026-11-11).
            // By the parameter's kind, as `param::slider_step` decides
            // the step: a seed spans 1 to 65535, so its step is far under
            // 0.001 and still one value (CodeRabbit on #900).
            let whole = p.spec.options.integer && p.spec.max > p.spec.min;
            // A double-click resets it to the parameter's default, placed
            // on the travel this row shows: the knob's, or the exposed
            // range on a sub-patch row (ui-layer.md, section 5, rule 16).
            let default = p.spec.normalize(p.spec.default).clamp(0.0, 1.0);
            let slider = Slider::new(position).ghost(ghost).default(default);
            let slider = if whole {
                slider.key_step(step).fine_key_step(step).drag(Drag::Step(step))
            } else {
                slider
            };
            // The number a screen reader hears, as the item list gives it.
            let number = crate::a11y::items::Number::parameter(&p.spec, position, Some(step));
            Element::slider(slider).key("value").access(
                Access::new(Role::Slider, label)
                    .description(p.value.clone())
                    .value(Value::Number {
                        value: number.value,
                        min: number.min,
                        max: number.max,
                        step: number.step,
                    })
                    .action(Action::Increment)
                    .action(Action::Decrement)
                    .action(Action::SetValue),
            )
        }
        ParamControl::Radio { choices, selected } => {
            let radios = choices.iter().enumerate().map(|(at, name)| {
                Element::radio(*name, style::BODY_TEXT, at == *selected).key(choice_key(&p.spec, at, name))
            });
            Element::radio_group(label, radios).spacing(3.0).key("value")
        }
        ParamControl::PickList { choices, selected } => {
            let options = choices.iter().map(ToString::to_string).collect();
            Element::select(Select::new(label, options, Some(*selected), looks.select))
                .key("value")
                .width(Length::FILL)
        }
    }
}

/// A choice's key under `<row>/value`: its name for a list whose names
/// switch with a count, so each keeps its node, otherwise its index.
pub(crate) fn choice_key(spec: &loom_core::ParamSpec, at: usize, name: &str) -> IdKey {
    if spec.options.count_choices.is_some() {
        IdKey::from(name)
    } else {
        IdKey::from(at as u32)
    }
}

/// The **Range** select: Full first, then each range (Brylie's decision,
/// 2026-11-25). Full is chosen while no listed range holds the value, and
/// choosing it sets the parameter's full range.
fn range_select(range: &RangeRow, looks: Looks) -> Element {
    let mut options = vec![FULL.to_string()];
    options.extend(range.choices.iter().cloned());
    let chosen = range.selected.map_or(0, |at| at + 1);
    Element::select(Select::new("Range", options, Some(chosen), looks.small_select))
        .width(Length::Fixed(RANGE_SELECT_WIDTH))
        .hover_text("Range: what a full turn of the control covers.")
        .key("range")
}

/// The Range select's first option: the parameter's full range.
pub(crate) const FULL: &str = "Full";

/// A row's MIDI part (`<row>/midi`): the mapping line, with Learn in learn
/// mode, the control it is mapped to otherwise, and Options and × while it
/// has a mapping; then the settings, while they are open. `learn` is
/// learn mode's armed state and detail.
fn midi(module: u32, p: &ParamRow, label: &str, learn: Option<(bool, &str)>, looks: Looks) -> Element {
    let _ = module;
    let mut line = Vec::new();
    let heard = match learn {
        Some((armed, detail)) => {
            let role = if armed {
                ButtonRole::Primary
            } else {
                ButtonRole::Secondary
            };
            let name = format!("Learn {label} {detail}").trim_end().to_string();
            line.push(
                Element::row([
                    Element::text(label.to_string(), style::BODY_TEXT).width(Length::FILL),
                    Element::text(detail.to_string(), style::BODY_TEXT),
                ])
                .padding(style::BUTTON_PADDING)
                .radius(role.tokens().radius)
                .looks(role.looks())
                .width(Length::FILL)
                .key("learn")
                .focusable()
                .takes_first(ESCAPE)
                .access(
                    Access::new(Role::Button, name)
                        .value(Value::On(armed))
                        .action(Action::Click),
                ),
            );
            "MIDI mapping".to_string()
        }
        None => {
            let control = p.mapping.clone().unwrap_or_default();
            line.push(
                Element::text(control.clone(), style::SMALL_TEXT)
                    .colour(looks.quiet)
                    .width(Length::FILL),
            );
            format!("MIDI: {control}")
        }
    };
    if p.mapping.is_some() {
        line.push(
            Element::text("Options", style::SMALL_TEXT)
                .padding(Padding::all(1.0))
                .looks(ButtonRole::Plain.looks())
                .key("options")
                .focusable()
                .takes_first(ESCAPE)
                .access(Access::new(Role::Button, "MIDI mapping options").action(Action::Click)),
        );
        line.push(remove("remove", "Remove MIDI mapping"));
    }
    let mut parts = vec![
        Element::row(line)
            .spacing(4.0)
            .align_y(Align::Center)
            .width(Length::FILL)
            .key("line")
            .access(Access::new(Role::Group, heard)),
    ];
    if let Some(row) = p.mapping_options.as_ref().filter(|row| row.open) {
        parts.push(settings(row, looks));
    }
    col(parts).spacing(2.0).width(Length::FILL).key("midi")
}

/// A mapping's settings (`<row>/midi/settings`): mode, then range and
/// invert, or the encoding for Relative (midi-learn.md, Mapping modes).
fn settings(row: &MappingRow, looks: Looks) -> Element {
    let options = row.options;
    let label = |words: &'static str| Element::text(words, style::SMALL_TEXT).colour(looks.quiet);
    let line = |parts: Vec<Element>| {
        Element::row(parts)
            .spacing(4.0)
            .align_y(Align::Center)
            .width(Length::FILL)
    };
    let modes = [
        MidiMode::Absolute,
        MidiMode::Pickup,
        MidiMode::Relative(encoding_of(options.mode).unwrap_or_default()),
    ];
    let at = modes
        .iter()
        .position(|mode| std::mem::discriminant(mode) == std::mem::discriminant(&options.mode));
    let names = modes.iter().map(|mode| mode_name(*mode).to_string()).collect();
    let mut parts = vec![line(vec![
        label("Mode"),
        Element::space(Length::FILL, Length::Shrink),
        Element::select(Select::new("MIDI mode", names, at, looks.small_select))
            .width(Length::Fixed(MIDI_SELECT_WIDTH))
            .key("mode"),
    ])];
    if let Some(encoding) = encoding_of(options.mode) {
        let names = ENCODINGS.iter().map(|e| encoding_name(*e).to_string()).collect();
        let at = ENCODINGS.iter().position(|e| *e == encoding);
        parts.push(line(vec![
            label("Encoding"),
            Element::space(Length::FILL, Length::Shrink),
            Element::select(Select::new("Encoding", names, at, looks.small_select))
                .width(Length::Fixed(MIDI_SELECT_WIDTH))
                .key("encoding"),
        ]));
    } else {
        let bad = row.range_error.is_some();
        let field = |text: &str, key: &'static str, name: &'static str| {
            let (border, width) = if bad {
                (looks.danger, style::INVALID_BORDER)
            } else {
                (looks.field.border, style::RULE)
            };
            let mut access = Access::new(Role::TextInput, name);
            if let Some(reason) = &row.range_error {
                access = access.description(reason.clone());
            }
            Element::field(text.to_string(), style::SMALL_TEXT)
                .padding(Padding::all(1.0))
                .background(looks.field.fill)
                .border(width, border)
                .radius(looks.field.radius)
                .width(Length::Fixed(RANGE_FIELD_WIDTH))
                .key(key)
                .takes_first(ESCAPE)
                .access(access)
        };
        parts.push(line(vec![
            label("Range"),
            Element::space(Length::FILL, Length::Shrink),
            field(&row.low, "low", "MIDI range low"),
            label("to"),
            field(&row.high, "high", "MIDI range high"),
        ]));
        if let Some(reason) = &row.range_error {
            parts.push(
                small(reason, looks.quiet)
                    .key("range_error")
                    .access(Access::label(reason.clone())),
            );
        }
        parts.push(Element::checkbox("Invert", style::SMALL_TEXT, options.invert).key("invert"));
    }
    if let Some(note) = &row.range_note {
        parts.push(
            small(note, looks.quiet)
                .key("range_note")
                .access(Access::label(note.clone())),
        );
    }
    col(parts).spacing(2.0).width(Length::FILL).key("settings")
}

/// A Relative mapping's encodings, in the order the select lists them.
pub(crate) const ENCODINGS: [MidiEncoding; 3] = [
    MidiEncoding::Offset64,
    MidiEncoding::TwosComplement,
    MidiEncoding::SignBit,
];

/// A row's modulation: its assignments' cards (`<row>/assignment/<id>`),
/// then, on a row that takes new ones, Add modulation and its sources
/// (`<row>/modulation/…`).
fn modulation(
    assignments: &[AssignmentRow],
    adding: Option<&[SourceRow]>,
    allow_add: bool,
    looks: Looks,
) -> Vec<Element> {
    let mut parts = Vec::new();
    if !assignments.is_empty() {
        let cards = assignments.iter().map(|a| card(a, looks));
        parts.push(col(cards).spacing(6.0).width(Length::FILL).key("assignment"));
    }
    if !allow_add {
        return parts;
    }
    let (words, name) = if adding.is_some() {
        ("Cancel", "Cancel adding modulation")
    } else {
        ("+ Add modulation", "Add modulation")
    };
    let mut add = vec![
        Element::text(words, style::SMALL_TEXT)
            .padding(Padding::symmetric(2.0, 8.0))
            .radius(ButtonRole::Secondary.tokens().radius)
            .looks(ButtonRole::Secondary.looks())
            .key("add")
            .focusable()
            .takes_first(ESCAPE)
            .access(Access::new(Role::Button, name).action(Action::Click)),
    ];
    if let Some(sources) = adding {
        // One column per source module, keyed by it, holding its ports'
        // buttons, so each is `…/source/<module>/<port>` once.
        let mut modules: Vec<(u32, Vec<Element>)> = Vec::new();
        for s in sources {
            let module = s.source.module.0;
            match modules.iter_mut().find(|(seen, _)| *seen == module) {
                Some((_, buttons)) => buttons.push(source(s)),
                None => modules.push((module, vec![source(s)])),
            }
        }
        let columns = modules
            .into_iter()
            .map(|(module, buttons)| col(buttons).spacing(1.0).width(Length::FILL).key(module));
        let column = col(columns).spacing(1.0).width(Length::FILL).key("source");
        add.push(column.padding(Padding::symmetric(0.0, 8.0)));
    }
    parts.push(col(add).spacing(6.0).width(Length::FILL).key("modulation"));
    parts
}

/// One source offered while adding a modulation
/// (`<row>/modulation/source/<module>/<port>`); a source that cannot be
/// used is turned off.
fn source(s: &SourceRow) -> Element {
    let role = ButtonRole::Secondary;
    let access = Access::new(Role::Button, format!("Modulate from {}", s.name));
    let access = if s.enabled {
        access.action(Action::Click)
    } else {
        access.states(States {
            disabled: true,
            ..States::default()
        })
    };
    Element::text(s.name.clone(), style::SMALL_TEXT)
        .padding(Padding::symmetric(2.0, 6.0))
        .radius(role.tokens().radius)
        .looks(role.looks())
        .width(Length::FILL)
        .key(s.source.port as u32)
        .focusable()
        .takes_first(ESCAPE)
        .access(access)
}

/// One assignment's card, as the matrix's without its target line: On,
/// the source, ×, the polarity, and the depth with its text.
fn card(a: &AssignmentRow, looks: Looks) -> Element {
    let polarities = a.polarities.iter().map(ToString::to_string).collect();
    let at = a.polarities.iter().position(|choice| *choice == a.polarity);
    col([
        Element::row([
            Element::checkbox("On", style::SMALL_TEXT, a.enabled).key("on"),
            Element::text(a.source.clone(), style::SMALL_TEXT)
                .colour(looks.text)
                .width(Length::FILL),
            remove("remove", "Remove modulation"),
        ])
        .spacing(4.0)
        .align_y(Align::Center)
        .width(Length::FILL),
        Element::select(Select::new("Polarity", polarities, at, looks.quiet_select))
            .width(Length::Shrink)
            .key("polarity"),
        Element::row([
            Element::slider(
                Slider::new((a.depth + 1.0) / 2.0)
                    .centred()
                    .key_step(0.005)
                    .fine_key_step(0.0005),
            )
            .key("depth")
            .access(
                Access::new(Role::Slider, "Depth")
                    .description(a.depth_text.clone())
                    .value(Value::Number {
                        value: crate::modulation::percent_value(a.depth),
                        min: -100.0,
                        max: 100.0,
                        step: Some(1.0),
                    })
                    .action(Action::Increment)
                    .action(Action::Decrement)
                    .action(Action::SetValue),
            ),
            Element::text(a.depth_text.clone(), style::SMALL_TEXT)
                .colour(looks.text)
                .width(Length::Fixed(DEPTH_TEXT_WIDTH)),
        ])
        .spacing(6.0)
        .align_y(Align::Center)
        .width(Length::FILL),
    ])
    .spacing(2.0)
    .padding(6.0)
    .width(Length::FILL)
    .background(looks.card_fill)
    .border(style::RULE, looks.card_border)
    .radius(style::CARD_RADIUS)
    .key(a.id.0)
    .access(Access::new(Role::Group, format!("Modulation from {}", a.source)))
}

/// An unknown module's inspector: its kind, what it is, its Label field
/// and Apply, its saved values, and each link with its ×.
pub(super) fn unknown(m: &UnknownRows, looks: Looks) -> Element {
    const ABOUT: &str =
        "This version of LoomDSP does not have this module. It makes no sound. Its settings are kept and saved.";
    let mut parts = vec![
        Element::text(m.kind.clone(), style::HEADING_TEXT)
            .colour(looks.text)
            .key("kind")
            .access(Access::label(m.kind.clone())),
        body(ABOUT, looks.text).key("about").access(Access::label(ABOUT)),
        Element::text("Label", style::BODY_TEXT).colour(looks.text),
        Element::row([
            Element::field(m.label.clone(), style::CONTROL_TEXT)
                .placeholder("Label", looks.quiet)
                .padding(style::FIELD_PADDING)
                .background(looks.field.fill)
                .border(style::RULE, looks.field.border)
                .radius(looks.field.radius)
                .width(Length::FILL)
                .key("label")
                .takes_first(ESCAPE)
                .access(Access::new(Role::TextInput, "Label")),
            button("apply", "Apply", "Apply label", ButtonRole::Secondary),
        ])
        .spacing(6.0)
        .align_y(Align::Center)
        .width(Length::FILL),
    ];
    if !m.params.is_empty() {
        let saved = m.params.iter().map(|(name, value)| {
            let words = format!("{name}: {value}");
            body(&words, looks.text).key(name.as_str()).access(Access::label(words))
        });
        parts.push(col(saved).spacing(6.0).width(Length::FILL).key("saved"));
    }
    let (mut assignments, mut midi) = (Vec::new(), Vec::new());
    for link in &m.links {
        let (list, key, words) = match link {
            UnknownLink::Assignment { id, text } => (&mut assignments, IdKey::from(*id), text),
            UnknownLink::Midi { index, text } => (&mut midi, IdKey::from(*index as u32), text),
        };
        list.push(
            Element::row([
                Element::text(words.clone(), style::SMALL_TEXT)
                    .colour(looks.text)
                    .width(Length::FILL),
                remove("remove", "Remove link"),
            ])
            .align_y(Align::Center)
            .width(Length::FILL)
            .key(key)
            .access(Access::new(Role::Group, words.clone())),
        );
    }
    let mut links = Vec::new();
    if !assignments.is_empty() {
        links.push(col(assignments).spacing(6.0).width(Length::FILL).key("assignment"));
    }
    if !midi.is_empty() {
        links.push(col(midi).spacing(6.0).width(Length::FILL).key("midi"));
    }
    if !links.is_empty() {
        parts.push(col(links).spacing(6.0).width(Length::FILL).key("link"));
    }
    col(parts).spacing(6.0).width(Length::FILL)
}

/// A slot's inspector: its name, its role, the Part select, a note, the
/// part's role rows, a group of the part's other rows under its heading,
/// and Dissolve slot.
pub(super) fn slot(s: &SlotRows, shown: Shown) -> Element {
    let looks = shown.looks;
    let role = format!("Slot for {}", s.role);
    let mut parts = vec![
        Element::text(s.name.clone(), style::HEADING_TEXT)
            .colour(looks.text)
            .key("name")
            .access(Access::label(s.name.clone())),
        body(&role, looks.quiet).key("role").access(Access::label(role.clone())),
    ];
    let names = s.choices.iter().map(|choice| choice.label.clone()).collect();
    let current = s.choices.iter().position(|choice| choice.current);
    parts.push(
        Element::row([
            Element::text("Part", style::BODY_TEXT).colour(looks.text),
            Element::select(Select::new("Part", names, current, looks.select))
                .width(Length::Shrink)
                .key("part"),
        ])
        .spacing(8.0)
        .align_y(Align::Center),
    );
    if let Some(note) = &s.note {
        parts.push(body(note, looks.quiet).key("note").access(Access::label(note.clone())));
    }
    if let Some(under) = &s.rows_id {
        let mut inside = slot_rows(&s.role_params, shown);
        if !s.other_params.is_empty() {
            let heading = format!("{} only", s.part.clone().unwrap_or_default());
            let mut others = vec![body(&heading, looks.quiet)];
            others.extend(slot_rows(&s.other_params, shown));
            inside.push(
                col(others)
                    .spacing(6.0)
                    .width(Length::FILL)
                    .key("others")
                    .access(Access::new(Role::Group, heading)),
            );
        }
        // The part's keys after the slot's: its descriptor ID, or
        // `subpatch` and its key, one column each.
        let depth = super::super::ids::panel(s.id).keys().len();
        let mut element = col(inside).spacing(6.0).width(Length::FILL);
        for key in under.keys()[depth..].iter().rev() {
            element = col([element.key(key.clone())]).width(Length::FILL);
        }
        parts.push(element);
    }
    if s.part.is_some() {
        parts.push(button(
            "dissolve",
            "Dissolve slot",
            "Dissolve slot",
            ButtonRole::Secondary,
        ));
    }
    col(parts).spacing(6.0).width(Length::FILL)
}

/// A slot's rows, grouped by their kind key (`param`, `exposed`), each
/// group in order.
fn slot_rows(rows: &[(String, u32, ParamRow)], shown: Shown) -> Vec<Element> {
    let mut out: Vec<(IdKey, Vec<Element>)> = Vec::new();
    for (label, module, row) in rows {
        let keys = row.ui.keys();
        let kind = keys[keys.len() - 2].clone();
        if out.last().is_none_or(|(last, _)| *last != kind) {
            out.push((kind.clone(), Vec::new()));
        }
        out.last_mut()
            .expect("pushed above")
            .1
            .push(param_row(*module, label, row, None, shown));
    }
    out.into_iter()
        .map(|(kind, rows)| col(rows).spacing(6.0).width(Length::FILL).key(kind))
        .collect()
}

/// A line of body text in `colour`, wrapping to the column.
fn body(words: &str, colour: Color) -> Element {
    Element::text(words.to_string(), style::BODY_TEXT)
        .colour(colour)
        .wrap()
        .width(Length::FILL)
}

/// A line of small text in `colour`, wrapping to the column.
fn small(words: &str, colour: Color) -> Element {
    Element::text(words.to_string(), style::SMALL_TEXT)
        .colour(colour)
        .wrap()
        .width(Length::FILL)
}

/// A button in `role`, keyed `key`, showing `label`, named `name`.
fn button(key: &'static str, label: &'static str, name: &str, role: ButtonRole) -> Element {
    Element::text(label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(key)
        .focusable()
        .takes_first(ESCAPE)
        .access(Access::new(Role::Button, name).action(Action::Click))
}

/// The small × that removes a row's item, named and with hover text for
/// what it removes (panels.md, The inspector).
fn remove(key: &'static str, what: &'static str) -> Element {
    Element::text("×", style::CONTROL_TEXT)
        .padding(Padding::symmetric(0.0, 6.0))
        .looks(ButtonRole::Plain.looks())
        .hover_text(what)
        .key(key)
        .focusable()
        .takes_first(ESCAPE)
        .access(Access::new(Role::Button, what).action(Action::Click))
}

/// A sub-patch tile's inspector (sub-patches.md, Editor model): its name
/// field, what it is, its ports, Open, its mode rows, each exposed item's
/// row, and its Authors box, `authors` as typed while it has focus.
pub(super) fn sub_patch(s: &super::super::SubPatchRows, authors: &str, shown: Shown) -> Element {
    let looks = shown.looks;
    let mut parts = vec![
        Element::field(s.name.clone(), style::HEADING_TEXT)
            .placeholder("Sub-patch name", looks.quiet)
            .padding(style::FIELD_PADDING)
            .background(looks.field.fill)
            .border(style::RULE, looks.field.border)
            .radius(looks.field.radius)
            .width(Length::FILL)
            .key("name")
            .takes_first(ESCAPE)
            .access(Access::new(Role::TextInput, "Sub-patch name")),
        body("Sub-patch", looks.quiet)
            .key("kind")
            .access(Access::label("Sub-patch")),
    ];
    let ports: Vec<Element> = [("Inputs", &s.inputs), ("Outputs", &s.outputs)]
        .into_iter()
        .filter(|(_, ports)| !ports.is_empty())
        .map(|(label, ports)| {
            let words = format!("{label}: {}", ports.join(", "));
            body(&words, looks.text).key(label).access(Access::label(words))
        })
        .collect();
    if !ports.is_empty() {
        parts.push(col(ports).spacing(6.0).width(Length::FILL).key("ports"));
    }
    parts.push(button_named(
        "open",
        "Open",
        format!("Open {}", s.name),
        ButtonRole::Secondary,
        true,
    ));
    parts.extend(mode_rows(s, looks));
    if s.params.is_empty() {
        const NOTHING: &str = "Nothing exposed. Open it and press Expose on a parameter.";
        parts.push(
            body(NOTHING, looks.quiet)
                .key("nothing_exposed")
                .access(Access::label(NOTHING)),
        );
    } else {
        let rows = s.params.iter().map(|exposed| {
            // The Member control is the select's own: no Unexpose.
            let own = (!exposed.member).then_some(&exposed.own);
            param_row(exposed.module, &exposed.label, &exposed.row, own, shown)
        });
        parts.push(rows_column(rows, "exposed"));
    }
    parts.push(Element::text("Authors, one per line", style::BODY_TEXT).colour(looks.quiet));
    parts.push(
        Element::text_area(authors, style::CONTROL_TEXT)
            .placeholder("Author name", looks.quiet)
            .padding(style::FIELD_PADDING)
            .background(looks.field.fill)
            .border(style::RULE, looks.field.border)
            .radius(looks.field.radius)
            .width(Length::FILL)
            .key("authors")
            .takes_first(ESCAPE)
            .access(Access::new(Role::TextInput, "Authors, one per line")),
    );
    col(parts).spacing(6.0).width(Length::FILL)
}

/// The mode rows (sub-patches.md, The editor for modes): the Mode select;
/// for a layer or a select, its members, each with Open, Up, Down, and
/// Remove (`<t>/members/<member>/…`), and Add member; for a stack, Copies,
/// Stereo spread, and each spread's slider (`<t>/spread/<name>`).
fn mode_rows(s: &super::super::SubPatchRows, looks: Looks) -> Vec<Element> {
    let mut parts = Vec::new();
    let modes = loom_patch::Mode::ALL;
    let names = modes.iter().map(|mode| mode.label().to_string()).collect();
    let at = modes.iter().position(|mode| *mode == s.mode);
    parts.push(
        Element::row([
            Element::text("Mode", style::BODY_TEXT).colour(looks.text),
            Element::select(Select::new("Mode", names, at, looks.select))
                .width(Length::Shrink)
                .key("mode"),
        ])
        .spacing(8.0)
        .align_y(Align::Center),
    );
    if s.mode.has_members() {
        let last = s.members.len().saturating_sub(1);
        let mut members = vec![Element::text("Members", style::BODY_TEXT).colour(looks.quiet)];
        for (n, member) in s.members.iter().enumerate() {
            let name = format!("{}. {}", n + 1, member.name);
            let line = Element::row([
                Element::text(name.clone(), style::BODY_TEXT)
                    .colour(looks.text)
                    .width(Length::FILL),
                button_named(
                    "open",
                    "Open",
                    format!("Open {}", member.name),
                    ButtonRole::Secondary,
                    true,
                ),
                button_named(
                    "up",
                    "Up",
                    format!("Move {} up", member.name),
                    ButtonRole::Secondary,
                    n > 0,
                ),
                button_named(
                    "down",
                    "Down",
                    format!("Move {} down", member.name),
                    ButtonRole::Secondary,
                    n < last,
                ),
                button_named(
                    "remove",
                    "Remove",
                    format!("Remove {}", member.name),
                    ButtonRole::Secondary,
                    true,
                ),
            ])
            .spacing(6.0)
            .align_y(Align::Center)
            .width(Length::FILL)
            .key(member.id)
            .access(Access::new(Role::Group, name));
            members.push(line);
        }
        parts.push(
            col(members)
                .spacing(6.0)
                .width(Length::FILL)
                .key("members")
                .access(Access::new(Role::Group, "Members")),
        );
        parts.push(button_named(
            "add_member",
            "Add member",
            "Add member".to_string(),
            ButtonRole::Secondary,
            true,
        ));
    }
    if s.mode == loom_patch::Mode::Stack {
        let counts: Vec<String> = (1..=loom_patch::MOST_MEMBERS as u32).map(|n| n.to_string()).collect();
        let at = s.copies.checked_sub(1).map(|at| at as usize);
        parts.push(
            Element::row([
                Element::text("Copies", style::BODY_TEXT).colour(looks.text),
                Element::select(Select::new("Copies", counts, at, looks.select))
                    .width(Length::Shrink)
                    .key("copies"),
            ])
            .spacing(8.0)
            .align_y(Align::Center),
        );
        parts.push(Element::checkbox("Stereo spread", style::BODY_TEXT, s.stereo_spread).key("stereo_spread"));
        if !s.spread.is_empty() {
            let mut spreads = vec![Element::text("Spread", style::BODY_TEXT).colour(looks.quiet)];
            for spread in &s.spread {
                // To a tenth of a percent, the step Shift and an arrow key
                // move it.
                let percent = crate::modulation::percent_text(spread.amount);
                spreads.push(
                    Element::row([
                        Element::text(spread.name.clone(), style::BODY_TEXT).colour(looks.text),
                        Element::slider(Slider::new(spread.amount).key_step(0.01).fine_key_step(0.001))
                            .key(spread.name.as_str())
                            .access(
                                Access::new(Role::Slider, format!("Spread of {}", spread.name))
                                    .description(percent.clone())
                                    .value(Value::Number {
                                        value: crate::modulation::percent_value(spread.amount),
                                        min: 0.0,
                                        max: 100.0,
                                        step: Some(1.0),
                                    })
                                    .action(Action::Increment)
                                    .action(Action::Decrement)
                                    .action(Action::SetValue),
                            ),
                        Element::text(percent, style::BODY_TEXT).colour(looks.text),
                    ])
                    .spacing(8.0)
                    .align_y(Align::Center)
                    .width(Length::FILL),
                );
            }
            // The heading has no node of its own; each slider says what it
            // spreads.
            let column = col(spreads).spacing(6.0).width(Length::FILL).key("spread");
            parts.push(column);
        }
    }
    parts
}

/// A button in `role`, keyed `key`, showing `label`, named `name`, turned
/// off unless `on`: no stop, and its node disabled (section 6, rule 2).
fn button_named(key: &'static str, label: &'static str, name: String, role: ButtonRole, on: bool) -> Element {
    let access = Access::new(Role::Button, name);
    let access = if on {
        access.action(Action::Click)
    } else {
        access.states(States {
            disabled: true,
            ..States::default()
        })
    };
    // A turned-off element is no stop, by its node (`is_stop`), as in
    // Preferences.
    Element::text(label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(key)
        .focusable()
        .takes_first(ESCAPE)
        .access(access)
}
