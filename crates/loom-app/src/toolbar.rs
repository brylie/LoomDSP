//! The toolbar as plain data: what each button says, whether it works, and
//! its hover text, and the toolbar's panel, which `loom-ui` draws from it
//! (plan for leaving Iced, step 3.2, row 2). The panel's elements give the
//! toolbar's nodes too, and the UI snapshot reads the same model, so they
//! cannot disagree (`docs/design/ui-layer.md`, section 7, rule 1).

use crate::style::{self, Role as ButtonRole};
use crate::{App, FileAction, Message, learn, settings};
use loom_ui::access::{Access, Action, Role, States, Value};
use loom_ui::{Align, Element, Layer, Length, Padding, View};
use loom_ui_core::{Color, UiId};
use serde::Serialize;

/// The toolbar's panel. Every button's UI ID is inside it.
pub(crate) fn toolbar_id() -> UiId {
    UiId::panel("toolbar")
}

/// The buttons that sit together in the toolbar, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Group {
    /// New, Open, Save, and the other file commands.
    File,
    /// Add module.
    Add,
    /// Hold note and Panic.
    Play,
    /// Undo and Redo.
    Edit,
    /// MIDI learn and Preferences.
    Setup,
}

/// One toolbar button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Button {
    /// Its fixed name, the last key of its UI ID: `undo` in `toolbar/undo`.
    pub name: &'static str,
    /// The text on the button.
    pub label: &'static str,
    /// False while a press would do nothing. The hover text then says why.
    pub enabled: bool,
    /// The hover text, if the button has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hover: Option<String>,
    /// For a button that turns a mode on and off: whether it is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
    /// Its UI ID as text, for the container that wraps it in the window.
    #[serde(skip)]
    pub(crate) id: &'static str,
    /// The buttons it sits with.
    #[serde(skip)]
    pub(crate) group: Group,
    /// The command error, when this button is disabled.
    #[serde(skip)]
    pub(crate) disabled: Option<&'static str>,
}

impl Button {
    /// The message for an enabled button.
    pub(crate) fn press(&self) -> Option<Message> {
        self.enabled.then(|| message(self.name)).flatten()
    }

    /// Applies the same availability rule to the view and commands.
    fn blocked(mut self, reason: Option<&'static str>) -> Self {
        self.enabled = reason.is_none();
        self.disabled = reason;
        self
    }
}

/// The toolbar's buttons, in the order they are drawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    /// Every button the toolbar shows now.
    pub buttons: Vec<Button>,
}

/// The message a press on the button named `name` sends, or `None` for a
/// name the toolbar does not have.
pub(crate) fn message(name: &str) -> Option<Message> {
    Some(match name {
        "new" => Message::File(FileAction::New),
        "open" => Message::File(FileAction::Open),
        "save" => Message::File(FileAction::Save),
        "save_as" => Message::File(FileAction::SaveAs),
        "save_to_library" => Message::File(FileAction::SaveToLibrary),
        "browse" => Message::File(FileAction::Browse),
        "recent" => Message::OpenRecent,
        "add_module" => Message::OpenBrowser,
        "hold" => Message::HoldNote,
        "panic" => Message::Panic,
        "undo" => Message::Undo,
        "redo" => Message::Redo,
        "midi_learn" => Message::Learn(learn::Message::Toggle),
        "map" => Message::Preferences(settings::Message::Map(None)),
        "preferences" => Message::Preferences(settings::Message::Open),
        _ => return None,
    })
}

/// A button that always works and has no hover text.
fn plain(name: &'static str, id: &'static str, label: &'static str, group: Group) -> Button {
    Button {
        name,
        label,
        enabled: true,
        hover: None,
        on: None,
        id,
        group,
        disabled: None,
    }
}

impl App {
    /// The toolbar as it is now.
    pub(crate) fn toolbar_model(&self) -> Model {
        let shortcut = |mac: &'static str, other: &'static str| {
            Some(if cfg!(target_os = "macos") { mac } else { other }.to_string())
        };
        let mut buttons = vec![
            plain("new", "toolbar/new", "New", Group::File),
            plain("open", "toolbar/open", "Open", Group::File),
            plain("save", "toolbar/save", "Save", Group::File),
            plain("save_as", "toolbar/save_as", "Save As", Group::File),
        ];
        // Inside a sub-patch, the file commands also save that sub-patch to
        // the library (sub-patches.md, Library).
        if !self.session.level.is_root() {
            buttons.push(plain(
                "save_to_library",
                "toolbar/save_to_library",
                "Save to library",
                Group::File,
            ));
        }
        buttons.push(Button {
            hover: shortcut("Your patches: Cmd+Shift+O", "Your patches: Ctrl+Shift+O"),
            ..plain("browse", "toolbar/browse", "Browse", Group::File)
        });
        buttons.push(Button {
            hover: Some(self.recent_hover().to_string()),
            ..plain("recent", "toolbar/recent", "Recent", Group::File)
                .blocked(self.rig.recent.files().is_empty().then(|| self.recent_hover()))
        });
        buttons.push(Button {
            hover: Some("Press A or double-click empty canvas".to_string()),
            ..plain("add_module", "toolbar/add_module", "Add module", Group::Add)
        });
        // Disabled controls say why (visual-system.md, Status messages).
        buttons.push(Button {
            hover: self.hold_hover().map(str::to_string),
            ..plain(
                "hold",
                "toolbar/hold",
                if self.session.engine.holding {
                    "Release note"
                } else {
                    "Hold note"
                },
                Group::Play,
            )
            .blocked(self.hold_hover())
        });
        buttons.push(Button {
            hover: shortcut("Silence everything: Cmd+.", "Silence everything: Ctrl+."),
            ..plain("panic", "toolbar/panic", "Panic", Group::Play)
        });
        for (name, id, label, hint, redo) in [
            ("undo", "toolbar/undo", "Undo", self.undo_hint(), false),
            ("redo", "toolbar/redo", "Redo", self.redo_hint(), true),
        ] {
            let disabled = hint.as_ref().err().map(|block| block.press_error(redo));
            buttons.push(Button {
                hover: Some(hint.unwrap_or_else(|block| block.hover(redo).to_string())),
                ..plain(name, id, label, Group::Edit).blocked(disabled)
            });
        }
        buttons.push(Button {
            on: Some(self.session.learn.is_on()),
            ..plain("midi_learn", "toolbar/midi_learn", "MIDI learn", Group::Setup)
        });
        buttons.push(Button {
            hover: Some(format!(
                "Patch map: {}. Press M to cycle.",
                self.rig.store.saved().patch_map
            )),
            ..plain("map", "toolbar/map", "Map", Group::Setup)
        });
        buttons.push(Button {
            hover: shortcut("Cmd+,", "Ctrl+,"),
            ..plain("preferences", "toolbar/preferences", "Preferences", Group::Setup)
        });
        Model { buttons }
    }
}

