//! Canvas shortcuts shared with the editor's shortcut help.

/// A canvas shortcut and the entry that explains it in the app.
pub struct Shortcut {
    /// Key identity. The focus owner checks modifiers.
    pub key: loom_ui_core::Key,
    /// How the shortcut appears in the app's help list.
    pub help: &'static str,
}

impl Shortcut {
    /// Matches a single letter from a toolkit key event.
    pub fn matches_letter(&self, text: &str) -> bool {
        matches!(self.key, loom_ui_core::Key::Character(key) if text.eq_ignore_ascii_case(key.encode_utf8(&mut [0; 4])))
    }
}

/// A opens the browser at the pointer or at the centre of the view.
pub const ADD: Shortcut = Shortcut {
    key: loom_ui_core::Key::Character('a'),
    help: "A opens the module browser at the pointer or canvas centre. Type, then Enter adds.",
};

/// C opens the port picker for the selected tile.
pub const CABLE: Shortcut = Shortcut {
    key: loom_ui_core::Key::Character('c'),
    help: "C opens the cable picker on the selected tile.",
};

/// Space with no control focused toggles the Transport's Run control.
pub const RUN: Shortcut = Shortcut {
    key: loom_ui_core::Key::Space,
    help: "Space toggles Run when no control has focus.",
};

/// Shortcuts whose help is drawn straight from their bindings.
pub const EDITOR_SHORTCUTS: [Shortcut; 3] = [ADD, CABLE, RUN];
