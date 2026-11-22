//! The app's visual tokens: its text sizes, switch size, and button roles
//! (`docs/design/visual-system.md`). Every view takes its sizes and button
//! looks from here, so the app reads as one instrument, and a future theme
//! changes them in one place (FR-PANEL-06).
//!
//! The canvas keeps its own text sizes in world units, because tile text
//! grows and shrinks with zoom.

use crate::Element;
use iced::Theme;
use iced::widget::button as iced_button;
use iced::widget::markdown as iced_markdown;

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

/// How high the stats bar is, in logical pixels: the bar `loom-ui` draws
/// and the space Iced keeps for it, so the two cannot disagree. Whole
/// pixels, since the runtime's layout rounds to them (2026-11-17).
pub const STATS_BAR_HEIGHT: f32 = 24.0;

/// How high the toolbar is, its rule included, in logical pixels: the
/// panel `loom-ui` draws and its share of the space Iced keeps above the
/// canvas. Iced's toolbar was 38.9 high with a 1 px rule; whole pixels,
/// since the runtime's layout rounds to them.
pub const TOOLBAR_HEIGHT: f32 = 40.0;

/// How high the breadcrumb is, its rule included, inside a sub-patch. At
/// the top level it takes no height. Iced's was 30.9 with a 1 px rule.
pub const BREADCRUMB_HEIGHT: f32 = 32.0;

/// The height above the canvas that the toolbar and, `inside` a sub-patch,
/// the breadcrumb take: the root panel's slots and Iced's space both use
/// it, so the canvas starts where the panels end.
pub const fn top_height(inside: bool) -> f32 {
    if inside {
        TOOLBAR_HEIGHT + BREADCRUMB_HEIGHT
    } else {
        TOOLBAR_HEIGHT
    }
}

/// The room around a button's label: 5 above and below, 10 at the sides,
/// as Iced's buttons had.
pub const BUTTON_PADDING: loom_ui::Padding = loom_ui::Padding::symmetric(5.0, 10.0);

/// The space between buttons side by side: within a toolbar group, and
/// between the breadcrumb's entries and their separators. At least the
/// focus ring's reach (its offset and its width) plus a margin, so a ring
/// never covers a neighbour's fill or its text (ui-layer.md, section 6,
/// rule 3; `no_focus_ring_reaches_a_neighbour`).
pub const BUTTON_SPACING: f32 = 6.0;

/// The width of a rule between parts of the window.
pub const RULE: f32 = 1.0;

/// The line height of the app's text, as a multiple of its size: Iced's
/// default, kept for text `loom-ui` draws.
const LINE_HEIGHT: f32 = 1.3;

/// [`BODY`] text, for `loom-ui` elements.
pub const BODY_TEXT: loom_ui_core::TextStyle = loom_ui_core::TextStyle {
    size: BODY,
    line_height: LINE_HEIGHT,
    weight: loom_ui_core::Weight::Regular,
};

/// [`CONTROL`] text, for `loom-ui` elements.
pub const CONTROL_TEXT: loom_ui_core::TextStyle = loom_ui_core::TextStyle {
    size: CONTROL,
    line_height: LINE_HEIGHT,
    weight: loom_ui_core::Weight::Regular,
};

/// [`HEADING`] text, for `loom-ui` elements: the patch name in the
/// toolbar.
pub const HEADING_TEXT: loom_ui_core::TextStyle = loom_ui_core::TextStyle {
    size: HEADING,
    line_height: LINE_HEIGHT,
    weight: loom_ui_core::Weight::Regular,
};

/// Quiet text: details, IDs, and the title of a closed tab. Iced's own
/// secondary text colour is 4.27:1 on the panels, under the 4.5:1 that
/// WCAG asks for, so the app uses this instead (a test checks it).
pub const QUIET: iced::Color = iced::Color::from_rgb(0.66, 0.69, 0.75);
/// Status bar text for a warning, with the word `Warning:` beside it.
pub const WARNING_TEXT: iced::Color = iced::Color::from_rgb8(0xF5, 0xA6, 0x23);
/// Status bar text for an error, with the word `Error:` beside it.
pub const ERROR_TEXT: iced::Color = iced::Color::from_rgb8(0xEB, 0x6B, 0x6B);

/// A button in the default role, secondary: quiet, so the patch and its
/// values stand out. Give it another role with `.style(style::primary)`,
/// or `style::plain`. Danger is Panic's, which `loom-ui` draws (`toolbar.rs`).
pub fn button<'a, M>(content: impl Into<Element<'a, M>>) -> iced_button::Button<'a, M, Theme, crate::Renderer> {
    iced_button(content).style(secondary)
}

