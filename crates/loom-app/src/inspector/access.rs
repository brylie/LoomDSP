//! The inspector's parts as a screen reader is told them: [`Item`]s built
//! from the same [`Inspector`] model the view draws (`inspector.rs`), part
//! for part, with the same UI IDs (`ids.rs`) and the same messages.
//!
//! What must be heard goes in a part's name or its value, not its
//! description, since some platforms read a description late or not at
//! all (`docs/design/ui-layer.md`, section 7, rule 23). The one exception
//! is a slider's value with its unit, which the canvas's knobs give the
//! same way (rule 14): if the listening check finds it is not heard, both
//! change together.

use super::{
    AssignmentRow, DrivesGroup, ExposureRow, Inspector, MappingRow, Message, ModeChoice, ModuleRows, ParamControl,
    ParamMode, ParamRow, SourceRow, SubPatchRows, UnknownLink, encoding_name, encoding_of, ids, mode_name,
};
use crate::Message as AppMessage;
use crate::a11y::items::{Item, Kind, Number, SetNumber, SetText};
use loom_core::{MidiEncoding, MidiMappingOptions, MidiMode, ParamMapping};
use loom_ui_core::UiId;

/// Every part the inspector shows for `model`, under one named group per
/// panel. Empty when it shows nothing.
pub(crate) fn items(model: &Inspector) -> Vec<Item> {
    match model {
        Inspector::Patch(rows) => vec![patch(rows)],
        Inspector::Several(n) => {
            let panel = UiId::panel("inspector").with("several");
            vec![Item::text(panel, format!("{n} modules selected"))]
        }
        Inspector::Empty => Vec::new(),
        Inspector::Unknown(m) => {
            let panel = ids::panel(m.id);
            let mut parts = vec![
                Item::text(ids::part(&panel, "kind"), m.kind.clone()),
                Item::text(
                    ids::part(&panel, "about"),
                    "This version of LoomDSP does not have this module. It makes no sound. Its settings are kept and saved.",
                ),
                field(
                    ids::part(&panel, "label"),
                    "Label",
                    &m.label,
                    SetText::UnknownLabel(m.id),
                ),
                Item::button(
                    ids::part(&panel, "apply"),
                    "Apply label",
                    Some(AppMessage::Inspector(Message::UnknownLabelApply { module: m.id })),
                ),
            ];
            for (name, value) in &m.params {
                parts.push(Item::text(
                    ids::part(&panel, "saved").with(name.as_str()),
                    format!("{name}: {value}"),
                ));
            }
            for link in &m.links {
                let (key, text, message) = match link {
                    UnknownLink::Assignment { id, text } => (
                        ids::part(&panel, "link/assignment").with(*id),
                        text,
                        AppMessage::Modulation(crate::modulation::Message::Unassign(loom_core::AssignmentId(*id))),
                    ),
                    UnknownLink::Midi { index, text } => (
                        ids::part(&panel, "link/midi").with(*index as u32),
                        text,
                        AppMessage::Learn(crate::learn::Message::Unmap {
                            module: m.id,
                            index: *index,
                        }),
                    ),
                };
                parts.push(Item::group(
                    key.clone(),
                    text.clone(),
                    vec![Item::button(ids::part(&key, "remove"), "Remove link", Some(message))],
                ));
            }
            vec![Item::group(panel, m.kind.clone(), parts)]
        }
        Inspector::Module(m) => vec![module(m)],
        Inspector::SubPatch(s) => vec![sub_patch(s)],
        Inspector::Slot(s) => {
            let panel = ids::panel(s.id);
            let mut parts = vec![
                Item::text(ids::part(&panel, "name"), s.name.clone()),
                Item::text(ids::part(&panel, "role"), format!("Slot for {}", s.role)),
            ];
            let current = s.choices.iter().position(|c| c.current);
            let swap = |at: Option<usize>| {
                at.and_then(|at| s.choices.get(at)).map(|choice| AppMessage::SwapSlot {
                    slot: s.id,
                    to: choice.to.clone(),
                })
            };
            parts.push(Item {
                label: Some("Part".to_string()),
                value: Some(
                    current
                        .and_then(|at| s.choices.get(at))
                        .map_or_else(|| crate::slots::NO_PART.to_string(), |c| c.label.clone()),
                ),
                step: match current {
                    Some(at) => (swap(Some(at + 1)), swap(at.checked_sub(1))),
                    None => (swap(Some(0)), None),
                },
                ..Item::new(ids::part(&panel, "part"), Kind::Choice)
            });
            for (label, module, row) in &s.role_params {
                parts.push(param_row(*module, label, row, None));
            }
            if !s.other_params.is_empty() {
                parts.push(Item::text(
                    ids::part(&panel, "others"),
                    format!("{} only", s.part.clone().unwrap_or_default()),
                ));
                for (label, module, row) in &s.other_params {
                    parts.push(param_row(*module, label, row, None));
                }
            }
            parts.push(Item::button(
                ids::part(&panel, "dissolve"),
                "Dissolve slot",
                Some(AppMessage::DissolveSlot(s.id)),
            ));
            vec![Item::group(panel, s.name.clone(), parts)]
        }
        Inspector::Port(p) => {
            let panel = ids::panel(p.id);
            vec![Item::group(
                panel.clone(),
                p.name.clone(),
                vec![
                    Item::text(ids::part(&panel, "direction"), p.direction.clone()),
                    Item::text(
                        ids::part(&panel, "about"),
                        "A port of this sub-patch. Its name is the port's label on the tile outside.",
                    ),
                ],
            )]
        }
    }
}

