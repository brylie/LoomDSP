//! Where text goes on a tile: the title, port labels, and panel captions
//! (`docs/design/panels.md`, Level of detail).
//!
//! One list serves three uses: the text drawn close up, the faint bars that
//! stand in for it further out, and hover on port labels. Keeping them in
//! one place means a bar always sits exactly where its text would be.
//!
//! Positions are anchors, as Iced reads them: centred text is centred on
//! its anchor, and right-aligned text ends at it.

use crate::controls::{knob_centre, knob_drawn, light_centre, radio_centre};
use crate::scene::{CLOCK_MARK_SPACE, GRID, HEADER, ModuleView, PortRef, WidgetViewKind, fit_label, text_width};
use iced::{Point, Rectangle, Size};
use std::borrow::Cow;

/// Title size in world units.
pub const TITLE_SIZE: f32 = 11.0;
/// Port label and caption size in world units.
pub const LABEL_SIZE: f32 = 9.0;

/// How text sits on its anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Starts at the anchor.
    Left,
    /// Centred on the anchor.
    Center,
    /// Ends at the anchor.
    Right,
}

/// What a piece of tile text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    /// The module title, in the header.
    Title,
    /// A placeholder's text marker.
    Unknown,
    /// A port's label.
    Port(PortRef),
    /// The caption of a panel widget, by index.
    Caption(usize),
    /// Option `.1` of the radio group at widget index `.0`.
    Option(usize, usize),
}

/// One piece of text on a tile.
#[derive(Debug, Clone, PartialEq)]
pub struct TextItem<'a> {
    /// What it is.
    pub kind: TextKind,
    /// The text, already cut with "…" to fit where needed.
    pub content: Cow<'a, str>,
    /// World point the text is aligned to, at its vertical centre.
    pub anchor: Point,
    /// How the text sits on the anchor.
    pub align: Align,
    /// Font size in world units.
    pub size: f32,
    /// Room for the text in world units. Longer text is clipped.
    pub width: f32,
}

impl TextItem<'_> {
    /// The estimated box of the text, in world units.
    pub fn bounds(&self) -> Rectangle {
        let width = text_width(&self.content, self.size).min(self.width);
        let x = match self.align {
            Align::Left => self.anchor.x,
            Align::Center => self.anchor.x - width * 0.5,
            Align::Right => self.anchor.x - width,
        };
        let height = self.size * 1.2;
        Rectangle::new(Point::new(x, self.anchor.y - height * 0.5), Size::new(width, height))
    }

    /// The faint bar that stands in for the text when it is too small to
    /// draw: as wide as the text, and half as tall as its font size, about
    /// the height of its lower-case letters.
    pub fn bar(&self) -> Rectangle {
        let text = self.bounds();
        let height = self.size * 0.5;
        Rectangle::new(
            Point::new(text.x, self.anchor.y - height * 0.5),
            Size::new(text.width, height),
        )
    }
}