/// A button role's look in one state: its fill, or none, and its text
/// (visual-system.md, Role tokens).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoleState {
    /// The fill inside the button, or none.
    pub fill: Option<loom_ui_core::Color>,
    /// The colour of its label.
    pub text: loom_ui_core::Color,
}

/// A button role's look in each state. These tokens are the one source for
/// both toolkits until Iced goes: `loom-ui`'s buttons take them through
/// [`Role::looks`], and Iced's through the role style functions below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoleTokens {
    /// Neither hovered nor pressed, and turned on.
    pub normal: RoleState,
    /// The pointer is over it.
    pub hovered: RoleState,
    /// A press on it is held.
    pub pressed: RoleState,
    /// Turned off.
    pub off: RoleState,
    /// The corners' radius: 2 for a filled role, 0 for plain.
    pub radius: f32,
}

/// A state with a fill.
const fn filled(fill: loom_ui_core::Color, text: loom_ui_core::Color) -> RoleState {
    RoleState { fill: Some(fill), text }
}

/// A state with no fill.
const fn bare(text: loom_ui_core::Color) -> RoleState {
    RoleState { fill: None, text }
}

/// `#RRGGBB` as a colour.
const fn hex(rgb: u32) -> loom_ui_core::Color {
    loom_ui_core::Color::rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Primary: the one main action of a view, or a mode that is on. Orange.
pub const PRIMARY: RoleTokens = RoleTokens {
    normal: filled(hex(0xF5A623), hex(0x2A1A00)),
    hovered: filled(hex(0xF9C876), hex(0x2A1A00)),
    pressed: filled(hex(0xDC8D0A), hex(0x2A1A00)),
    off: filled(hex(0x5C4A2E), hex(0xCDBD9E)),
    radius: 2.0,
};

/// Secondary: every other button, and the default. A lifted grey.
pub const SECONDARY: RoleTokens = RoleTokens {
    normal: filled(hex(0x7F828D), hex(0x0E0E10)),
    hovered: filled(hex(0x9497A0), hex(0x0E0E10)),
    pressed: filled(hex(0x6C6F7A), hex(0xF2F3F6)),
    off: filled(hex(0x3E414B), hex(0xAEB1BB)),
    radius: 2.0,
};

/// Danger: Panic. Red.
pub const DANGER: RoleTokens = RoleTokens {
    normal: filled(hex(0xE04F4F), hex(0x0E0E10)),
    hovered: filled(hex(0xE77474), hex(0x0E0E10)),
    pressed: filled(hex(0xD42525), hex(0xF2F3F6)),
    off: filled(hex(0x5A3434), hex(0xC9ABAB)),
    radius: 2.0,
};

/// Plain: small inline actions, the × that removes a row, and the
/// breadcrumb's entries. Text only, with a fill as feedback.
pub const PLAIN: RoleTokens = RoleTokens {
    normal: bare(hex(0xE6E8EE)),
    hovered: filled(hex(0x40444F), hex(0xE6E8EE)),
    pressed: filled(hex(0x50556A), hex(0xE6E8EE)),
    off: bare(hex(0x8E929C)),
    radius: 0.0,
};

/// A button role (visual-system.md, Button roles).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The one main action of a view, or a mode that is on.
    Primary,
    /// Every other button.
    Secondary,
    /// Panic.
    Danger,
    /// Small inline actions.
    Plain,
}

impl Role {
    /// Its tokens.
    pub const fn tokens(self) -> RoleTokens {
        match self {
            Self::Primary => PRIMARY,
            Self::Secondary => SECONDARY,
            Self::Danger => DANGER,
            Self::Plain => PLAIN,
        }
    }

    /// Its looks for a `loom-ui` element, from the same tokens.
    pub fn looks(self) -> loom_ui::Looks {
        let tokens = self.tokens();
        let state = |state: RoleState| loom_ui::StateLook {
            background: state.fill,
            text: Some(state.text),
        };
        loom_ui::Looks {
            normal: state(tokens.normal),
            hovered: state(tokens.hovered),
            pressed: state(tokens.pressed),
            off: state(tokens.off),
        }
    }

    /// [`Role::iced`], for the tests: danger has no Iced style function,
    /// since its one button, Panic, is `loom-ui`'s.
    #[cfg(test)]
    pub(crate) fn iced_for_tests(self, status: iced_button::Status) -> iced_button::Style {
        self.iced(status)
    }