/// A text field: its name, its text, what typing sets, and the widget that
/// takes focus.
fn field(id: UiId, label: &str, text: &str, set: SetText) -> Item {
    Item {
        label: Some(label.to_string()),
        value: Some(text.to_string()),
        set_text: Some(set),
        focus: Some(ids::field(&id)),
        ..Item::new(id, Kind::Field)
    }
}

/// The patch's own inspector, shown while nothing is selected.
fn patch(rows: &crate::patch_info::PatchRows) -> Item {
    use crate::patch_info::Message as Info;
    let panel = ids::patch();
    let mut parts = vec![
        Item::text(ids::part(&panel, "name"), rows.name.clone()),
        // Read only until the multi-line editor comes (phase 3): its name
        // and its text, and focus, but no SetValue.
        Item {
            label: Some("Authors, one per line".to_string()),
            value: Some(rows.authors.join("\n")),
            focus: Some(iced::advanced::widget::Id::new(crate::patch_info::AUTHORS_INPUT)),
            ..Item::new(ids::part(&panel, "authors"), Kind::Field)
        },
    ];
    for (index, tag) in rows.tags.iter().enumerate() {
        let key = ids::part(&panel, "tag").with(tag.as_str());
        parts.push(Item::group(
            key.clone(),
            format!("Tag {tag}"),
            vec![Item::button(
                ids::part(&key, "remove"),
                format!("Remove tag {tag}"),
                Some(AppMessage::PatchInfo(Info::RemoveTag(index))),
            )],
        ));
    }
    parts.push(Item {
        focus: Some(iced::advanced::widget::Id::new(crate::patch_info::TAG_INPUT)),
        ..field(ids::part(&panel, "tags"), "Tags", &rows.tag_draft, SetText::Tag)
    });
    if let Some(note) = &rows.tag_note {
        parts.push(Item::text(ids::part(&panel, "tag_note"), note.clone()));
    }
    parts.push(Item::text(
        ids::part(&panel, "licence"),
        format!("Licence: {}", rows.licence),
    ));
    parts.push(Item::button(
        ids::part(&panel, "choose_licence"),
        "Choose licence",
        Some(AppMessage::PatchInfo(Info::ChooseLicence)),
    ));
    if let Some(summary) = &rows.licence_summary {
        parts.push(Item::text(ids::part(&panel, "licence_summary"), summary.clone()));
    }
    if let Some(url) = &rows.licence_url {
        parts.push(Item::text(ids::part(&panel, "licence_url"), url.clone()));
    }
    if !rows.parts_by.is_empty() {
        parts.push(Item::text(
            ids::part(&panel, "parts_by"),
            format!("Includes parts by {}", rows.parts_by.join(", ")),
        ));
    }
    parts.push(Item::text(ids::part(&panel, "hint"), "Select a module to edit it."));
    parts.push(Item::text(
        ids::part(&panel, "hint/add"),
        "Press A over the canvas to add one.",
    ));
    parts.push(Item::button(
        ids::part(&panel, "shortcuts"),
        "Keyboard shortcuts",
        Some(AppMessage::ShowShortcuts),
    ));
    Item::group(panel, "Patch", parts)
}

