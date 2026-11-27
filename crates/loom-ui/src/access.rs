//! What an element is to a screen reader (ui-layer.md, section 7).
//!
//! An element that a screen reader should meet carries an [`Access`]: its
//! role, its name, its value, and what a screen reader can do with it. It
//! is the element's own data, so the node a screen reader reads and the box
//! the layout places come from one element (section 7, rule 1).
//!
//! [`Access`] is our own type, not AccessKit's node: rule 2 (a role that
//! can be operated, a non-empty name, a value) is checked against it, and
//! AccessKit's breaking releases reach only the one method that makes its
//! node. The Focus action is never set here: every focus stop offers it,
//! and nothing else does (section 7, rule 22).

use accesskit::{Action as KitAction, Node, Role as KitRole, Toggled};
use loom_ui_core::UiId;
use std::fmt;

/// What an element is, as the table in section 7 names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// A button.
    Button,
    /// A switch that is on or off.
    Switch,
    /// A check box.
    CheckBox,
    /// A slider: a knob, a fader, or a number.
    Slider,
    /// A group of radio buttons.
    RadioGroup,
    /// One radio button.
    RadioButton,
    /// A select, which shows its chosen option.
    ComboBox,
    /// A text field.
    TextInput,
    /// Text that shows something, with no label of its own (rule 17).
    Label,
    /// A named group of parts, such as a panel.
    Group,
    /// A dialog.
    Dialog,
    /// A dialog that holds the window until it is answered.
    AlertDialog,
    /// A menu.
    Menu,
    /// One item of a menu.
    MenuItem,
    /// A list.
    List,
    /// One item of a list.
    ListItem,
    /// A list that one row of is picked from.
    ListBox,
    /// One row of a list box.
    ListBoxOption,
    /// A row of buttons that is one Tab stop, with Left and Right moving
    /// among them (the WAI-ARIA toolbar pattern), such as Your patches'
    /// tags. It lies across the window, so its node says it is horizontal.
    Toolbar,
    /// A row of tabs.
    TabList,
    /// One tab.
    Tab,
    /// A message meant to be announced.
    Status,
    /// A drawing, named, with its state in words in the description.
    Image,
    /// A meter.
    Meter,
    /// A link.
    Link,
    /// A tree the app drives (rule 12).
    Tree,
    /// One row of a tree.
    TreeItem,
    /// A scroll container (rule 28): its offset and its scroll actions come
    /// from the container, not from its [`Access`].
    ScrollView,
}

impl Role {
    /// True for a role a user acts on, which must offer an action (rule 2).
    pub fn is_operable(self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::Switch
                | Self::CheckBox
                | Self::Slider
                | Self::RadioButton
                | Self::ComboBox
                | Self::TextInput
                | Self::MenuItem
                | Self::ListBoxOption
                | Self::Tab
                | Self::Link
                | Self::TreeItem
        )
    }

    /// AccessKit's role for it.
    fn kit(self) -> KitRole {
        match self {
            Self::Button => KitRole::Button,
            Self::Switch => KitRole::Switch,
            Self::CheckBox => KitRole::CheckBox,
            Self::Slider => KitRole::Slider,
            Self::RadioGroup => KitRole::RadioGroup,
            Self::RadioButton => KitRole::RadioButton,
            Self::ComboBox => KitRole::ComboBox,
            Self::TextInput => KitRole::TextInput,
            Self::Label => KitRole::Label,
            Self::Group => KitRole::Group,
            Self::Dialog => KitRole::Dialog,
            Self::AlertDialog => KitRole::AlertDialog,
            Self::Menu => KitRole::Menu,
            Self::MenuItem => KitRole::MenuItem,
            Self::List => KitRole::List,
            Self::ListItem => KitRole::ListItem,
            Self::ListBox => KitRole::ListBox,
            Self::ListBoxOption => KitRole::ListBoxOption,
            Self::Toolbar => KitRole::Toolbar,
            Self::TabList => KitRole::TabList,
            Self::Tab => KitRole::Tab,
            Self::Status => KitRole::Status,
            Self::Image => KitRole::Image,
            Self::Meter => KitRole::Meter,
            Self::Link => KitRole::Link,
            Self::Tree => KitRole::Tree,
            Self::TreeItem => KitRole::TreeItem,
            Self::ScrollView => KitRole::ScrollView,
        }
    }
}

