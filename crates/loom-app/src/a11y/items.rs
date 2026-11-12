//! The parts of a panel the toolkit draws, as a screen reader is told them:
//! plain data built from the same model the panel's view draws from
//! (`docs/design/ui-layer.md`, section 7, rule 1).
//!
//! An [`Item`] says what one part is, what it says, and what each action a
//! screen reader can ask for does, as the messages a pointer or the keys
//! would send. The tree turns items into nodes, and the layer turns an
//! action on a node back into those messages, so a panel has no
//! accessibility code of its own beyond building its items. The inspector,
//! the matrix, and the overlays over the canvas are built this way.
//!
//! An item's UI ID is the same one the view names the part's container
//! with, so the window says where the part is (`places.rs`).

use crate::Message;
use loom_core::ParamSpec;
use loom_ui_core::UiId;

/// What a part is, which decides its role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A named group of other parts: a panel, a parameter's row, a card.
    Group,
    /// Text the panel shows. Its words are its value; it has no label of
    /// its own, since an adapter reads a label node's label in place of
    /// its value (section 7, rule 17).
    Text,
    /// A button.
    Button,
    /// A check box, on or off.
    Check,
    /// A slider over a number.
    Slider,
    /// A set of options shown at once, one chosen.
    Options,
    /// One of those options.
    Option,
    /// A list that shows its chosen option and opens to choose another.
    Choice,
    /// A field to type text in.
    Field,
    /// A list of rows, such as the matrix's assignments. Its children are
    /// list items, each told its position and the list's size, so a screen
    /// reader can say "3 of 7".
    List,
    /// One row of a list, holding that row's parts.
    ListItem,
    /// A window over the rest that owns the input: help, Your patches,
    /// Preferences. Its label is its title.
    Dialog,
    /// A dialog that asks before going on: the question. Its description is
    /// the sentence under its title.
    AlertDialog,
    /// A menu over the rest that owns the input: the tile menu, the probe
    /// menu, the Recent list.
    Menu,
    /// One item of a menu. Click picks it.
    MenuItem,
    /// A link in text. Click follows it.
    Link,
    /// A list of options to pick from, one selected, such as the module
    /// browser's results. Its children are list box options, each told its
    /// position and the list's size.
    ListBox,
    /// One option of a list box. `selected` says whether it is the one the
    /// keys are on; Click picks it.
    ListBoxOption,
}

/// A slider's number and range, in the units the user sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Number {
    /// The value now.
    pub value: f64,
    /// The lowest value.
    pub min: f64,
    /// The highest value.
    pub max: f64,
    /// The amount one Increment moves, when it is one amount everywhere.
    pub step: Option<f64>,
}

/// What a screen reader's SetValue with a number sets.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SetNumber {
    /// A module's parameter, in its own range.
    Param {
        /// The module.
        module: u32,
        /// The parameter's index on it.
        index: usize,
        /// The parameter as its control maps it.
        spec: ParamSpec,
    },
    /// A modulation assignment's depth, in percent from -100 to 100, as
    /// the panel says it.
    Depth(loom_core::AssignmentId),
    /// The spread of one exposed item of a stack, in percent from 0 to 100.
    Spread {
        /// The sub-patch tile.
        tile: u32,
        /// The exposed item's name.
        name: String,
    },
}

impl SetNumber {
    /// The messages that set the number to `value`, kept within range. A
    /// value that is not a number sets nothing.
    pub(crate) fn messages(&self, value: f64) -> Vec<Message> {
        if value.is_nan() {
            return Vec::new();
        }
        let value = value as f32;
        vec![match self {
            Self::Param { module, index, spec } => Message::Inspector(crate::inspector::Message::Param {
                module: *module,
                index: *index,
                position: spec.normalize(value),
            }),
            Self::Depth(id) => Message::Modulation(crate::modulation::Message::Depth {
                id: *id,
                depth: (value / 100.0).clamp(-1.0, 1.0),
            }),
            // The slider says the spread in percent, as the panel does.
            Self::Spread { tile, name } => Message::Mode(crate::modes::Message::SetSpread {
                tile: *tile,
                name: name.clone(),
                amount: (value / 100.0).clamp(0.0, 1.0),
            }),
        }]
    }
}