/// One module's inspector.
fn module(m: &ModuleRows) -> Item {
    let panel = ids::panel(m.id);
    let mut parts = vec![
        Item::text(ids::part(&panel, "description"), m.description),
        Item::button(
            ids::part(&panel, "help"),
            format!("Help for {}", m.name),
            Some(AppMessage::ShowModuleHelp(m.id)),
        ),
    ];
    if m.resets_clip {
        parts.push(Item::button(
            ids::part(&panel, "reset_clip"),
            "Reset clip",
            Some(AppMessage::ResetClip(m.id)),
        ));
    }
    if m.params.is_empty() {
        parts.push(Item::text(ids::part(&panel, "no_params"), "No parameters."));
    }
    for row in &m.params {
        parts.push(param_row(m.id, row.label, row, None));
    }
    if !m.drives.is_empty() {
        parts.push(drives(&panel, &m.drives));
    }
    for port in &m.ports {
        parts.push(Item {
            description: Some(port.description.clone()),
            ..Item::text(
                ids::part(&panel, "port").with(port.direction).with(port.label),
                format!("{} {}: {}", port.direction, port.label, port.details),
            )
        });
    }
    Item::group(panel, m.name, parts)
}

/// The **Drives** list: each line selects the module it drives.
fn drives(panel: &UiId, groups: &[DrivesGroup]) -> Item {
    let mut lines = Vec::new();
    for group in groups {
        for (n, line) in group.lines.iter().enumerate() {
            let id = ids::part(panel, "drives").with(group.output).with(n as u32);
            lines.push(Item::button(
                id,
                format!("{}: {}", group.output, line.text),
                Some(AppMessage::Inspector(Message::SelectModule(line.target))),
            ));
        }
    }
    Item::group(ids::part(panel, "drives"), "Drives", lines)
}

