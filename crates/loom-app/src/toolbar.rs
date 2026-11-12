//! The toolbar as plain data: what each button says, whether it works, and
//! its hover text. The window draws its buttons from this model, and the
//! accessibility tree and the UI snapshot read the same model, so the three
//! cannot disagree (`docs/design/ui-layer.md`, section 7, rule 1).

use crate::{App, FileAction, Message, learn, settings};
use serde::Serialize;

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
            enabled: !self.rig.recent.files().is_empty(),
            hover: Some(self.recent_hover().to_string()),
            ..plain("recent", "toolbar/recent", "Recent", Group::File)
        });
        buttons.push(Button {
            hover: Some("Or press A over the canvas, or double-click empty canvas".to_string()),
            ..plain("add_module", "toolbar/add_module", "Add module", Group::Add)
        });
        // Disabled controls say why (visual-system.md, Status messages).
        buttons.push(Button {
            enabled: self.session.engine.midi.is_some(),
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
        });
        buttons.push(Button {
            hover: shortcut("Silence everything: Cmd+.", "Silence everything: Ctrl+."),
            ..plain("panic", "toolbar/panic", "Panic", Group::Play)
        });
        for (name, id, label, hint, redo) in [
            ("undo", "toolbar/undo", "Undo", self.undo_hint(), false),
            ("redo", "toolbar/redo", "Redo", self.redo_hint(), true),
        ] {
            buttons.push(Button {
                enabled: hint.is_ok(),
                hover: Some(hint.unwrap_or_else(|block| block.hover(redo).to_string())),
                ..plain(name, id, label, Group::Edit)
            });
        }
        buttons.push(Button {
            on: Some(self.session.learn.is_on()),
            ..plain("midi_learn", "toolbar/midi_learn", "MIDI learn", Group::Setup)
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
