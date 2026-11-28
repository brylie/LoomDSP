//! Where the canvas is, for the accessibility tree and the control
//! channel, and the names the accessibility layer gives text fields.
//!
//! Every other part of the window is placed by the runtime's layout, by
//! its UI ID (`Context::bounds`); the canvas's place and camera come from
//! its own element (`canvas.rs`).

use iced::advanced::widget::Id;
use loom_canvas::{CanvasTyped, CanvasView};
use loom_ui_core::UiId;

/// What starts the widget ID of a text field: only [`field_widget_id`]
/// makes such an ID, so [`field_of`] never takes a named container for a
/// field, whatever its UI ID (the question's sentence is `question/text`).
const FIELD: &str = "field:";

/// True for the characters a field's widget ID holds as they are. Every
/// other byte is written `%XX`, so the ID's text needs no escape when Iced
/// prints it, and [`field_of`] can read it back.
fn plain(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.')
}

/// The widget ID of the text field in the part named `part`, such as the
/// value field `inspector/4/param/cutoff/field`. Focus effects and the view
/// both use it, and [`field_of`] reads it back.
pub(crate) fn field_widget_id(part: &UiId) -> Id {
    let mut text = String::from(FIELD);
    for byte in part.to_string().bytes() {
        if plain(byte) {
            text.push(byte as char);
        } else {
            text.push_str(&format!("%{byte:02X}"));
        }
    }
    Id::from(text)
}

/// The part whose text field has the widget ID `id`, or `None` for any ID
/// [`field_widget_id`] did not make.
///
/// Iced gives an ID's text only through `Debug`, which prints
/// `Id(Custom("..."))`. A field's ID holds only characters that print as
/// themselves there, so the text between the quotes is the ID itself. The
/// test `iced_prints_a_field_id_as_it_is` holds that, and this goes with the
/// bridge at plan step 2.2.
pub(crate) fn field_of(id: &Id) -> Option<UiId> {
    let printed = format!("{id:?}");
    let text = printed.strip_prefix("Id(Custom(\"")?.strip_suffix("\"))")?;
    let escaped = text.strip_prefix(FIELD)?;
    let mut bytes = Vec::with_capacity(escaped.len());
    let mut rest = escaped.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else if plain(byte) {
            bytes.push(byte);
            rest = tail;
        } else {
            return None;
        }
    }
    String::from_utf8(bytes).ok()?.parse().ok()
}

/// What the window holds of the canvas: its place and camera, and its
/// open typed field.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Places {
    /// The patch canvas and its camera.
    pub canvas: Option<CanvasView>,
    /// The value editor drawn over a tile, if open.
    pub typed: Option<CanvasTyped>,
    /// The part a search looked for by its UI ID is in the layout but
    /// scrolled out of view: a pointer sent there would land on something
    /// else, so `locate` refuses it.
    pub(crate) scrolled_out: bool,
}

impl Places {
    /// Places with only the canvas known, as tests of the canvas tree use.
    #[cfg(test)]
    pub(crate) fn of_canvas(canvas: Option<CanvasView>) -> Self {
        Self {
            canvas,
            ..Self::default()
        }
    }

    /// The canvas and its open typed field, read from its element.
    pub(crate) fn live(canvas: Option<CanvasView>, typed: Option<CanvasTyped>) -> Self {
        Self {
            canvas,
            typed,
            scrolled_out: false,
        }
    }
}

#[cfg(test)]
mod tests;