/// A sub-patch tile's inspector: its name, ports, mode, and surface.
fn sub_patch(s: &SubPatchRows) -> Item {
    use crate::modes::Message as Mode;
    let panel = ids::panel(s.id);
    let mut parts = vec![
        field(
            ids::part(&panel, "name"),
            "Sub-patch name",
            &s.name,
            SetText::SubPatchName(s.id),
        ),
        Item::text(ids::part(&panel, "kind"), "Sub-patch"),
    ];
    for (label, ports) in [("Inputs", &s.inputs), ("Outputs", &s.outputs)] {
        if !ports.is_empty() {
            parts.push(Item::text(
                ids::part(&panel, "ports").with(label),
                format!("{label}: {}", ports.join(", ")),
            ));
        }
    }
    parts.push(Item::button(
        ids::part(&panel, "open"),
        format!("Open {}", s.name),
        Some(AppMessage::OpenTile(s.id)),
    ));
    let modes = loom_patch::Mode::ALL;
    let at = modes.iter().position(|mode| *mode == s.mode).unwrap_or(0);
    let set_mode = |at: Option<usize>| {
        at.and_then(|at| modes.get(at)).map(|mode| {
            AppMessage::Mode(Mode::SetMode {
                tile: s.id,
                mode: *mode,
            })
        })
    };
    parts.push(Item {
        label: Some("Mode".to_string()),
        value: Some(ModeChoice(s.mode).to_string()),
        step: (set_mode(Some(at + 1)), set_mode(at.checked_sub(1))),
        ..Item::new(ids::part(&panel, "mode"), Kind::Choice)
    });
    if s.mode.has_members() {
        let last = s.members.len().saturating_sub(1);
        let mut members = Vec::new();
        for (n, member) in s.members.iter().enumerate() {
            let key = ids::part(&panel, "member").with(member.id);
            let name = format!("{}. {}", n + 1, member.name);
            members.push(Item::group(
                key.clone(),
                name.clone(),
                vec![
                    Item::button(
                        ids::part(&key, "open"),
                        format!("Open {}", member.name),
                        Some(AppMessage::OpenTile(member.id)),
                    ),
                    Item::button(
                        ids::part(&key, "up"),
                        format!("Move {} up", member.name),
                        (n > 0).then_some(AppMessage::Mode(Mode::MoveMember {
                            member: member.id,
                            up: true,
                        })),
                    ),
                    Item::button(
                        ids::part(&key, "down"),
                        format!("Move {} down", member.name),
                        (n < last).then_some(AppMessage::Mode(Mode::MoveMember {
                            member: member.id,
                            up: false,
                        })),
                    ),
                    Item::button(
                        ids::part(&key, "remove"),
                        format!("Remove {}", member.name),
                        Some(AppMessage::Mode(Mode::RemoveMember(member.id))),
                    ),
                ],
            ));
        }
        parts.push(Item::group(ids::part(&panel, "members"), "Members", members));
        parts.push(Item::button(
            ids::part(&panel, "add_member"),
            "Add member",
            Some(AppMessage::Mode(Mode::AddMember(s.id))),
        ));
    }
    if s.mode == loom_patch::Mode::Stack {
        let most = loom_patch::MOST_MEMBERS as u32;
        let copies = |n: u32| {
            (1..=most)
                .contains(&n)
                .then_some(AppMessage::Mode(Mode::SetCopies { tile: s.id, copies: n }))
        };
        parts.push(Item {
            label: Some("Copies".to_string()),
            value: Some(s.copies.to_string()),
            step: (copies(s.copies + 1), copies(s.copies.wrapping_sub(1))),
            ..Item::new(ids::part(&panel, "copies"), Kind::Choice)
        });
        parts.push(Item {
            label: Some("Stereo spread".to_string()),
            on: Some(s.stereo_spread),
            click: Some(AppMessage::Mode(Mode::SetStereoSpread {
                tile: s.id,
                on: !s.stereo_spread,
            })),
            ..Item::new(ids::part(&panel, "stereo_spread"), Kind::Check)
        });
        for spread in &s.spread {
            let set = |amount: f32| {
                AppMessage::Mode(Mode::SetSpread {
                    tile: s.id,
                    name: spread.name.clone(),
                    amount,
                })
            };
            let percent = f64::from((spread.amount * 100.0).round());
            parts.push(Item {
                label: Some(format!("Spread of {}", spread.name)),
                number: Some(Number {
                    value: percent,
                    min: 0.0,
                    max: 100.0,
                    step: Some(1.0),
                }),
                step: (
                    (spread.amount < 1.0).then(|| set((spread.amount + 0.01).min(1.0))),
                    (spread.amount > 0.0).then(|| set((spread.amount - 0.01).max(0.0))),
                ),
                set_number: Some(SetNumber::Spread {
                    tile: s.id,
                    name: spread.name.clone(),
                }),
                groups: true,
                ..Item::new(ids::part(&panel, "spread").with(spread.name.as_str()), Kind::Slider)
            });
        }
    }
    if s.params.is_empty() {
        parts.push(Item::text(
            ids::part(&panel, "nothing_exposed"),
            "Nothing exposed. Open it and press Expose on a parameter.",
        ));
    }
    for exposed in &s.params {
        let own = (!exposed.member).then_some(&exposed.own);
        parts.push(param_row(exposed.module, &exposed.label, &exposed.row, own));
    }
    Item::group(panel, s.name.clone(), parts)
}