/// What a screen reader's SetValue with text sets: what typing the text
/// and pressing Enter would.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SetText {
    /// A parameter's value field.
    Value {
        /// The module.
        module: u32,
        /// The parameter's index on it.
        index: usize,
    },
    /// The low end of a MIDI mapping's range, beside its high end.
    MidiLow {
        /// The module.
        module: u32,
        /// The parameter's index on it.
        index: usize,
        /// The high end, as the field shows it.
        high: String,
    },
    /// The high end of a MIDI mapping's range, beside its low end.
    MidiHigh {
        /// The module.
        module: u32,
        /// The parameter's index on it.
        index: usize,
        /// The low end, as the field shows it.
        low: String,
    },
    /// A sub-patch's name.
    SubPatchName(u32),
    /// The label of a module this version does not have.
    UnknownLabel(u32),
    /// A new tag for the patch.
    Tag,
    /// The matrix's filter. It filters as it is typed in and has no Enter
    /// of its own, so the text is the one message.
    MatrixFilter,
    /// The module browser's search field. It searches as it is typed in,
    /// and Enter would pick a row, so the text is the one message.
    BrowserQuery,
    /// Your patches' search field, which filters as it is typed in.
    PatchesSearch,
    /// Your patches' Author field, which filters as it is typed in.
    PatchesAuthor,
}

impl SetText {
    /// The messages that type `text` and press Enter.
    pub(crate) fn messages(&self, text: &str) -> Vec<Message> {
        use crate::inspector::Message as Inspector;
        use crate::learn::Message as Learn;
        let text = text.to_string();
        match self {
            Self::Value { module, index } => vec![
                Message::Inspector(Inspector::ValueText {
                    module: *module,
                    index: *index,
                    text,
                }),
                Message::Inspector(Inspector::ValueSubmit {
                    module: *module,
                    index: *index,
                }),
            ],
            Self::MidiLow { module, index, high } => vec![
                Message::Learn(Learn::MidiRangeText {
                    module: *module,
                    index: *index,
                    low: text,
                    high: high.clone(),
                }),
                Message::Learn(Learn::MidiRangeSubmit {
                    module: *module,
                    index: *index,
                }),
            ],
            Self::MidiHigh { module, index, low } => vec![
                Message::Learn(Learn::MidiRangeText {
                    module: *module,
                    index: *index,
                    low: low.clone(),
                    high: text,
                }),
                Message::Learn(Learn::MidiRangeSubmit {
                    module: *module,
                    index: *index,
                }),
            ],
            Self::SubPatchName(module) => vec![
                Message::Inspector(Inspector::SubPatchNameDraft {
                    module: *module,
                    value: text,
                }),
                Message::Inspector(Inspector::SubPatchNameApply { module: *module }),
            ],
            Self::UnknownLabel(module) => vec![
                Message::Inspector(Inspector::UnknownLabelDraft {
                    module: *module,
                    value: text,
                }),
                Message::Inspector(Inspector::UnknownLabelApply { module: *module }),
            ],
            Self::Tag => vec![
                Message::PatchInfo(crate::patch_info::Message::TagDraft(text)),
                Message::PatchInfo(crate::patch_info::Message::TagSubmit),
            ],
            Self::MatrixFilter => vec![Message::Matrix(crate::matrix::Event::Filter(text))],
            Self::BrowserQuery => vec![Message::Browser(crate::browser::Message::Query(text))],
            Self::PatchesSearch => vec![Message::Patches(crate::patches::Message::Search(text))],
            Self::PatchesAuthor => vec![Message::Patches(crate::patches::Message::Author(text))],
        }
    }
}

/// One part of a panel, and what a screen reader can do with it.
#[derive(Debug, Clone)]
pub(crate) struct Item {
    /// Its UI ID: the same one its container in the window has.
    pub id: UiId,
    /// What it is.
    pub kind: Kind,
    /// Its name. A text part has none: its words are its value.
    pub label: Option<String>,
    /// What it says: a text's words, a field's text, a choice's option.
    pub value: Option<String>,
    /// More about it, as its hover text gives it. Read late, if at all, on
    /// some platforms (section 7, rule 23), so nothing that must be heard
    /// is only here.
    pub description: Option<String>,
    /// A slider's number.
    pub number: Option<Number>,
    /// A check box's or an option's state.
    pub on: Option<bool>,
    /// A list box option's state: true for the one the keys are on.
    pub selected: Option<bool>,
    /// For a field whose keys move in a list: the option they are on, as its
    /// active descendant. AccessKit's macOS adapter (0.27.2) does not read
    /// it; other platforms do.
    pub active: Option<UiId>,
    /// What ShowContextMenu sends: the part's menu, opened as a key would.
    pub menu: Option<Message>,
    /// False while it can do nothing; its description then says why.
    pub enabled: bool,
    /// What a Click sends.
    pub click: Option<Message>,
    /// What Increment sends, and what Decrement sends. `None` where the
    /// part is already at that end.
    pub step: (Option<Message>, Option<Message>),
    /// What a SetValue with a number sets.
    pub set_number: Option<SetNumber>,
    /// What a SetValue with text sets.
    pub set_text: Option<SetText>,
    /// For a field, the widget that takes keyboard focus. Only a field can
    /// take it outside the canvas until phase 3 (section 7, rule 22).
    pub focus: Option<iced::advanced::widget::Id>,
    /// True when its SetValue, Increment, and Decrement join the screen
    /// reader's undo group, as a slider's do (section 7, rule 6).
    pub groups: bool,
    /// What it holds.
    pub children: Vec<Item>,
}