/// Calls `f` with every piece of text on a module's tile, in drawing order:
/// title, port labels, then panel text.
pub fn for_each<'a>(module: &'a ModuleView, mut f: impl FnMut(TextItem<'a>)) {
    let tile = module.bounds();
    let pad = 7.0;
    f(TextItem {
        kind: TextKind::Title,
        content: Cow::Borrowed(&module.title),
        anchor: Point::new(tile.x + pad, tile.y + HEADER * 0.5),
        align: Align::Left,
        size: TITLE_SIZE,
        width: module.width() - 2.0 * pad,
    });

    if module.unknown {
        f(TextItem {
            kind: TextKind::Unknown,
            content: Cow::Borrowed("? Unknown module"),
            anchor: Point::new(
                tile.x + tile.width * 0.5,
                tile.y + HEADER + (tile.height - HEADER) * 0.5,
            ),
            align: Align::Center,
            size: LABEL_SIZE,
            width: tile.width * 0.5,
        });
    }

    // What the panel draws, so labels can grow only into empty space on
    // their line (panels.md, Labels and hover text; #455).
    let panel = panel_boxes(module);

    let label_width = module.label_width();
    let ports = module
        .inputs
        .iter()
        .enumerate()
        .map(|(i, p)| (false, i, p))
        .chain(module.outputs.iter().enumerate().map(|(i, p)| (true, i, p)));
    let mut port_boxes = Vec::new();
    for (output, index, port) in ports {
        let y = if output {
            module.output_position(index).y
        } else {
            module.input_position(index).y
        };
        // A clock mark goes between the marker and the label, so the label
        // moves right and has less room (panels.md, Clock mark).
        let mark = if !output && port.follows_transport.is_some() {
            CLOCK_MARK_SPACE
        } else {
            0.0
        };
        let (x, align) = if output {
            (tile.x + module.width() - 9.0, Align::Right)
        } else {
            (tile.x + 9.0 + mark, Align::Left)
        };
        // A label may grow inward, into empty space on its row, as far as
        // its half of the tile. A tile with an empty panel already gives
        // each side half the tile.
        let mut width = label_width - mark;
        if !module.panel.widgets.is_empty() {
            let row = line_box(tile.x, tile.width, y);
            let middle = tile.center_x();
            let on_row = panel.iter().map(|o| o.bounds).filter(|b| overlaps_line(*b, row));
            let room = if output {
                let edge = on_row.filter(|b| b.x < x).map(|b| b.x + b.width).fold(middle, f32::max);
                x - edge - GAP
            } else {
                let edge = on_row.filter(|b| b.x + b.width > x).map(|b| b.x).fold(middle, f32::min);
                edge - GAP - x
            };
            width = width.max(room);
        }
        let item = TextItem {
            kind: TextKind::Port(PortRef {
                module: module.id,
                output,
                index,
            }),
            content: fit_label(&port.label, width, LABEL_SIZE),
            anchor: Point::new(x, y),
            align,
            size: LABEL_SIZE,
            width,
        };
        port_boxes.push(item.bounds());
        f(item);
    }

    let (inner_left, inner_right) = (tile.x + 9.0, tile.x + tile.width - 9.0);
    for (index, widget) in module.panel.widgets.iter().enumerate() {
        let Some(rect) = module.widget_bounds(index) else {
            continue;
        };
        let y = caption_y(&widget.kind, rect);
        let line = line_box(tile.x, tile.width, y);
        let centre = rect.center_x();
        let natural = text_width(&widget.label, LABEL_SIZE);
        let own = natural.min(rect.width) * 0.5;
        // How far the caption may reach on each side: past what other
        // widgets draw, halfway into a gap it shares with other text (less
        // half of `GAP`, so two captions never run together), and up to the
        // port labels and the port markers' inset.
        let (mut left, mut right) = (inner_left, inner_right);
        for o in panel
            .iter()
            .filter(|o| o.owner != index && overlaps_line(o.bounds, line))
        {
            let (b, text) = (o.bounds, o.text);
            let (near_left, near_right) = if text {
                (centre - own, centre + own)
            } else {
                (centre, centre)
            };
            if b.x + b.width <= near_left + 0.01 {
                left = left.max(if text {
                    (b.x + b.width + near_left + GAP) * 0.5
                } else {
                    b.x + b.width + GAP
                });
            } else if b.x >= near_right - 0.01 {
                right = right.min(if text {
                    (b.x + near_right - GAP) * 0.5
                } else {
                    b.x - GAP
                });
            } else {
                (left, right) = (left.max(centre - own), right.min(centre + own));
            }
        }
        for b in port_boxes.iter().filter(|b| overlaps_line(**b, line)) {
            if b.center_x() < centre {
                left = left.max(b.x + b.width + GAP);
            } else {
                right = right.min(b.x - GAP);
            }
        }
        // Centred when it fits. Otherwise moved the least it needs toward
        // the free side, as long as it stays over its widget. Otherwise cut.
        let half = (centre - left).min(right - centre);
        let (anchor, width) = if natural * 0.5 <= half {
            (centre, rect.width.max(2.0 * half))
        } else {
            let moved = if centre - left < right - centre {
                left + natural * 0.5
            } else {
                right - natural * 0.5
            };
            if natural <= right - left && (rect.x..=rect.x + rect.width).contains(&moved) {
                (moved, natural)
            } else {
                (centre, rect.width.max(2.0 * half))
            }
        };
        f(TextItem {
            kind: TextKind::Caption(index),
            content: fit_label(&widget.label, width, LABEL_SIZE),
            anchor: Point::new(anchor, y),
            align: Align::Center,
            size: LABEL_SIZE,
            width,
        });
        if let WidgetViewKind::Segmented { options, .. } = &widget.kind {
            // Each name is centred in its segment, and cut to fit it.
            for (i, option) in options.iter().enumerate() {
                let segment = crate::controls::segment_rect(rect, i, options.len());
                let width = segment.width - 4.0;
                f(TextItem {
                    kind: TextKind::Option(index, i),
                    content: fit_label(option, width, LABEL_SIZE),
                    anchor: segment.center(),
                    align: Align::Center,
                    size: LABEL_SIZE,
                    width,
                });
            }
        }
        if let WidgetViewKind::Radio { options } = &widget.kind {
            // Option text runs from just right of its button to the
            // widget's right edge. It is cut with "…" like a caption, never
            // wrapped (#394): a word split over two lines is hard to read,
            // and the second line runs into the next option.
            let option_width = rect.width - 12.0;
            for (i, option) in options.iter().enumerate() {
                let centre = radio_centre(rect, i);
                f(TextItem {
                    kind: TextKind::Option(index, i),
                    content: fit_label(option, option_width, LABEL_SIZE),
                    anchor: Point::new(centre.x + 6.0, centre.y),
                    align: Align::Left,
                    size: LABEL_SIZE,
                    width: option_width,
                });
            }
        }
    }
}

