//! The probe menu: right-click an output or a cable, then Add scope or Add
//! meter places that module next to it, already connected. Spec:
//! `docs/design/panels.md`, Add scope and Add meter.

use crate::style::{self, button};
use iced::widget::{column, container, text};
use iced::{Element, Length, Point, Rectangle, Size, Theme};
use loom_canvas::{GRID, PortRef, tile_size};
use loom_core::ModuleDescriptor;

/// Width of the menu.
const WIDTH: f32 = 140.0;
/// Height of one item.
const ITEM_HEIGHT: f32 = 26.0;
const PADDING: f32 = 4.0;
/// Height of the whole menu, for keeping it inside the canvas.
const HEIGHT: f32 = 2.0 * PADDING + 2.0 * ITEM_HEIGHT;

/// How far right of the output the new module's corner goes.
const GAP: f32 = 3.0 * GRID;
/// How far down the new module moves while it overlaps a tile.
const STEP: f32 = 2.0 * GRID;

/// What the menu adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// A scope (`loom.display.scope`).
    Scope,
    /// A level meter (`loom.display.meter`).
    Meter,
}

impl Probe {
    /// The module kind it adds.
    pub fn id(self) -> &'static str {
        match self {
            Self::Scope => "loom.display.scope",
            Self::Meter => "loom.display.meter",
        }
    }

    /// The menu item and undo step name, such as "add scope".
    pub fn verb(self) -> &'static str {
        match self {
            Self::Scope => "add scope",
            Self::Meter => "add meter",
        }
    }
}

/// What the menu asks of the app.
#[derive(Debug, Clone)]
pub enum Message {
    /// An item was chosen.
    Pick(Probe),
    /// Escape, or a click outside the menu.
    Close,
}

/// An open menu.
pub struct ProbeMenu {
    /// The output to probe.
    pub output: PortRef,
    /// Where it was opened, in world units: the output port, or the point
    /// clicked on a cable.
    pub world: Point,
    /// The menu's top-left corner, in window coordinates.
    at: Point,
}

impl ProbeMenu {
    /// Opens the menu at a window point, moved so it stays inside `area`.
    pub fn open(output: PortRef, world: Point, at: Point, area: Rectangle) -> Self {
        let x = at.x.min(area.x + area.width - WIDTH).max(area.x);
        let y = at.y.min(area.y + area.height - HEIGHT).max(area.y);
        Self {
            output,
            world,
            at: Point::new(x, y),
        }
    }

    /// The menu, placed at its corner. The app lays it over the window.
    pub fn view(&self) -> Element<'_, Message> {
        let item = |label: &'static str, probe: Probe| {
            button(text(label).size(style::CONTROL))
                .on_press(Message::Pick(probe))
                .width(Length::Fill)
                .height(ITEM_HEIGHT)
                .style(style::plain)
        };
        let menu = column![item("Add scope", Probe::Scope), item("Add meter", Probe::Meter)]
            .padding(PADDING)
            .width(WIDTH);
        let menu = container(menu).style(menu_style);
        iced::widget::pin(iced::widget::opaque(menu)).position(self.at).into()
    }
}

/// Where a new probe module goes: its top-left corner 3 grid units right of
/// `anchor`, with its first input as close to the anchor's height as the
/// grid allows, moved down in steps of 2 grid units until it overlaps none
/// of `tiles`.
pub fn place(descriptor: &ModuleDescriptor, anchor: Point, tiles: &[Rectangle]) -> Point {
    let size: Size = tile_size(
        descriptor.inputs.len(),
        descriptor.outputs.len(),
        descriptor.panel.width,
        descriptor.panel.height,
    );
    // The first input's centre, measured from the tile's top, as
    // `ModuleView::input_position` places it.
    let input = loom_canvas::HEADER + loom_canvas::ROW * 0.5;
    let start = loom_canvas::snap(Point::new(anchor.x + GAP, anchor.y - input));
    let mut corner = start;
    // Jump straight to the first step below a blocking tile: the same place
    // stepping would reach. Each jump passes one tile, so this ends after at
    // most one jump per tile. Far from the origin, f32 can no longer add a
    // step, so stop if a jump does not move.
    while let Some(tile) = tiles.iter().find(|tile| tile.intersects(&Rectangle::new(corner, size))) {
        let steps = ((tile.y + tile.height - start.y) / STEP).ceil();
        let next = start.y + steps * STEP;
        if next <= corner.y {
            break;
        }
        corner.y = next;
    }
    corner
}

fn menu_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(palette.background.base.color.into()),
        border: iced::Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.35),
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod app_tests;
