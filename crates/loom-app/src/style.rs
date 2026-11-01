//! The app's visual tokens: its text sizes, switch size, and button roles
//! (`docs/design/visual-system.md`). Every view takes its sizes and button
//! looks from here, so the app reads as one instrument, and a future theme
//! changes them in one place (FR-PANEL-06).
//!
//! The canvas keeps its own text sizes in world units, because tile text
//! grows and shrinks with zoom.

use iced::widget::button as iced_button;
use iced::widget::markdown as iced_markdown;
use iced::{Element, Theme};

/// Table cells, row details, and captions in the inspector and matrix.
pub const SMALL: f32 = 11.0;
/// Running text, hover text, and the status bar.
pub const BODY: f32 = 12.0;
/// Buttons, fields, pick lists, and the labels of toggles and checkboxes.
pub const CONTROL: f32 = 13.0;
/// Section headings and the patch name.
pub const HEADING: f32 = 15.0;
/// Help and dialog titles.
pub const TITLE: f32 = 18.0;
/// The marker of a toggle, checkbox, or radio button.
pub const SWITCH: f32 = 14.0;

/// Quiet text: details, IDs, and the title of a closed tab. Iced's own
/// secondary text colour is 4.27:1 on the panels, under the 4.5:1 that
/// WCAG asks for, so the app uses this instead (a test checks it).
pub const QUIET: iced::Color = iced::Color::from_rgb(0.66, 0.69, 0.75);

/// A button in the default role, secondary: quiet, so the patch and its
/// values stand out. Give it another role with `.style(style::primary)`,
/// `style::danger`, or `style::plain`.
pub fn button<'a, M>(content: impl Into<Element<'a, M>>) -> iced_button::Button<'a, M> {
    iced_button(content).style(secondary)
}

/// The one main action of a view, or a mode that is on: orange.
pub fn primary(theme: &Theme, status: iced_button::Status) -> iced_button::Style {
    iced_button::primary(theme, status)
}

/// Every other button: a lifted grey fill with light text.
pub fn secondary(theme: &Theme, status: iced_button::Status) -> iced_button::Style {
    iced_button::secondary(theme, status)
}

/// The emergency control, Panic: red.
pub fn danger(theme: &Theme, status: iced_button::Status) -> iced_button::Style {
    iced_button::danger(theme, status)
}

/// Small inline actions, such as the × that removes a row: text only.
pub fn plain(theme: &Theme, status: iced_button::Status) -> iced_button::Style {
    iced_button::text(theme, status)
}

/// A value shown as plain text that can be clicked to edit it, such as a
/// cell of the modulation matrix. A light fill on hover, and an outline
/// while it has keyboard focus, so focus never depends on colour alone.
pub fn cell(focused: bool) -> impl Fn(&Theme, iced_button::Status) -> iced_button::Style {
    move |theme, status| {
        let palette = theme.extended_palette();
        let background = matches!(status, iced_button::Status::Hovered | iced_button::Status::Pressed)
            .then(|| palette.background.weak.color.into());
        iced_button::Style {
            background,
            text_color: palette.background.base.text,
            border: iced::Border {
                color: if focused {
                    palette.primary.base.color
                } else {
                    iced::Color::TRANSPARENT
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..iced_button::Style::default()
        }
    }
}

/// A tab title. The open tab has a fill and a border; the others are plain
/// dim text. Shape as well as colour tells them apart (NFR-UX-03).
pub fn tab(open: bool) -> impl Fn(&Theme, iced_button::Status) -> iced_button::Style {
    move |theme, status| {
        let palette = theme.extended_palette();
        let hovered = matches!(status, iced_button::Status::Hovered | iced_button::Status::Pressed);
        iced_button::Style {
            background: (open || hovered).then(|| palette.background.weak.color.into()),
            text_color: if open { palette.background.base.text } else { QUIET },
            border: iced::Border {
                color: if open {
                    palette.background.strong.color
                } else {
                    iced::Color::TRANSPARENT
                },
                width: 1.0,
                radius: 4.0.into(),
            },
            ..iced_button::Style::default()
        }
    }
}

/// A card in a list, such as one modulation assignment. The selected card
/// is outlined in the primary colour.
pub fn card(selected: bool) -> impl Fn(&Theme) -> iced::widget::container::Style {
    move |theme| {
        let palette = theme.extended_palette();
        iced::widget::container::Style {
            background: Some(palette.background.weak.color.scale_alpha(0.5).into()),
            border: iced::Border {
                color: if selected {
                    palette.primary.base.color
                } else {
                    palette.background.strong.color.scale_alpha(0.5)
                },
                width: if selected { 1.5 } else { 1.0 },
                radius: 5.0.into(),
            },
            ..iced::widget::container::Style::default()
        }
    }
}

/// A pick list that reads as plain text until hovered or opened, such as
/// an assignment's polarity on its card. Its handle stays visible, so it
/// still looks like a choice.
pub fn quiet_pick_list(theme: &Theme, status: iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    let palette = theme.extended_palette();
    let active = !matches!(status, iced::widget::pick_list::Status::Active);
    iced::widget::pick_list::Style {
        text_color: palette.background.base.text,
        // Iced's strong background is 1.76:1 on the panel, too faint for
        // the one mark that says "this is a choice" (WCAG 1.4.11).
        placeholder_color: QUIET,
        handle_color: QUIET,
        background: if active {
            palette.background.weak.color.into()
        } else {
            iced::Color::TRANSPARENT.into()
        },
        border: iced::Border {
            color: if active {
                palette.background.strong.color
            } else {
                iced::Color::TRANSPARENT
            },
            width: 1.0,
            radius: 3.0.into(),
        },
    }
}

/// Markdown in the help overlay, on the same scale as the rest of the app.
pub fn markdown(theme: &Theme) -> iced_markdown::Settings {
    let mut settings = iced_markdown::Settings::with_text_size(BODY, theme);
    settings.h1_size = TITLE.into();
    settings.h2_size = HEADING.into();
    settings.h3_size = HEADING.into();
    settings.h4_size = CONTROL.into();
    settings.h5_size = CONTROL.into();
    settings.h6_size = CONTROL.into();
    settings.code_size = SMALL.into();
    settings
}

#[cfg(test)]
mod tests;
