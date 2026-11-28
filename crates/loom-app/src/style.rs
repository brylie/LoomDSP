//! The app's visual tokens: its text sizes, switch size, and button roles
//! (`docs/design/visual-system.md`). Every view takes its sizes and button
//! looks from here, so the app reads as one instrument, and a future theme
//! changes them in one place (FR-PANEL-06).
//!
//! The canvas keeps its own text sizes in world units, because tile text
//! grows and shrinks with zoom.

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

/// How high the right column's row of tabs is, in logical pixels: Iced's
/// row, which keeps this height, and the space the root panel leaves above
/// the inspector's slot (plan 3.2, row 4d). Iced's own was 36.9: a 13 px
/// label at a line height of 1.3, with 4 px of padding in each tab and 6 px
/// around the row.
pub const RIGHT_TABS_HEIGHT: f32 = 37.0;

/// How wide the right column is, its inspector, matrix, and outline.
pub const RIGHT_COLUMN_WIDTH: f32 = 280.0;

/// How high the breadcrumb is, its rule included, inside a sub-patch. At
/// the top level it takes no height. Iced's was 30.9 with a 1 px rule.
pub const BREADCRUMB_HEIGHT: f32 = 32.0;

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

/// The width of the danger outline on a field whose text is refused: a
/// value field, the MIDI range pair, and the Tags field (panel-widgets.md,
/// Typing a value). Twice a rule, so it reads without colour. It is drawn
/// in [`ERROR_TEXT`], 5.06:1 on the panel (#463).
pub const INVALID_BORDER: f32 = 2.0;

/// The line height of the app's text, as a multiple of its size: Iced's
/// default, kept for text `loom-ui` draws.
const LINE_HEIGHT: f32 = 1.3;

/// [`BODY`] text, for `loom-ui` elements.
pub const BODY_TEXT: loom_ui_core::TextStyle = loom_ui_core::TextStyle {
    size: BODY,
    line_height: LINE_HEIGHT,
    weight: loom_ui_core::Weight::Regular,
};

/// [`SMALL`] text, for `loom-ui` elements: details and IDs.
pub const SMALL_TEXT: loom_ui_core::TextStyle = loom_ui_core::TextStyle {
    size: SMALL,
    line_height: LINE_HEIGHT,
    weight: loom_ui_core::Weight::Regular,
};

/// Help's widest box, in logical pixels, as Iced drew it.
pub const HELP_WIDTH: f32 = 720.0;

/// [`CONTROL`] text, for `loom-ui` elements.
pub const CONTROL_TEXT: loom_ui_core::TextStyle = loom_ui_core::TextStyle {
    size: CONTROL,
    line_height: LINE_HEIGHT,
    weight: loom_ui_core::Weight::Regular,
};

/// [`TITLE`] text, for `loom-ui` elements: a dialog's title.
pub const TITLE_TEXT: loom_ui_core::TextStyle = loom_ui_core::TextStyle {
    size: TITLE,
    line_height: LINE_HEIGHT,
    weight: loom_ui_core::Weight::Regular,
};

/// The wash over the window behind a dialog (visual-system.md, Overlays):
/// black at 40%, as Iced's question had.
pub const SCRIM: loom_ui_core::Color = loom_ui_core::Color::rgba(0.0, 0.0, 0.0, 0.4);

/// The corner radius of a dialog's box, as Iced's box had
/// (visual-system.md, Overlays).
pub const OVERLAY_RADIUS: f32 = 2.0;

/// The width of the question's box, in logical pixels.
pub const QUESTION_WIDTH: f32 = 460.0;

/// The width of the licence chooser's box, in logical pixels, as Iced
/// drew it.
pub const CHOOSER_WIDTH: f32 = 560.0;

/// The licence chooser's tallest box, in logical pixels, as Iced drew it.
/// A shorter window shrinks it, and its body scrolls.
pub const CHOOSER_HEIGHT: f32 = 720.0;

/// The room between the radios of a group: more than two focus rings
/// ([`focus_ring`], 4 each side), so no ring touches a neighbour's
/// (ui-layer.md, section 6, rule 9).
pub const RADIO_SPACING: f32 = 10.0;

/// The room between an inspector port list's headings and its groups, in
/// logical pixels.
pub const PORT_SPACING: f32 = 2.0;

