//! What the runtime's inspector rows send (plan 3.2, row 4d.2): a click, a
//! choice in a select, a slider's move, a field's edit, and a hover, each
//! found by its part's UI ID in the same model the rows are drawn from.
//!
//! A click is the message the part's accessibility item sends for Click
//! (`inspector/access.rs`), so a pointer, a key, and a screen reader send
//! the same one, and the two cannot drift apart. A choice, a move, and an
//! edit carry a value the item list has no message for, so they are
//! mapped here.

use super::super::{Inspector, ParamMode, ParamRow, access, ids};
use crate::App;
use crate::a11y::items;
use crate::learn::Message as Learn;
use crate::modulation::Message as Modulation;
use loom_core::MidiMode;
use loom_ui::{FieldEvent, SlidePhase};
use loom_ui_core::UiId;

/// A parameter's row as the routes read it: the module whose parameter it
/// edits, and the row.
type Row<'a> = (u32, &'a ParamRow);

/// Every parameter row the inspector shows for `model`, with the module
/// each edits.
fn rows(model: &Inspector) -> Vec<Row<'_>> {
    match model {
        Inspector::Module(m) => m.params.iter().map(|p| (m.id, p)).collect(),
        Inspector::Slot(s) => s
            .role_params
            .iter()
            .chain(&s.other_params)
            .map(|(_, module, p)| (*module, p))
            .collect(),
        _ => Vec::new(),
    }
}

impl App {
    /// The message for a click on a part of a module's, an unknown
    /// module's, or a slot's rows: the message its accessibility item sends
    /// for Click. `None` for a part that is turned off or sends nothing.
    pub(crate) fn inspector_rows_click(&self, id: &UiId) -> Option<crate::Message> {
        let model = self.inspector_model();
        if !matches!(model, Inspector::Module(_) | Inspector::Unknown(_) | Inspector::Slot(_)) {
            return None;
        }
        let parts = access::items(&model);
        let item = items::find(&parts, id)?;
        if !item.enabled {
            return None;
        }
        item.click.clone()
    }

    /// The message for option `index` chosen in a select of the rows: a
    /// parameter's choice, its Range (Full first), a mapping's mode or
    /// encoding, a modulation's polarity, or a slot's part.
    pub(crate) fn inspector_chose(&self, id: &UiId, index: usize) -> Option<crate::Message> {
        use crate::Message;
        if !self.inspector_owns() {
            return None;
        }
        let model = self.inspector_model();
        if let Inspector::Slot(s) = &model
            && *id == ids::part(&ids::panel(s.id), "part")
        {
            let choice = s.choices.get(index)?;
            return Some(Message::SwapSlot {
                slot: s.id,
                to: choice.to.clone(),
            });
        }
        for (module, p) in rows(&model) {
            let at = |name: &'static str| ids::part(&p.ui, name);
            let index_of = p.index;
            if *id == at("value") {
                return Some(Message::Inspector(super::super::Message::Choice {
                    module,
                    index: index_of,
                    choice: index,
                }));
            }
            if *id == at("range") {
                // Full is first, then each range (Brylie, 2026-11-25).
                return Some(Message::Inspector(match index.checked_sub(1) {
                    Some(range) => super::super::Message::Range {
                        module,
                        index: index_of,
                        range,
                    },
                    None => super::super::Message::FullRange {
                        module,
                        index: index_of,
                    },
                }));
            }
            if let Some(row) = &p.mapping_options {
                let options = row.options;
                let set = |options| {
                    Message::Learn(Learn::MidiOptions {
                        module,
                        index: index_of,
                        options,
                    })
                };
                if *id == at("midi/settings/mode") {
                    let encoding = super::super::encoding_of(options.mode).unwrap_or_default();
                    let modes = [MidiMode::Absolute, MidiMode::Pickup, MidiMode::Relative(encoding)];
                    let mode = *modes.get(index)?;
                    return Some(set(options.with_mode(mode)));
                }
                if *id == at("midi/settings/encoding") {
                    let encoding = super::rows::ENCODINGS.get(index)?;
                    return Some(set(options.with_mode(MidiMode::Relative(*encoding))));
                }
            }
            if let ParamMode::Edit { assignments, .. } = &p.mode {
                for a in assignments {
                    if *id == ids::part(&ids::assignment(&p.ui, a.id.0), "polarity") {
                        let choice = *a.polarities.get(index)?;
                        return Some(Message::Modulation(Modulation::Polarity { id: a.id, choice }));
                    }
                }
            }
        }
        None
    }