/// Something a panel draws, as text beside it sees it.
struct PanelBox {
    /// The widget it belongs to, by index.
    owner: usize,
    /// Its box in world units.
    bounds: Rectangle,
    /// True for text: a caption or a radio option, at the width it has
    /// before any label grows. Other text shares the gap between them;
    /// drawn parts keep [`GAP`] clear.
    text: bool,
}

/// What every widget on a module's panel draws: its control or display,
/// its caption, and its radio options.
fn panel_boxes(module: &ModuleView) -> Vec<PanelBox> {
    let mut boxes = Vec::new();
    for (owner, widget) in module.panel.widgets.iter().enumerate() {
        let Some(rect) = module.widget_bounds(owner) else {
            continue;
        };
        let drawn = match &widget.kind {
            WidgetViewKind::Knob { .. } => knob_drawn(rect),
            // The buttons, in a column below the caption row.
            WidgetViewKind::Radio { .. } => {
                let right = radio_centre(rect, 0).x + 5.0;
                Rectangle::new(
                    Point::new(rect.x, rect.y + GRID),
                    Size::new(right - rect.x, rect.height - GRID),
                )
            }
            // The segments, in the row below the caption.
            WidgetViewKind::Segmented { .. } => Rectangle::new(
                Point::new(rect.x, rect.y + GRID),
                Size::new(rect.width, rect.height - GRID),
            ),
            _ => rect,
        };
        boxes.push(PanelBox {
            owner,
            bounds: drawn,
            text: false,
        });
        let caption = text_width(&widget.label, LABEL_SIZE).min(rect.width);
        boxes.push(PanelBox {
            owner,
            bounds: line_box(rect.center_x() - caption * 0.5, caption, caption_y(&widget.kind, rect)),
            text: true,
        });
        if let WidgetViewKind::Radio { options } = &widget.kind {
            for (i, option) in options.iter().enumerate() {
                let centre = radio_centre(rect, i);
                let width = text_width(option, LABEL_SIZE).min(rect.width - 12.0);
                boxes.push(PanelBox {
                    owner,
                    bounds: line_box(centre.x + 6.0, width, centre.y),
                    text: true,
                });
            }
        }
    }
    boxes
}

/// Space kept between a label that grew and what is beside it.
const GAP: f32 = 3.0;

/// Where a widget's caption sits, as the vertical centre of its text.
fn caption_y(kind: &WidgetViewKind, rect: Rectangle) -> f32 {
    match kind {
        WidgetViewKind::Knob { .. } => knob_centre(rect).y + 1.5 * GRID,
        WidgetViewKind::Meter | WidgetViewKind::Scope => rect.y + rect.height - GRID * 0.5,
        WidgetViewKind::Shape { .. } => rect.y + rect.height - GRID * 0.5,
        WidgetViewKind::Toggle | WidgetViewKind::Light => light_centre(rect).y + GRID,
        // On the face of the button.
        WidgetViewKind::Button => rect.center_y(),
        WidgetViewKind::Radio { .. } => rect.y + GRID * 0.5,
        WidgetViewKind::Select { .. } | WidgetViewKind::Segmented { .. } => rect.y + GRID * 0.5,
    }
}

/// The box of one line of label text from `x`, `width` wide, centred on
/// `y`. As tall as [`TextItem::bounds`] makes it.
fn line_box(x: f32, width: f32, y: f32) -> Rectangle {
    let height = LABEL_SIZE * 1.2;
    Rectangle::new(Point::new(x, y - height * 0.5), Size::new(width, height))
}

/// True when `b` reaches more than [`TOUCH`] into the line of text
/// `line`. The line's box is 1.2 times the font size, taller than its
/// letters, so a shape that only touches its edge does not meet the text.
fn overlaps_line(b: Rectangle, line: Rectangle) -> bool {
    b.y < line.y + line.height - TOUCH && b.y + b.height > line.y + TOUCH
}

/// How far a shape may reach into a line of text's box without meeting its
/// letters, in world units.
const TOUCH: f32 = 1.0;

/// The port whose label is under a world point. Text is drawn above every
/// tile, so a label can show over a tile that overlaps its own. Every tile
/// under the point is searched, topmost first.
pub fn port_label_at(scene: &crate::scene::Scene, point: Point) -> Option<PortRef> {
    scene
        .modules()
        .iter()
        .rev()
        .filter(|module| module.bounds().contains(point))
        .find_map(|module| {
            let mut found = None;
            for_each(module, |item| {
                if let TextKind::Port(port) = item.kind
                    && item.bounds().contains(point)
                {
                    found = Some(port);
                }
            });
            found
        })
}

#[cfg(test)]
mod tests;