/// The room between two ports' lines in the inspector's port list, in
/// logical pixels: about 18 px from one port to the next (#901).
pub const PORT_LINE_SPACING: f32 = 4.0;

/// The room above a port group's heading after the first, in logical
/// pixels, so Inputs and Outputs read as two groups (#901).
pub const PORT_GROUP_GAP: f32 = 8.0;

/// How far a part that belongs to a radio sits in from the radio's mark,
/// so it lines up with the radio's label: the mark's width, 14, and the
/// gap after it, 6 (`loom-ui`'s `Element::radio`).
pub const RADIO_INDENT: f32 = 20.0;

/// The room inside a text field around its text, as Iced's field had.
pub const FIELD_PADDING: f32 = 5.0;

/// The corner radius of a menu's box (visual-system.md, Overlays,
/// `MENU_PANEL`).
pub const MENU_RADIUS: f32 = 6.0;

/// The width of a menu's border (visual-system.md, Overlays,
/// `MENU_PANEL`).
pub const MENU_BORDER: f32 = 1.0;

/// The corner radius of a menu item (visual-system.md, Overlays,
/// `MENU_ITEM`).
pub const MENU_ITEM_RADIUS: f32 = 3.0;

/// The corner radius of a browser's text field (visual-system.md,
/// Overlays, `BROWSER_FIELD`), as Iced's text input drew it.
pub const FIELD_RADIUS: f32 = 4.0;

/// The corner radius of a family swatch beside a module browser row's
/// name (visual-system.md, Overlays, `SWATCH`).
pub const SWATCH_RADIUS: f32 = 2.0;

/// A Your patches tag's outline width and corner radius (visual-system.md,
/// Overlays, `TAG`): the open-tab look while it filters.
pub const TAG_BORDER: f32 = 1.0;
/// See [`TAG_BORDER`].
pub const TAG_RADIUS: f32 = 4.0;

/// The shadow under a menu (visual-system.md, Overlays): black at 35%,
/// 4 logical pixels down, with a 12 pixel blur, as Iced's menu had.
pub const MENU_SHADOW: loom_ui::ShadowStyle = loom_ui::ShadowStyle {
    colour: loom_ui_core::Color::rgba(0.0, 0.0, 0.0, 0.35),
    offset: loom_ui_core::Vector { x: 0.0, y: 4.0 },
    blur: 12.0,
};

/// [`HEADING`] text, for `loom-ui` elements: the patch name in the
/// toolbar.
pub const HEADING_TEXT: loom_ui_core::TextStyle = loom_ui_core::TextStyle {
    size: HEADING,
    line_height: LINE_HEIGHT,
    weight: loom_ui_core::Weight::Regular,
};

/// The fill of the window's panels: the toolbar, the right column, the
/// stats bar, menus, fields, and the module browser. `#22242A`. The canvas
/// has its own background (`loom_canvas::Palette`).
///
/// This and the shades below were Iced's: its theme worked them out from
/// the app's six palette colours. They were recorded on 2026-11-27 by
/// printing each value the running app used, as exact `f32`s, not by
/// reading how Iced works them out (ADR-0018), so the look did not move
/// when the theme went (plan for leaving Iced, phase 4, P3).
pub const PANEL_FILL: loom_ui_core::Color = hex(0x22242A);

/// The body text colour on the panels, `#E6E8EE`. Not to be confused with
/// [`BODY_TEXT`], which is a size and a line height.
pub const TEXT: loom_ui_core::Color = hex(0xE6E8EE);

/// The accent colour, `#F5A623`: a chosen card's outline, a highlighted
/// row's outline, a link, and a switch that is on.
pub const ACCENT: loom_ui_core::Color = hex(0xF5A623);

/// A raised fill on the panels, about `#393D48`: an open tab, a card, a
/// dialog's box, a hovered menu item, the highlighted row, hover text.
pub const RAISED: loom_ui_core::Color = loom_ui_core::Color::rgb(0.224_145_3, 0.238_989_3, 0.284_014_14);

/// Rules between parts of the window and the borders of fields, tabs, and
/// floating panels, about `#454A59`.
pub const EDGE: loom_ui_core::Color = loom_ui_core::Color::rgb(0.272_034_94, 0.290_621_07, 0.347_204_1);