    /// The message for a slider of the rows moving to `position`, in
    /// `phase` of its gesture: a parameter's value, or a modulation's
    /// depth. The end of a gesture closes its undo step, as Iced's slider's
    /// release did.
    pub(crate) fn inspector_slid(&self, id: &UiId, position: f32, phase: SlidePhase) -> Option<crate::Message> {
        use crate::Message;
        if !self.inspector_owns() {
            return None;
        }
        let end = phase == SlidePhase::End;
        let model = self.inspector_model();
        for (module, p) in rows(&model) {
            if *id == ids::part(&p.ui, "value") {
                return Some(Message::Inspector(super::super::Message::Slid {
                    module,
                    index: p.index,
                    position,
                    end,
                }));
            }
            if let ParamMode::Edit { assignments, .. } = &p.mode {
                for a in assignments {
                    if *id == ids::part(&ids::assignment(&p.ui, a.id.0), "depth") {
                        return Some(Message::Inspector(super::super::Message::DepthSlid {
                            id: a.id,
                            depth: position.mul_add(2.0, -1.0),
                            end,
                        }));
                    }
                }
            }
        }
        None
    }

    /// The message for an event on a field of the rows: a parameter's
    /// typed value, a mapping's range ends, or an unknown module's label.
    /// Each edit is the text as typed, and Enter submits it.
    pub(crate) fn inspector_field(&self, id: &UiId, event: &FieldEvent) -> Option<crate::Message> {
        use super::super::Message as Inspect;
        use crate::Message;
        let model = self.inspector_model();
        if let Inspector::Unknown(m) = &model
            && *id == ids::part(&ids::panel(m.id), "label")
        {
            return match event {
                FieldEvent::Edited(value) => Some(Message::Inspector(Inspect::UnknownLabelDraft {
                    module: m.id,
                    value: value.clone(),
                })),
                FieldEvent::Committed(_) => Some(Message::Inspector(Inspect::UnknownLabelApply { module: m.id })),
                _ => None,
            };
        }
        for (module, p) in rows(&model) {
            let index = p.index;
            if *id == ids::part(&p.ui, "field") {
                return match event {
                    FieldEvent::Edited(text) => Some(Message::Inspector(Inspect::ValueText {
                        module,
                        index,
                        text: text.clone(),
                    })),
                    FieldEvent::Committed(_) => Some(Message::Inspector(Inspect::ValueSubmit { module, index })),
                    _ => None,
                };
            }
            let Some(row) = &p.mapping_options else { continue };
            let (low, high) = (
                ids::part(&p.ui, "midi/settings/low"),
                ids::part(&p.ui, "midi/settings/high"),
            );
            if *id != low && *id != high {
                continue;
            }
            return match event {
                FieldEvent::Edited(text) => {
                    let (low, high) = if *id == low {
                        (text.clone(), row.high.clone())
                    } else {
                        (row.low.clone(), text.clone())
                    };
                    Some(Message::Learn(Learn::MidiRangeText {
                        module,
                        index,
                        low,
                        high,
                    }))
                }
                FieldEvent::Committed(_) => Some(Message::Learn(Learn::MidiRangeSubmit { module, index })),
                _ => None,
            };
        }
        None
    }

    /// Whether a screen reader's SetValue on the field `id` is the app's to
    /// carry out: a field of a module's, an unknown module's, or a slot's
    /// rows, whose item commits the value it is given, as Iced's did (a
    /// typed value, a mapping's range ends, a label). The runtime would
    /// only make a draft of it. Patch info's fields are the runtime's.
    pub(crate) fn inspector_commits_set_value(&self, id: &UiId) -> bool {
        if !self.inspector_owns() || !id.is_under(&super::inspector_id()) {
            return false;
        }
        let model = self.inspector_model();
        if !matches!(model, Inspector::Module(_) | Inspector::Unknown(_) | Inspector::Slot(_)) {
            return false;
        }
        items::find(&access::items(&model), id).is_some_and(|item| item.set_text.is_some())
    }

    /// The message for the pointer coming onto a row that takes a dragged
    /// output, or leaving it (`None`): the row lights while the drag is
    /// over it (modulation.md).
    pub(crate) fn inspector_hovered(&self, id: &UiId, on: bool) -> Option<crate::Message> {
        let model = self.inspector_model();
        let (module, p) = rows(&model).into_iter().find(|(_, p)| p.ui == *id && p.accepts_drop)?;
        let row = on.then_some((module, p.index));
        Some(crate::Message::Inspector(super::super::Message::DropRow(row)))
    }
}