/// An element's value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A number in its range, as a slider or a meter has (rules 4, 14, and
    /// 21). The text with its unit goes in the description.
    Number {
        /// The number.
        value: f64,
        /// The lowest it can be.
        min: f64,
        /// The highest it can be.
        max: f64,
        /// One step, only where equal steps of the control are equal steps
        /// of the value (rule 14).
        step: Option<f64>,
    },
    /// Text, in the words the screen shows.
    Text(String),
    /// On or off, or checked or not.
    On(bool),
}

/// What a screen reader can ask an element to do, besides focusing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Press it.
    Click,
    /// Move it one step up.
    Increment,
    /// Move it one step down.
    Decrement,
    /// Set its value.
    SetValue,
    /// Open its menu.
    ShowContextMenu,
    /// Open its list, as a closed select's.
    Expand,
    /// Close its list, as an open select's.
    Collapse,
}

impl Action {
    /// Our action for AccessKit's `action`, or `None` for one that is not
    /// an element's own to carry out, as Focus, a scroll, or a text edit.
    pub(crate) fn from_kit(action: KitAction) -> Option<Self> {
        Some(match action {
            KitAction::Click => Self::Click,
            KitAction::Increment => Self::Increment,
            KitAction::Decrement => Self::Decrement,
            KitAction::SetValue => Self::SetValue,
            KitAction::ShowContextMenu => Self::ShowContextMenu,
            KitAction::Expand => Self::Expand,
            KitAction::Collapse => Self::Collapse,
            _ => return None,
        })
    }

    /// AccessKit's action for it.
    fn kit(self) -> KitAction {
        match self {
            Self::Click => KitAction::Click,
            Self::Increment => KitAction::Increment,
            Self::Decrement => KitAction::Decrement,
            Self::SetValue => KitAction::SetValue,
            Self::ShowContextMenu => KitAction::ShowContextMenu,
            Self::Expand => KitAction::Expand,
            Self::Collapse => KitAction::Collapse,
        }
    }
}

/// A state an element can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct States {
    /// It is turned off and does nothing.
    pub disabled: bool,
    /// It is the chosen one of a set: an open tab, a highlighted row.
    pub selected: bool,
    /// It is open, where it can be opened. `None` where it cannot.
    pub expanded: Option<bool>,
    /// It holds the window: nothing outside it can be reached.
    pub modal: bool,
    /// It is the current item of a set that shows where the user is, as
    /// the open level's entry in a breadcrumb: reported as the current
    /// page (`aria_current`), as the WAI-ARIA breadcrumb pattern does.
    pub current: bool,
}

/// What an element is to a screen reader.
#[derive(Debug, Clone, PartialEq)]
pub struct Access {
    role: Role,
    name: String,
    description: Option<String>,
    value: Option<Value>,
    states: States,
    actions: Vec<Action>,
    /// The element the keys act on while this one keeps the focus, as a
    /// search field's highlighted row (rule 27).
    active: Option<UiId>,
    /// The element it controls, as an open select its list (rule 13).
    controls: Option<UiId>,
    /// Where it is in its set, from 0, as a list box option.
    position: Option<usize>,
    /// How many items it holds, as a list box.
    size: Option<usize>,
}

/// A way an [`Access`] breaks section 7, rule 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// It has no key, so it has no UI ID, which focus and a node need.
    NoKey,
    /// It is a focus stop with no [`Access`]: a user reaches it with Tab,
    /// and a screen reader would have no node to put the focus on.
    StopWithoutAccess,
    /// It is a slot with an [`Access`]. A slot holds another panel, whose
    /// own elements say what they are; give the panel's root the
    /// [`Access`] instead.
    SlotWithAccess,
    /// It has no name, where its role needs one.
    NoName,
    /// It has no value, or the wrong kind, where its role needs one: a
    /// number for a slider or a meter, on or off for a switch or a check
    /// box, and text for a label or a status.
    NoValue,
    /// It is something a user acts on, and offers no action.
    NotOperable,
    /// It is a scroll container that takes its keys from a stop
    /// ([`Element::keys_from`](crate::Element::keys_from)) that is no focus
    /// stop of its panel (ui-layer.md, section 5, rule 12).
    KeysFromNoStop,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoKey => "it has no key, so it has no UI ID",
            Self::StopWithoutAccess => "it is a focus stop with no Access, so it has no node",
            Self::SlotWithAccess => "it is a slot with an Access; give the slot's panel the Access",
            Self::NoName => "it has no name",
            Self::NoValue => "it has no value of the kind its role needs",
            Self::NotOperable => "a user acts on it, and it offers no action",
            Self::KeysFromNoStop => "it takes its keys from an element that is no focus stop of its panel",
        })
    }
}