/// A modulation card's fill: [`RAISED`] at half opacity.
pub const CARD_FILL: loom_ui_core::Color = loom_ui_core::Color { a: 0.5, ..RAISED };

/// A modulation card's border: [`EDGE`] at half opacity.
pub const CARD_BORDER: loom_ui_core::Color = loom_ui_core::Color { a: 0.5, ..EDGE };

/// The fill behind selected text in a field, and behind an inspector row
/// lit as a drop target: a dark, muted amber, `#7A5520`, so [`TEXT`] on
/// it keeps 5.45:1 (WCAG 2.2, SC 1.4.3; #463). Iced's weak primary, which
/// it replaces, gave 2.54:1.
pub const SELECTION: loom_ui_core::Color = hex(0x7A5520);

/// The edge of a text field and of a select: their only boundary, so it
/// stands out at 3:1 or more against the fill around it (WCAG 2.2, SC
/// 1.4.11, which names a text field's boundary as the visual information
/// that identifies it; #463). The quiet colour: 7.12:1 on [`PANEL_FILL`].
pub const FIELD_EDGE: loom_ui_core::Color = QUIET;

/// Quiet text: details, IDs, and the title of a closed tab. Iced's own
/// secondary text colour is 4.27:1 on the panels, under the 4.5:1 that
/// WCAG asks for, so the app uses this instead (a test checks it).
pub const QUIET: loom_ui_core::Color = loom_ui_core::Color::rgb(0.66, 0.69, 0.75);
/// An unchecked switch's edge, and a choice's border on hover.
pub const CONTROL_EDGE: loom_ui_core::Color = QUIET;

/// How a check box looks (visual-system.md, Check boxes): off, a 1 px edge
/// in [`CONTROL_EDGE`]; on, filled in the canvas's control value colour,
/// with its mark in the canvas background, so a checked box reads as a set
/// value, as #856 made it for #463's contrast. The runtime's boxes read it
/// through `PanelStyle::check`; a test checks each pair for 3:1.
pub fn check_look() -> loom_ui::paint::CheckLook {
    let palette = loom_canvas::Palette::default();
    loom_ui::paint::CheckLook {
        edge: CONTROL_EDGE,
        fill: palette.control_value,
        mark: palette.background,
    }
}

/// Status bar text for a warning, with the word `Warning:` beside it.
pub const WARNING_TEXT: loom_ui_core::Color = hex(0xF5A623);
/// Status bar text for an error, with the word `Error:` beside it.
pub const ERROR_TEXT: loom_ui_core::Color = hex(0xEB6B6B);

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
/// [`Role::looks`].
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
pub const SCROLLBAR_THUMB: loom_ui_core::Color = QUIET;

/// The room a scroll container keeps at its right for its bar, scrolling
/// or not, in logical pixels: the bar and the inset on either side of it
/// (ui-layer.md, section 5, rule 12). A container cuts its content's right
/// padding by this, so its margins match (Brylie's decision, 2026-11-27).
pub fn scroll_gutter() -> f32 {
    let scroll = scroll();
    scroll.width + 2.0 * scroll.inset
}

/// The padding for content inside a scroll container: `vertical` above
/// and below, `horizontal` on the left, and on the right `horizontal` less
/// the gutter the container keeps, so the content's margins match whether
/// it scrolls or not (Brylie's decision, 2026-11-27).
pub fn scroll_padding(vertical: f32, horizontal: f32) -> loom_ui::Padding {
    loom_ui::Padding {
        top: vertical,
        right: (horizontal - scroll_gutter()).max(0.0),
        bottom: vertical,
        left: horizontal,
    }
}

/// How a scroll container of the app's own UI layer looks and moves: the
/// scrollbar tokens, the focus ring's reach kept as room around the
/// content, and one line of body text for its keys and the wheel.
pub fn scroll() -> loom_ui::ScrollStyle {
    let ring = focus_ring();
    loom_ui::ScrollStyle {
        width: SCROLLBAR_WIDTH,
        inset: SCROLLBAR_INSET,
        radius: SCROLLBAR_RADIUS,
        thumb: SCROLLBAR_THUMB,
        ring: ring.offset + ring.width,
        line: BODY_TEXT.size * BODY_TEXT.line_height,
    }
}