/// One parameter's row: a group named by its label, holding its value
/// control, its typed value, its range, its Expose buttons, its MIDI
/// mapping, and its modulation. `module` is the module whose parameter it
/// edits, as the row's messages name it.
fn param_row(module: u32, label: &str, p: &ParamRow, own: Option<&ExposureRow>) -> Item {
    let index = p.index;
    let ui = &p.ui;
    let mut parts = Vec::new();
    let mut description = Some(p.description.to_string()).filter(|text| !text.is_empty());
    if let Some(unused) = &p.unused {
        // Said in the row's name, so it is heard with the row.
        description = description
            .map(|text| format!("{text} {unused}"))
            .or(Some(unused.clone()));
    }
    for exposure in own.into_iter().chain(p.exposure.as_ref()) {
        parts.push(expose(exposure, ui));
    }
    match &p.mode {
        ParamMode::Learn { armed, detail } => {
            parts.push(Item {
                label: Some(format!("Learn {label} {detail}").trim_end().to_string()),
                on: Some(*armed),
                click: Some(AppMessage::Learn(crate::learn::Message::Arm((module, index)))),
                ..Item::new(ids::part(ui, "learn"), Kind::Button)
            });
            mapping(module, index, p, ui, &mut parts);
        }
        ParamMode::Edit {
            position,
            step,
            fine: _,
            control,
            assignments,
            adding,
        } => {
            parts.push(value_control(module, label, p, *position, *step, control));
            if let Some(field_row) = &p.field {
                parts.push(Item {
                    description: field_row.error.clone(),
                    focus: Some(crate::controls::value_field_id(module, index)),
                    ..field(
                        ids::part(ui, "field"),
                        &format!("{label} value"),
                        &field_row.text,
                        SetText::Value { module, index },
                    )
                });
            } else {
                parts.push(Item::text(ids::part(ui, "shown"), p.value.clone()));
            }
            if let Some(range) = &p.range {
                let set = |at: usize| {
                    (at < range.choices.len()).then_some(AppMessage::Inspector(Message::Range {
                        module,
                        index,
                        range: at,
                    }))
                };
                parts.push(Item {
                    label: Some("Range".to_string()),
                    value: Some(
                        range
                            .selected
                            .and_then(|at| range.choices.get(at).cloned())
                            .unwrap_or_else(|| "Full".to_string()),
                    ),
                    description: Some("What a full turn of the control covers.".to_string()),
                    step: match range.selected {
                        Some(at) => (set(at + 1), at.checked_sub(1).and_then(set)),
                        None => (None, None),
                    },
                    ..Item::new(ids::part(ui, "range"), Kind::Choice)
                });
            }
            mapping(module, index, p, ui, &mut parts);
            modulation(module, index, assignments, adding.as_deref(), ui, &mut parts);
        }
    }
    Item {
        description,
        ..Item::group(ui.clone(), label, parts)
    }
}

/// A row's value control: a slider in the parameter's own range, a set of
/// options, or a list.
fn value_control(module: u32, label: &str, p: &ParamRow, position: f32, step: f32, control: &ParamControl) -> Item {
    let index = p.index;
    let id = ids::part(&p.ui, "value");
    let choose = |choice: usize| AppMessage::Inspector(Message::Choice { module, index, choice });
    match control {
        ParamControl::Slider => {
            let spec = p.spec;
            let to = |position: f32| {
                AppMessage::Inspector(Message::Param {
                    module,
                    index,
                    position,
                })
            };
            // One of its own arrow-key steps (section 7, rule 5), as the
            // toolkit slider takes it: to the next step on its grid.
            let steps = (position / step).round();
            let up = (step * (steps + 1.0)).min(1.0);
            let down = (step * (steps - 1.0)).max(0.0);
            Item {
                label: Some(label.to_string()),
                // The value with its unit, as the row shows it.
                description: Some(p.value.clone()),
                number: Some(Number {
                    value: f64::from(spec.denormalize(position)),
                    min: f64::from(spec.min),
                    max: f64::from(spec.max),
                    step: matches!(spec.options.mapping, ParamMapping::Linear)
                        .then(|| f64::from(step * (spec.max - spec.min))),
                }),
                step: ((up != position).then(|| to(up)), (down != position).then(|| to(down))),
                set_number: Some(SetNumber::Param { module, index, spec }),
                groups: true,
                ..Item::new(id, Kind::Slider)
            }
        }
        ParamControl::Radio { choices, selected } => {
            let options = choices
                .iter()
                .enumerate()
                .map(|(at, name)| Item {
                    label: Some((*name).to_string()),
                    on: Some(at == *selected),
                    click: (at != *selected).then(|| choose(at)),
                    ..Item::new(id.clone().with("option").with(at as u32), Kind::Option)
                })
                .collect();
            Item {
                label: Some(label.to_string()),
                children: options,
                ..Item::new(id, Kind::Options)
            }
        }
        ParamControl::PickList { choices, selected } => Item {
            label: Some(label.to_string()),
            value: choices.get(*selected).map(|name| (*name).to_string()),
            step: (
                (*selected + 1 < choices.len()).then(|| choose(*selected + 1)),
                selected.checked_sub(1).map(choose),
            ),
            ..Item::new(id, Kind::Choice)
        },
    }
}