impl Item {
    /// A part of kind `kind` with nothing set yet.
    pub(crate) fn new(id: UiId, kind: Kind) -> Self {
        Self {
            id,
            kind,
            label: None,
            value: None,
            description: None,
            number: None,
            on: None,
            selected: None,
            active: None,
            menu: None,
            enabled: true,
            click: None,
            step: (None, None),
            set_number: None,
            set_text: None,
            focus: None,
            groups: false,
            children: Vec::new(),
        }
    }

    /// Text the panel shows.
    pub(crate) fn text(id: UiId, words: impl Into<String>) -> Self {
        Self {
            value: Some(words.into()),
            ..Self::new(id, Kind::Text)
        }
    }

    /// A named group holding `children`.
    pub(crate) fn group(id: UiId, label: impl Into<String>, children: Vec<Item>) -> Self {
        Self {
            label: Some(label.into()),
            children,
            ..Self::new(id, Kind::Group)
        }
    }

    /// A button that sends `click` when it is enabled.
    pub(crate) fn button(id: UiId, label: impl Into<String>, click: Option<Message>) -> Self {
        Self {
            label: Some(label.into()),
            enabled: click.is_some(),
            click,
            ..Self::new(id, Kind::Button)
        }
    }

    /// Every part in this one and below it, this one first.
    pub(crate) fn walk(&self) -> Vec<&Item> {
        let mut all = vec![self];
        for child in &self.children {
            all.extend(child.walk());
        }
        all
    }
}

/// The item among `items`, at any depth, whose UI ID is `id`.
pub(crate) fn find<'a>(items: &'a [Item], id: &UiId) -> Option<&'a Item> {
    items.iter().flat_map(Item::walk).find(|item| item.id == *id)
}

/// The messages for a screen reader's `action` on `item`, with its data,
/// or none when the item does not do that, or not now.
pub(crate) fn messages(item: &Item, action: accesskit::Action, data: Option<&accesskit::ActionData>) -> Vec<Message> {
    use accesskit::{Action, ActionData};
    if !item.enabled {
        return Vec::new();
    }
    match (action, data) {
        (Action::Click, _) => item.click.clone().into_iter().collect(),
        (Action::ShowContextMenu, _) => item.menu.clone().into_iter().collect(),
        (Action::Increment, _) => item.step.0.clone().into_iter().collect(),
        (Action::Decrement, _) => item.step.1.clone().into_iter().collect(),
        (Action::SetValue, Some(ActionData::NumericValue(value))) => item
            .set_number
            .as_ref()
            .map(|set| set.messages(*value))
            .unwrap_or_default(),
        (Action::SetValue, Some(ActionData::Value(text))) => {
            item.set_text.as_ref().map(|set| set.messages(text)).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;

/// Makes the UI IDs of a list's rows unique as they are built. A row's ID
/// comes from what it shows, such as a file's path, and two rows could in
/// rare cases show the same text: two file names that differ only in bytes
/// that are not UTF-8 read back as the same lossy text. A second row with an
/// ID already taken gets ` 2` added to its last key, a third ` 3`, in list
/// order, so no two nodes share an ID.
#[derive(Debug, Default)]
pub(crate) struct UniqueIds {
    /// The IDs given so far.
    taken: std::collections::HashSet<UiId>,
}

impl UniqueIds {
    /// `id`, or `id` with a number added to its last key when it is taken.
    pub(crate) fn take(&mut self, id: UiId) -> UiId {
        if self.taken.insert(id.clone()) {
            return id;
        }
        let keys = id.keys();
        let (last, outer) = keys.split_last().expect("an ID has a key");
        let base = match last {
            loom_ui_core::IdKey::Name(name) => name.to_string(),
            loom_ui_core::IdKey::Number(number) => number.to_string(),
        };
        (2..)
            .map(|n| {
                let key = format!("{base} {n}");
                match outer.split_first() {
                    Some((first, rest)) => rest
                        .iter()
                        .cloned()
                        .fold(UiId::panel(first.clone()), UiId::with)
                        .with(key.as_str()),
                    None => UiId::panel(key.as_str()),
                }
            })
            .find(|candidate| self.taken.insert(candidate.clone()))
            .expect("a free number")
    }
}