impl Access {
    /// An element with `role`, named `name` in the words the screen shows.
    pub fn new(role: Role, name: impl Into<String>) -> Self {
        Self {
            role,
            name: name.into(),
            description: None,
            value: None,
            states: States::default(),
            actions: Vec::new(),
            active: None,
            controls: None,
            position: None,
            size: None,
        }
    }

    /// A label that reads `text`: its name and the text shown, as
    /// "Cents: 12.5 cents" (rule 17). A label has no name of its own.
    pub fn label(text: impl Into<String>) -> Self {
        Self::new(Role::Label, "").value(Value::Text(text.into()))
    }

    /// Its description: the hover text, or a slider's value with its unit.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Its value.
    pub fn value(mut self, value: Value) -> Self {
        self.value = Some(value);
        self
    }

    /// Its states.
    pub fn states(mut self, states: States) -> Self {
        self.states = states;
        self
    }

    /// Offers `action`. Focus is not one: a focus stop offers it by itself.
    pub fn action(mut self, action: Action) -> Self {
        if !self.actions.contains(&action) {
            self.actions.push(action);
        }
        self
    }

    /// Names the element the keys act on while this one keeps the focus,
    /// by its UI ID: a search field's highlighted row, which WAI-ARIA calls
    /// its active descendant. The runtime gives the node the row's node ID
    /// when the tree is sent, and leaves it out while no laid-out element
    /// has that UI ID, so the tree never names a node it does not hold.
    #[must_use]
    pub fn active_descendant(mut self, id: UiId) -> Self {
        self.active = Some(id);
        self
    }

    /// Names the element it controls, by its UI ID, as a button that shows
    /// a part of the canvas names that part (section 7, rule 13). The
    /// runtime gives the node the other's node ID when the tree is sent,
    /// and leaves it out while the tree holds no node for that UI ID.
    #[must_use]
    pub fn controlling(mut self, id: UiId) -> Self {
        self.controls = Some(id);
        self
    }

    /// Says it is item `position` of its set, counted from 0, as a list
    /// box's option is. The set's size goes on the container
    /// ([`size_of_set`](Self::size_of_set)), as AccessKit has it.
    #[must_use]
    pub fn position_in_set(mut self, position: usize) -> Self {
        self.position = Some(position);
        self
    }

    /// Says it holds `size` items, as a list box says how many rows it
    /// shows.
    #[must_use]
    pub fn size_of_set(mut self, size: usize) -> Self {
        self.size = Some(size);
        self
    }

    /// The same node shown as an open select (section 7, rule 32): it is
    /// expanded, offers Collapse in place of Expand, names the highlighted
    /// option `row` as its active descendant, and controls the list `list`.
    pub(crate) fn opened(mut self, row: UiId, list: UiId) -> Self {
        self.states.expanded = Some(true);
        self.actions.retain(|action| *action != Action::Expand);
        if !self.actions.contains(&Action::Collapse) {
            self.actions.push(Action::Collapse);
        }
        self.active = Some(row);
        self.controls = Some(list);
        self
    }

    /// The element it controls, if any.
    pub(crate) fn controls(&self) -> Option<&UiId> {
        self.controls.as_ref()
    }

    /// The element it names as its active descendant, if any.
    pub(crate) fn active(&self) -> Option<&UiId> {
        self.active.as_ref()
    }

    /// True when it has a description of its own.
    pub(crate) fn has_description(&self) -> bool {
        self.description.is_some()
    }