    /// Its style for an Iced button in `status`: Iced keeps its own state
    /// handling, and each of its states takes the token for that state.
    fn iced(self, status: iced_button::Status) -> iced_button::Style {
        use loom_canvas::iced_convert::ToIced as _;
        let tokens = self.tokens();
        let state = match status {
            iced_button::Status::Active => tokens.normal,
            iced_button::Status::Hovered => tokens.hovered,
            iced_button::Status::Pressed => tokens.pressed,
            iced_button::Status::Disabled => tokens.off,
        };
        iced_button::Style {
            background: state.fill.map(|fill| iced::Background::Color(fill.to_iced())),
            text_color: state.text.to_iced(),
            border: iced::Border {
                radius: tokens.radius.into(),
                ..iced::Border::default()
            },
            ..iced_button::Style::default()
        }
    }
}

/// The one main action of a view, or a mode that is on: orange.
pub fn primary(_theme: &Theme, status: iced_button::Status) -> iced_button::Style {
    Role::Primary.iced(status)
}

/// Every other button: a lifted grey fill with dark text.
pub fn secondary(_theme: &Theme, status: iced_button::Status) -> iced_button::Style {
    Role::Secondary.iced(status)
}

/// Small inline actions, such as the × that removes a row: text only.
pub fn plain(_theme: &Theme, status: iced_button::Status) -> iced_button::Style {
    Role::Plain.iced(status)
}

/// `base`, outlined in the primary colour while `on`: the item of a menu
/// that its keys are on. The outline sits on the button's own border, so it
/// moves nothing, and the highlight never depends on colour alone.
pub fn outlined(
    base: fn(&Theme, iced_button::Status) -> iced_button::Style,
    on: bool,
) -> impl Fn(&Theme, iced_button::Status) -> iced_button::Style {
    move |theme, status| {
        let mut style = base(theme, status);
        if on {
            style.border.color = theme.extended_palette().primary.base.color;
            style.border.width = 1.5;
        }
        style
    }
}

/// The tile and probe menu surface, with an opaque fill and a visible
/// edge where it crosses a tile or empty canvas.
pub fn canvas_menu(theme: &Theme) -> iced::widget::container::Style {
    let palette = theme.extended_palette();
    iced::widget::container::Style {
        background: Some(palette.background.base.color.into()),
        border: iced::Border {
            color: QUIET,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..iced::widget::container::Style::default()
    }
}

/// A canvas menu row. The edge marks the hit area and the accent marks
/// the row under the pointer or keyboard focus.
pub fn menu_item(focused: bool) -> impl Fn(&Theme, iced_button::Status) -> iced_button::Style {
    move |theme, status| {
        let palette = theme.extended_palette();
        let active = matches!(status, iced_button::Status::Hovered | iced_button::Status::Pressed) || focused;
        iced_button::Style {
            background: Some(
                if active {
                    palette.background.weak.color
                } else {
                    palette.background.base.color
                }
                .into(),
            ),
            text_color: if status == iced_button::Status::Disabled {
                QUIET
            } else {
                palette.background.base.text
            },
            border: iced::Border {
                color: if active { palette.primary.base.color } else { QUIET },
                width: if active { 2.0 } else { 1.0 },
                radius: 3.0.into(),
            },
            ..iced_button::Style::default()
        }
    }
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

/// The focus ring of a part the app's own UI layer draws (ui-layer.md,
/// section 6, rule 3): the canvas's focus colour, so focus looks the same
/// on a tile and in a panel, 2 logical pixels wide and 2 outside the part.
/// It stands out at 3:1 or more against every panel background
/// (WCAG 2.2, 1.4.11 and 2.4.13), which a test checks.
pub fn focus_ring() -> loom_ui::RingStyle {
    loom_ui::RingStyle {
        color: loom_canvas::Palette::default().focus,
        width: 2.0,
        offset: 2.0,
    }
}

/// The width of a scroll container's thumb (ui-layer.md, section 5,
/// rule 12).
pub const SCROLLBAR_WIDTH: f32 = 6.0;

/// The room between a scroll container's thumb and its edges.
pub const SCROLLBAR_INSET: f32 = 2.0;

/// The radius of a scroll container's thumb: half its width, so its ends
/// are round.
pub const SCROLLBAR_RADIUS: f32 = 3.0;

/// The colour of a scroll container's thumb: the quiet text colour, which
/// stands out at 3:1 or more against the panel it runs over, as no track is
/// drawn (WCAG 2.2, 1.4.11). A test checks it.
pub const SCROLLBAR_THUMB: iced::Color = QUIET;

/// How a scroll container of the app's own UI layer looks and moves: the
/// scrollbar tokens, the focus ring's reach kept as room around the
/// content, and one line of body text for its keys and the wheel.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "used once the inspector moves to loom-ui (leaving-iced plan, row 4d)"
    )
)]
pub fn scroll() -> loom_ui::ScrollStyle {
    use loom_canvas::iced_convert::ToCore as _;
    let ring = focus_ring();
    loom_ui::ScrollStyle {
        width: SCROLLBAR_WIDTH,
        inset: SCROLLBAR_INSET,
        radius: SCROLLBAR_RADIUS,
        thumb: SCROLLBAR_THUMB.to_core(),
        ring: ring.offset + ring.width,
        line: BODY_TEXT.size * BODY_TEXT.line_height,
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
    // The link token, which `loom-ui`'s help takes too (with its underline).
    settings.style.link_color = {
        use loom_canvas::iced_convert::ToIced as _;
        link(theme).colour.to_iced()
    };
    settings
}

/// How a dialog over the canvas looks: help and Your patches. Today's
/// look, the palette's weak background with a corner of 2, now a token so
/// `loom-ui`'s panels take it (plan 3.2, rows 5b and 6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialogLook {
    /// The panel's fill.
    pub fill: loom_ui_core::Color,
    /// Its text.
    pub text: loom_ui_core::Color,
    /// The radius of its corners.
    pub radius: f32,
}