#[cfg(test)]
mod tests;

/// What the toolbar's panel is drawn from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Panel {
    /// The buttons.
    pub(crate) model: Model,
    /// The patch name it shows first, with `* ` while it is unsaved.
    pub(crate) title: String,
    /// The rule under it, in `style::EDGE`, as Iced's was.
    pub(crate) rule: Color,
}

impl App {
    /// Adds the toolbar's panel to the runtime's view, for its slot in the
    /// window's root panel (`chrome.rs`).
    pub(crate) fn toolbar_panel(&self, view: &mut View) {
        let panel = Panel {
            model: self.toolbar_model(),
            title: format!("{}{}", if self.unsaved() { "* " } else { "" }, self.patch_name()),
            rule: crate::style::EDGE,
        };
        view.panel_with(toolbar_id(), Layer::Panels, &panel, toolbar);
    }

    /// The toolbar's focus stops in the order Tab meets them: its turned-on
    /// buttons, left to right.
    pub(crate) fn toolbar_stops(&self) -> Vec<UiId> {
        self.toolbar_model()
            .buttons
            .iter()
            .filter(|button| button.enabled)
            .map(|button| toolbar_id().with(button.name))
            .collect()
    }

    /// The message for a click, from the pointer, Enter or Space, or a
    /// screen reader, on a toolbar button: its press, while it is turned on.
    pub(crate) fn toolbar_click(&self, id: &UiId) -> Option<Message> {
        let [_, loom_ui_core::IdKey::Name(name)] = id.keys() else {
            return None;
        };
        if !id.is_under(&toolbar_id()) {
            return None;
        }
        let model = self.toolbar_model();
        model.buttons.iter().find(|button| button.name == &**name)?.press()
    }
}

/// A button's role: Panic is danger, a mode that is on is primary, and the
/// rest are secondary (visual-system.md, Toolbar).
pub(crate) fn role_of(button: &Button) -> ButtonRole {
    match (button.name, button.on) {
        ("panic", _) => ButtonRole::Danger,
        (_, Some(true)) => ButtonRole::Primary,
        _ => ButtonRole::Secondary,
    }
}

/// The toolbar: the patch name, then the buttons in their groups, with
/// wider space between groups than inside them, and the rule under it.
fn toolbar(panel: &Panel) -> Element {
    let group = |which: Group| {
        Element::row(
            panel
                .model
                .buttons
                .iter()
                .filter(|button| button.group == which)
                .map(button),
        )
        .spacing(style::BUTTON_SPACING)
        .align_y(Align::Center)
    };
    let row = Element::row([
        Element::text(panel.title.clone(), style::HEADING_TEXT),
        group(Group::File),
        Element::space(Length::FILL, Length::Shrink),
        group(Group::Add),
        group(Group::Play),
        group(Group::Edit),
        group(Group::Setup),
    ])
    .spacing(20.0)
    .padding(Padding::symmetric(6.0, 10.0))
    .align_y(Align::Center)
    .width(Length::FILL)
    .height(Length::FILL);
    Element::column([
        row,
        Element::space(Length::FILL, Length::Fixed(style::RULE)).background(panel.rule),
    ])
    .width(Length::FILL)
    .height(Length::FILL)
}

/// One button: its label in its role's looks, a focus stop while it is
/// turned on, with its hover text, which is also its description, and a
/// mode's on or off (ui-layer.md, section 7, the toolbar button's row).
fn button(button: &Button) -> Element {
    let role = role_of(button);
    let mut access = Access::new(Role::Button, button.label);
    if let Some(on) = button.on {
        access = access.value(Value::On(on));
    }
    access = if button.enabled {
        access.action(Action::Click)
    } else {
        access.states(States {
            disabled: true,
            ..States::default()
        })
    };
    let face = Element::text(button.label, style::CONTROL_TEXT)
        .padding(style::BUTTON_PADDING)
        .radius(role.tokens().radius)
        .looks(role.looks())
        .key(button.name)
        .focusable()
        .access(access);
    match &button.hover {
        Some(hover) => face.hover_text(hover.clone()),
        None => face,
    }
}