    /// True when `text` only repeats its name or its text value, so as a
    /// description it would be read twice.
    pub(crate) fn repeats(&self, text: &str) -> bool {
        self.name == text || matches!(&self.value, Some(Value::Text(value)) if value == text)
    }

    /// True when it offers `action`.
    pub(crate) fn offers(&self, action: Action) -> bool {
        self.actions.contains(&action)
    }

    /// Its role.
    pub fn role(&self) -> Role {
        self.role
    }

    /// Its name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// True when its value is on: a chosen radio, a checked box.
    pub(crate) fn is_on(&self) -> bool {
        matches!(self.value, Some(Value::On(true)))
    }

    /// True when it is turned off. A turned-off element is no focus stop.
    pub fn is_disabled(&self) -> bool {
        self.states.disabled
    }

    /// What breaks section 7, rule 2, if anything. `focusable` says
    /// whether the element is a focus stop, which offers Focus. A turned-off
    /// element is exempt from the action check, since it offers nothing
    /// until it is turned on (rule 22).
    pub fn problem(&self, focusable: bool) -> Option<Problem> {
        let value_fits = match (self.role, &self.value) {
            (Role::Slider | Role::Meter, Some(Value::Number { .. })) => true,
            (Role::Slider | Role::Meter, _) => false,
            (Role::Switch | Role::CheckBox, Some(Value::On(_))) => true,
            (Role::Switch | Role::CheckBox, _) => false,
            (Role::Label | Role::Status, Some(Value::Text(text))) => !text.trim().is_empty(),
            (Role::Label | Role::Status, _) => false,
            _ => true,
        };
        // A label and a status are read by their text (the role table): a
        // status is no interactive element, so rule 2 asks it for no name.
        let named = matches!(self.role, Role::Label | Role::Status) || !self.name.trim().is_empty();
        if !named {
            Some(Problem::NoName)
        } else if !value_fits {
            Some(Problem::NoValue)
        } else if self.role.is_operable()
            // A list box option is operated by being highlighted, as a
            // field's active descendant (rule 27), so Click is its own to
            // offer: an option that cannot be picked, such as a file that
            // does not read, is listed with none and stays enabled.
            && self.role != Role::ListBoxOption
            && self.actions.is_empty()
            && !focusable
            && !self.is_disabled()
        {
            Some(Problem::NotOperable)
        } else {
            None
        }
    }

    /// The AccessKit node for it, with no bounds or children yet. A focus
    /// stop offers Focus; a turned-off element offers no action, and is no
    /// stop.
    pub(crate) fn node(&self, focusable: bool) -> Node {
        let mut node = Node::new(self.role.kit());
        if !self.name.is_empty() {
            node.set_label(self.name.clone());
        }
        if let Some(description) = &self.description {
            node.set_description(description.clone());
        }
        match &self.value {
            Some(Value::Number { value, min, max, step }) => {
                node.set_numeric_value(*value);
                node.set_min_numeric_value(*min);
                node.set_max_numeric_value(*max);
                if let Some(step) = step {
                    node.set_numeric_value_step(*step);
                }
            }
            Some(Value::Text(text)) => node.set_value(text.clone()),
            Some(Value::On(on)) => node.set_toggled(if *on { Toggled::True } else { Toggled::False }),
            None => {}
        }
        if self.role == Role::Toolbar {
            node.set_orientation(accesskit::Orientation::Horizontal);
        }
        if self.states.disabled {
            node.set_disabled();
        }
        if self.states.selected {
            node.set_selected(true);
        }
        if let Some(expanded) = self.states.expanded {
            node.set_expanded(expanded);
        }
        if self.states.modal {
            node.set_modal();
        }
        if self.states.current {
            node.set_aria_current(accesskit::AriaCurrent::Page);
        }
        if let Some(position) = self.position {
            node.set_position_in_set(position);
        }
        if let Some(size) = self.size {
            node.set_size_of_set(size);
        }
        if !self.states.disabled {
            for action in &self.actions {
                node.add_action(action.kit());
            }
        }
        if focusable {
            node.add_action(KitAction::Focus);
        }
        node
    }
}

#[cfg(test)]
mod tests;