/// The Expose or Unexpose button on a row, for the surface it acts on.
fn expose(exposure: &ExposureRow, row: &UiId) -> Item {
    let id = ids::part(row, "expose").with(exposure.surface.as_str());
    let (module, index) = (exposure.module, exposure.index);
    match &exposure.name {
        Some(name) => Item::button(
            id,
            format!("Unexpose on {}, exposed as {name}", exposure.surface),
            Some(AppMessage::Inspector(Message::Unexpose { module, index })),
        ),
        None => Item::button(
            id,
            format!("Expose on {}", exposure.surface),
            Some(AppMessage::Inspector(Message::Expose { module, index })),
        ),
    }
}

/// A row's MIDI mapping: the control it is mapped to, its Options and ×
/// buttons, and the options rows while they are open.
fn mapping(module: u32, index: usize, p: &ParamRow, ui: &UiId, parts: &mut Vec<Item>) {
    use crate::learn::Message as Learn;
    if let Some(control) = &p.mapping
        && matches!(p.mode, ParamMode::Edit { .. })
    {
        parts.push(Item::text(ids::part(ui, "midi"), format!("MIDI: {control}")));
    }
    if p.mapping.is_some() {
        parts.push(Item::button(
            ids::part(ui, "midi/options"),
            "MIDI mapping options",
            Some(AppMessage::Learn(Learn::MidiOptionsRows { module, index })),
        ));
        parts.push(Item::button(
            ids::part(ui, "midi/remove"),
            "Remove MIDI mapping",
            Some(AppMessage::Learn(Learn::Unmap { module, index })),
        ));
    }
    let Some(row) = p.mapping_options.as_ref().filter(|row| row.open) else {
        return;
    };
    parts.extend(mapping_options(module, index, row, ui));
}

/// A mapping's options rows: mode, then range and invert, or the encoding
/// for Relative (midi-learn.md, Mapping modes).
fn mapping_options(module: u32, index: usize, row: &MappingRow, ui: &UiId) -> Vec<Item> {
    use crate::learn::Message as Learn;
    let options = row.options;
    let set = |options: MidiMappingOptions| AppMessage::Learn(Learn::MidiOptions { module, index, options });
    let modes = [
        MidiMode::Absolute,
        MidiMode::Pickup,
        MidiMode::Relative(encoding_of(options.mode).unwrap_or_default()),
    ];
    let at = modes
        .iter()
        .position(|mode| std::mem::discriminant(mode) == std::mem::discriminant(&options.mode))
        .unwrap_or(0);
    let pick_mode = |at: Option<usize>| {
        at.and_then(|at| modes.get(at))
            .map(|mode| set(options.with_mode(*mode)))
    };
    let mut parts = vec![Item {
        label: Some("MIDI mode".to_string()),
        value: Some(mode_name(options.mode).to_string()),
        step: (pick_mode(Some(at + 1)), pick_mode(at.checked_sub(1))),
        ..Item::new(ids::part(ui, "midi/mode"), Kind::Choice)
    }];
    if let Some(encoding) = encoding_of(options.mode) {
        let encodings = [
            MidiEncoding::Offset64,
            MidiEncoding::TwosComplement,
            MidiEncoding::SignBit,
        ];
        let at = encodings.iter().position(|e| *e == encoding).unwrap_or(0);
        let pick = |at: Option<usize>| {
            at.and_then(|at| encodings.get(at))
                .map(|e| set(options.with_mode(MidiMode::Relative(*e))))
        };
        parts.push(Item {
            label: Some("Encoding".to_string()),
            value: Some(encoding_name(encoding).to_string()),
            step: (pick(Some(at + 1)), pick(at.checked_sub(1))),
            ..Item::new(ids::part(ui, "midi/encoding"), Kind::Choice)
        });
    } else {
        parts.push(Item {
            description: row.range_error.clone(),
            ..field(
                ids::part(ui, "midi/low"),
                "MIDI range low",
                &row.low,
                SetText::MidiLow {
                    module,
                    index,
                    high: row.high.clone(),
                },
            )
        });
        parts.push(Item {
            description: row.range_error.clone(),
            ..field(
                ids::part(ui, "midi/high"),
                "MIDI range high",
                &row.high,
                SetText::MidiHigh {
                    module,
                    index,
                    low: row.low.clone(),
                },
            )
        });
        if let Some(reason) = &row.range_error {
            parts.push(Item::text(ids::part(ui, "midi/range_error"), reason.clone()));
        }
        parts.push(Item {
            label: Some("Invert".to_string()),
            on: Some(options.invert),
            click: Some(set(MidiMappingOptions {
                invert: !options.invert,
                ..options
            })),
            ..Item::new(ids::part(ui, "midi/invert"), Kind::Check)
        });
    }
    if let Some(note) = &row.range_note {
        parts.push(Item::text(ids::part(ui, "midi/range_note"), note.clone()));
    }
    parts
}