/// The dialog look in `theme`.
pub fn dialog(theme: &Theme) -> DialogLook {
    use loom_canvas::iced_convert::ToCore as _;
    let palette = theme.extended_palette();
    DialogLook {
        fill: palette.background.weak.color.to_core(),
        text: palette.background.weak.text.to_core(),
        radius: 2.0,
    }
}

/// How dark help's backdrop is: black at this alpha, as today.
pub const HELP_SCRIM: f32 = 0.55;

/// How dark Your patches' backdrop is: black at this alpha, as today, the
/// question's too.
pub const PATCHES_SCRIM: f32 = 0.4;

/// How a link in help's text looks: the theme's primary colour, as help
/// shows links today, and an underline, since the colour alone is too
/// close to the body text to tell a link apart (WCAG 2.2, 1.4.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinkLook {
    /// The colour of its words and its line.
    pub colour: loom_ui_core::Color,
    /// It is underlined. Required while the colour is under 3:1 against
    /// the body text (WCAG 2.2, 1.4.1, technique G183), which a test checks.
    pub underline: bool,
}

/// The link look in `theme`.
pub fn link(theme: &Theme) -> LinkLook {
    use loom_canvas::iced_convert::ToCore as _;
    LinkLook {
        colour: theme.extended_palette().primary.base.color.to_core(),
        underline: true,
    }
}

/// How the module browser's floating panel looks: the palette's base
/// background, a 1 px outline in its strong colour, and a corner of 6, as
/// today.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatingLook {
    /// The panel's fill.
    pub fill: loom_ui_core::Color,
    /// Its text.
    pub text: loom_ui_core::Color,
    /// Its outline's colour.
    pub border: loom_ui_core::Color,
    /// Its outline's width.
    pub border_width: f32,
    /// The radius of its corners.
    pub radius: f32,
}

/// The floating panel look in `theme`.
pub fn floating_panel(theme: &Theme) -> FloatingLook {
    use loom_canvas::iced_convert::ToCore as _;
    let palette = theme.extended_palette();
    FloatingLook {
        fill: palette.background.base.color.to_core(),
        text: palette.background.base.text.to_core(),
        border: palette.background.strong.color.to_core(),
        border_width: 1.0,
        radius: 6.0,
    }
}

/// How a browser's highlighted row looks: a lifted grey with a 1 px
/// outline in the primary colour and a corner of 4, as today, so its
/// quieter text stays readable on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowLook {
    /// The row's fill.
    pub fill: loom_ui_core::Color,
    /// Its outline's colour.
    pub border: loom_ui_core::Color,
    /// Its outline's width.
    pub border_width: f32,
    /// The radius of its corners.
    pub radius: f32,
}

/// The highlighted row look in `theme`.
pub fn highlighted_row(theme: &Theme) -> RowLook {
    use loom_canvas::iced_convert::ToCore as _;
    let palette = theme.extended_palette();
    RowLook {
        fill: palette.background.weak.color.to_core(),
        border: palette.primary.base.color.to_core(),
        border_width: 1.0,
        radius: 4.0,
    }
}

#[cfg(test)]
mod tests;