/// The radius of a right-column tab's corners, and of an Outline row's
/// (`tabs.rs`, `outline/panel.rs`). The open tab has a fill and a border;
/// the others are plain dim text. Shape as well as colour tells them apart
/// (NFR-UX-03).
pub const TAB_RADIUS: f32 = 4.0;

/// The width of the open right-column tab's border.
pub const TAB_BORDER: f32 = 1.0;

/// The radius of a modulation card's corners, in the inspector and the
/// matrix.
pub const CARD_RADIUS: f32 = 5.0;

/// The radius of an inspector row's corners while a dragged output over it
/// lights it.
pub const DROP_ROW_RADIUS: f32 = 4.0;

/// How a dialog over the canvas looks: help and Your patches. The
/// [`RAISED`] fill with a corner of 2, a token so `loom-ui`'s panels take
/// it (plan 3.2, rows 5b and 6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DialogLook {
    /// The panel's fill.
    pub fill: loom_ui_core::Color,
    /// Its text.
    pub text: loom_ui_core::Color,
    /// The radius of its corners.
    pub radius: f32,
}

/// The dialog look: help, Your patches, Preferences, the licence chooser,
/// and the inspector's chips read it.
pub const DIALOG: DialogLook = DialogLook {
    fill: RAISED,
    text: TEXT,
    radius: 2.0,
};

/// How a text field `loom-ui` draws looks: a box in the window's base
/// colour with a border in its strong colour and a corner of 2, as Iced's
/// field looked (visual-system.md).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldLook {
    /// The box's fill.
    pub fill: loom_ui_core::Color,
    /// Its border, [`RULE`] wide.
    pub border: loom_ui_core::Color,
    /// The radius of its corners.
    pub radius: f32,
}

/// The text field look: the inspector's, the matrix's, and the licence
/// chooser's fields read it.
pub const FIELD: FieldLook = FieldLook {
    fill: PANEL_FILL,
    border: FIELD_EDGE,
    radius: 2.0,
};

/// Help's backdrop: darker than the question's and Your patches'
/// ([`SCRIM`]), as today, since help covers the whole canvas to be read.
pub const HELP_SCRIM: loom_ui_core::Color = loom_ui_core::Color::rgba(0.0, 0.0, 0.0, 0.55);

/// How a link in help's text looks: the accent colour, as help
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

/// The link look: help's links read it.
pub const LINK: LinkLook = LinkLook {
    colour: ACCENT,
    underline: true,
};

/// How the module browser's floating panel looks: the [`PANEL_FILL`], a
/// 1 px outline in [`EDGE`], and a corner of 6.
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

/// The floating panel look: the module browser reads it, and so does a
/// select's open list ([`select`]).
pub const FLOATING_PANEL: FloatingLook = FloatingLook {
    fill: PANEL_FILL,
    text: TEXT,
    border: EDGE,
    border_width: 1.0,
    radius: 6.0,
};

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

/// The highlighted row look: the module browser's and Your patches' rows
/// read it, and a select's highlighted option takes its fill.
pub const HIGHLIGHTED_ROW: RowLook = RowLook {
    fill: RAISED,
    border: ACCENT,
    border_width: 1.0,
    radius: 4.0,
};

/// How a select of the runtime's looks, and its open list (ui-layer.md,
/// section 5, rule 18): the box in [`PANEL_FILL`] with a 1 px outline in
/// [`EDGE`], as Iced's pick list was; the list in
/// the floating panel's look with the menu's shadow, and the highlighted
/// option in the highlighted row's fill. The mark is the text colour, so it
/// keeps 3:1 against the box (WCAG 2.2, SC 1.4.11).
pub fn select() -> loom_ui::SelectStyle {
    loom_ui::SelectStyle {
        text: CONTROL_TEXT,
        colour: TEXT,
        // A raised fill, so a select reads apart from a text field, which
        // is an outline on the panel (#901).
        background: RAISED,
        border: FIELD_EDGE,
        border_width: 1.0,
        radius: 2.0,
        padding_x: 8.0,
        padding_y: 4.0,
        mark: TEXT,
        list_background: FLOATING_PANEL.fill,
        list_border: FLOATING_PANEL.border,
        highlight: HIGHLIGHTED_ROW.fill,
        shadow: Some(MENU_SHADOW),
        scroll: scroll(),
    }
}

#[cfg(test)]
mod tests;