/// The modulation assignments on a row, each a card keyed by its
/// assignment, then **Add modulation** and the sources it offers.
fn modulation(
    module: u32,
    index: usize,
    assignments: &[AssignmentRow],
    adding: Option<&[SourceRow]>,
    ui: &UiId,
    parts: &mut Vec<Item>,
) {
    use crate::modulation::Message as Modulation;
    for a in assignments {
        let id = a.id;
        let key = ids::assignment(ui, id.0);
        let at = a
            .polarities
            .iter()
            .position(|choice| *choice == a.polarity)
            .unwrap_or(0);
        let polarity = |at: Option<usize>| {
            at.and_then(|at| a.polarities.get(at))
                .map(|choice| AppMessage::Modulation(Modulation::Polarity { id, choice: *choice }))
        };
        let depth = |depth: f32| AppMessage::Modulation(Modulation::Depth { id, depth });
        parts.push(Item::group(
            key.clone(),
            format!("Modulation from {}", a.source),
            vec![
                Item {
                    label: Some("On".to_string()),
                    on: Some(a.enabled),
                    click: Some(AppMessage::Modulation(Modulation::Enabled {
                        id,
                        enabled: !a.enabled,
                    })),
                    ..Item::new(ids::part(&key, "on"), Kind::Check)
                },
                Item {
                    label: Some("Polarity".to_string()),
                    value: Some(a.polarity.to_string()),
                    step: (polarity(Some(at + 1)), polarity(at.checked_sub(1))),
                    ..Item::new(ids::part(&key, "polarity"), Kind::Choice)
                },
                // In percent, as the card says it, so the number is what
                // is heard.
                Item {
                    label: Some("Depth".to_string()),
                    number: Some(Number {
                        value: f64::from((a.depth * 100.0).round()),
                        min: -100.0,
                        max: 100.0,
                        step: Some(1.0),
                    }),
                    step: (
                        (a.depth < 1.0).then(|| depth((a.depth + 0.01).min(1.0))),
                        (a.depth > -1.0).then(|| depth((a.depth - 0.01).max(-1.0))),
                    ),
                    set_number: Some(SetNumber::Depth(id)),
                    groups: true,
                    ..Item::new(ids::part(&key, "depth"), Kind::Slider)
                },
                Item::button(
                    ids::part(&key, "remove"),
                    "Remove modulation",
                    Some(AppMessage::Modulation(Modulation::Unassign(id))),
                ),
            ],
        ));
    }
    parts.push(Item::button(
        ids::part(ui, "add"),
        if adding.is_some() {
            "Cancel adding modulation"
        } else {
            "Add modulation"
        },
        Some(AppMessage::Modulation(Modulation::AddModulation { module, index })),
    ));
    for source in adding.unwrap_or_default() {
        let key = ids::part(ui, "add/source")
            .with(source.source.module.0)
            .with(source.source.port as u32);
        parts.push(Item::button(
            key,
            format!("Modulate from {}", source.name),
            source.enabled.then_some(AppMessage::Modulation(Modulation::Assign {
                source: source.source,
                module,
                index,
            })),
        ));
    }
}

#[cfg(test)]
mod tests;
