//! What the canvas paints above its GPU shapes: tile text, the choice list,
//! the typed field, and hover text. Each piece goes to a [`Painter`], and
//! text is sized by a [`TextMeasure`], so nothing here names a text
//! renderer.
//!
//! This is drawing, in a folder named for input. It is here because it
//! reads [`CanvasInput`]'s private state: the focus, the open list or
//! field, and the hover timer. `CanvasInput` now holds all of the canvas's
//! lasting state and also paints, so its name and this folder's no longer
//! fit. They are settled when the canvas becomes a custom element
//! (docs/design/ui-layer.md, section 9), not here.

use super::*;
use crate::controls::PanelValues;
use crate::hover::{HOVER_PADDING, HOVER_TEXT_PX};
use crate::painter::{Painter, Quad, Shaping, TextMeasure, TextRun, TextStyle, Vertical, Weight};
use crate::scene::BADGE_TEXT_SIZE;
use crate::tile_text::Align;

/// The style of one line of plain interface text.
fn plain(size: f32, line_height: f32) -> TextStyle {
    TextStyle {
        size,
        line_height,
        weight: Weight::Regular,
        shaping: Shaping::Simple,
    }
}

/// One line of tile text, centred top to bottom on its anchor.
fn one_line(text: &str, size: f32, width: f32, align: Align, anchor: Point, colour: Color, clip: Rect) -> TextRun<'_> {
    TextRun {
        text,
        anchor,
        bounds: Size::new(width, size * 1.4),
        align,
        vertical: Vertical::Center,
        style: plain(size, 1.2),
        wrap: false,
        colour,
        clip,
    }
}

impl CanvasInput {
    /// Paints everything above the GPU shapes, back to front: tile text,
    /// then the open choice list, the open typed field, and hover text,
    /// each in its own layer. `bounds` is the canvas's place in the host's
    /// logical pixels. Call it after the renderer has drawn the frame.
    pub fn paint(&self, model: &CanvasModel<'_>, bounds: Rect, measure: &dyn TextMeasure, painter: &mut dyn Painter) {
        let empty = PanelValues::default();
        let values = model.values.unwrap_or(&empty);
        model.paint_tile_text(self, values, bounds, painter);
        model.paint_select_popup(self, bounds, painter);
        model.paint_typed_field(self, bounds, measure, painter);
        model.paint_hover_text(self, bounds, measure, painter);
    }
}

impl CanvasModel<'_> {
    /// Paints the text on the tiles and in the badges, at the size the
    /// zoom gives it. Text too small to read is left out (panels.md, Level
    /// of detail).
    fn paint_tile_text(&self, state: &CanvasInput, values: &PanelValues, bounds: Rect, painter: &mut dyn Painter) {
        let zoom = state.camera.zoom;
        // Quantise sizes so zooming does not create a new text layout every
        // frame for every label.
        let quantise = |size: f32| (size * 2.0).round() / 2.0;
        let title_px = quantise(TITLE_SIZE * zoom);
        if title_px < TITLE_MIN_PX {
            return;
        }
        let label_px = quantise(LABEL_SIZE * zoom);
        let show_labels = label_px >= LABEL_MIN_PX;
        let visible = state.camera.visible(bounds.size());
        let to_screen = |world: Point| {
            let s = state.camera.to_screen(world);
            Point::new(bounds.x + s.x, bounds.y + s.y)
        };
        // The control whose value shows instead of its label.
        let active = state.value_shown();

        for module in self.scene.modules() {
            let tile = module.bounds();
            if !tile.intersects(visible) {
                continue;
            }
            let screen_tile = Rect::new(to_screen(tile.position()), tile.size() * zoom);
            let Some(clip) = screen_tile.intersection(bounds) else {
                continue;
            };
            tile_text::for_each(module, |item| {
                let (size, colour) = match item.kind {
                    TextKind::Title => (title_px, self.palette.text),
                    _ if !show_labels => return,
                    // A box's live text, cut with "…" and never wrapped. The
                    // bar at middle zoom stands in for it.
                    TextKind::Value(widget) => {
                        let shown = self.box_text(values, module, widget);
                        let shown = crate::scene::fit_label(shown, item.width, crate::tile_text::LABEL_SIZE);
                        painter.text(&one_line(
                            &shown,
                            label_px,
                            item.width * zoom,
                            item.align,
                            to_screen(item.anchor),
                            self.palette.text,
                            clip,
                        ));
                        return;
                    }
                    // The chosen segment is filled with the value colour,
                    // so its name is dark (visual-system.md, contrast).
                    TextKind::Option(widget, option)
                        if matches!(
                            module.panel.widgets.get(widget).map(|w| &w.kind),
                            Some(WidgetViewKind::Segmented { .. })
                        ) && self.position(module.id, widget).round() as usize == option =>
                    {
                        (label_px, self.palette.background)
                    }
                    TextKind::Option(..) => (label_px, self.palette.text),
                    // A button's label is on its face: light on the off
                    // face, dark on the orange face while on.
                    TextKind::Caption(widget)
                        if matches!(
                            module.panel.widgets.get(widget).map(|w| &w.kind),
                            Some(WidgetViewKind::Button)
                        ) =>
                    {
                        let on = self.position(module.id, widget) > 0.5;
                        (label_px, if on { self.palette.background } else { self.palette.text })
                    }
                    _ => (label_px, self.palette.text_dim),
                };
                // A focused or turning knob shows its value instead of its
                // caption.
                // In MIDI learn mode, a mapped control shows its mapping.
                let learned = learned_label(self.learn, module.id, item.kind);
                let value = match item.kind {
                    _ if learned.is_some() => learned,
                    TextKind::Caption(widget) if active == Some((module.id, widget)) => values
                        .get(module.id, widget)
                        .filter(|_| {
                            matches!(
                                module.panel.widgets[widget].kind,
                                WidgetViewKind::Knob { .. }
                                    | WidgetViewKind::Fader { .. }
                                    | WidgetViewKind::XyPad { .. }
                                    | WidgetViewKind::Range { .. }
                            )
                        })
                        .map(|v| v.text.as_str()),
                    _ => None,
                };
                painter.text(&one_line(
                    value.unwrap_or(&item.content),
                    size,
                    item.width * zoom,
                    item.align,
                    to_screen(item.anchor),
                    if value.is_some() { self.palette.text } else { colour },
                    clip,
                ));
            });
        }

        // Badge numbers usually sit outside their tile, so they are checked
        // against their own bounds, not the tile's, and clip to the canvas.
        // They show exactly when port labels do (#249): with its own
        // threshold, a badge could show as an empty pill beside labels.
        // Zoomed out further, the pill alone shows.
        let badge_px = quantise(BADGE_TEXT_SIZE * zoom);
        if !show_labels {
            return;
        }
        for module in self.scene.modules() {
            // A badge reaches past its tile's right edge by at most this
            // much: the gap after the port and a pill with several digits.
            const BADGE_REACH: f32 = 40.0;
            if !module.bounds().expand(BADGE_REACH).intersects(visible) {
                continue;
            }
            for (index, port) in module.outputs.iter().enumerate() {
                if let Some(rect) = self
                    .scene
                    .badge_bounds(module.id, index)
                    .filter(|r| r.intersects(visible))
                {
                    let label = port.badge.to_string();
                    painter.text(&one_line(
                        &label,
                        badge_px,
                        rect.width * zoom,
                        Align::Center,
                        to_screen(rect.center()),
                        self.palette.background,
                        bounds,
                    ));
                }
            }
        }
    }

    /// The text in a widget's box: a select list's choice, or the value text.
    fn box_text<'t>(&self, values: &'t PanelValues, module: &'t crate::scene::ModuleView, widget: usize) -> &'t str {
        match module.panel.widgets.get(widget).map(|w| &w.kind) {
            Some(WidgetViewKind::Select { options }) => {
                let chosen = values.position(module.id, widget).round().max(0.0) as usize;
                options.get(chosen).map_or("", String::as_str)
            }
            Some(WidgetViewKind::Meter { .. }) => values.get(module.id, widget).map_or("", |v| v.hold.as_str()),
            _ => values.get(module.id, widget).map_or("", |v| v.text.as_str()),
        }
    }

    /// Paints the open choice list above all tile text, at a fixed screen size.
    fn paint_select_popup(&self, state: &CanvasInput, bounds: Rect, painter: &mut dyn Painter) {
        let (Some(popup), Some((module, widget))) = (state.focus.popup().copied(), state.focus.control()) else {
            return;
        };
        let Some(rect) = self.select_popup_bounds(state, Rect::new(Point::ORIGIN, bounds.size())) else {
            return;
        };
        let Some(WidgetViewKind::Select { options }) = self.widget_kind(module, widget) else {
            return;
        };
        let at = Point::new(bounds.x + rect.x, bounds.y + rect.y);
        let area = Rect::new(at, rect.size());
        painter.begin_layer(bounds);
        {
            painter.quad(Quad {
                rect: area,
                fill: self.palette.control_body,
                border: self.palette.tile_border,
                border_width: 1.0,
                radius: 5.0,
            });
            let rows = (rect.height / SELECT_ROW_PX).round() as usize;
            for (index, option) in options.iter().enumerate().skip(popup.scroll).take(rows) {
                let row = (index - popup.scroll) as f32;
                if index == popup.highlight {
                    painter.quad(Quad::filled(
                        Rect::new(
                            Point::new(at.x + 2.0, at.y + SELECT_ROW_PX * row + 1.0),
                            Size::new(rect.width - 4.0, SELECT_ROW_PX - 2.0),
                        ),
                        self.palette.control_track,
                        3.0,
                    ));
                }
                let chosen = self.position(module, widget).round().max(0.0) as usize == index;
                let label = if chosen {
                    format!("✓  {option}")
                } else {
                    format!("    {option}")
                };
                painter.text(&TextRun {
                    text: &label,
                    anchor: Point::new(at.x + 7.0, at.y + SELECT_ROW_PX * (row + 0.5)),
                    bounds: Size::new(rect.width - 12.0, 17.0),
                    align: Align::Left,
                    vertical: Vertical::Center,
                    // The check mark is not in every interface font.
                    style: TextStyle {
                        shaping: Shaping::Full,
                        ..plain(12.0, 1.2)
                    },
                    wrap: false,
                    colour: self.palette.text,
                    clip: bounds,
                });
            }
            // A longer list has a bar at its right edge. Its length and place
            // show how much is out of view (rule 8: a shape, not a colour).
            if rows < options.len() {
                let share = rows as f32 / options.len() as f32;
                let start = popup.scroll as f32 / options.len() as f32;
                painter.quad(Quad::filled(
                    Rect::new(
                        Point::new(at.x + rect.width - 5.0, at.y + 2.0 + (rect.height - 4.0) * start),
                        Size::new(3.0, (rect.height - 4.0) * share),
                    ),
                    self.palette.text_dim,
                    1.5,
                ));
            }
        }
        painter.end_layer();
    }

    /// Paints the open field above all tile text. Bad text gets a warning
    /// outline, and its reason on a line below (rule 8: not colour alone).
    fn paint_typed_field(
        &self,
        state: &CanvasInput,
        bounds: Rect,
        measure: &dyn TextMeasure,
        painter: &mut dyn Painter,
    ) {
        let Some(field) = state.focus.typed() else {
            return;
        };
        let Some(rect) = self.typed_field_bounds(state, Rect::new(Point::ORIGIN, bounds.size())) else {
            return;
        };
        let area = Rect::new(Point::new(bounds.x + rect.x, bounds.y + rect.y), rect.size());
        let error = field.draft.error();
        let style = plain(FIELD_TEXT_PX, 1.2);
        // One line of field text, centred top to bottom on its anchor.
        let line = |text, width: f32, anchor: Point, clip: Rect| TextRun {
            text,
            anchor,
            bounds: Size::new(width, FIELD_HEIGHT_PX),
            align: Align::Left,
            vertical: Vertical::Center,
            style,
            wrap: false,
            colour: self.palette.text,
            clip,
        };
        let inner = FIELD_WIDTH_PX - 2.0 * FIELD_PADDING_PX;
        let width_of = |content: &str| measure.measure(content, style, None).width;
        let text_at = Point::new(area.x + FIELD_PADDING_PX, area.center_y());
        painter.begin_layer(bounds);
        {
            let outline = if error.is_some() {
                (self.palette.warning, 2.0)
            } else {
                (self.palette.control_value, 1.0)
            };
            painter.quad(Quad {
                rect: area,
                fill: self.palette.control_body,
                border: outline.0,
                border_width: outline.1,
                radius: 4.0,
            });
            if field.selected && !field.draft.text.is_empty() {
                painter.quad(Quad::filled(
                    Rect::new(
                        Point::new(text_at.x - 1.0, area.y + 3.0),
                        Size::new(width_of(&field.draft.text).min(inner) + 2.0, FIELD_HEIGHT_PX - 6.0),
                    ),
                    self.palette.control_track,
                    0.0,
                ));
            }
            painter.text(&line(&field.draft.text, inner, text_at, area));
            if !field.selected {
                let x = text_at.x + width_of(&field.draft.text[..field.caret]).min(inner);
                painter.quad(Quad::filled(
                    Rect::new(Point::new(x, area.y + 4.0), Size::new(1.0, FIELD_HEIGHT_PX - 8.0)),
                    self.palette.text,
                    0.0,
                ));
            }
            if let Some(reason) = error {
                let place = typed_reason_bounds(area, width_of(reason) + 2.0 * FIELD_PADDING_PX, bounds);
                painter.quad(Quad {
                    rect: place,
                    fill: self.palette.control_body,
                    border: self.palette.tile_border,
                    border_width: 1.0,
                    radius: 4.0,
                });
                painter.text(&line(
                    reason,
                    place.width,
                    Point::new(place.x + FIELD_PADDING_PX, place.center_y()),
                    bounds,
                ));
            }
        }
        painter.end_layer();
    }

    /// Hover text (panels.md, Labels and hover text): the label, the
    /// description, and the name when it differs, at a fixed screen size in
    /// a layer above the tiles. Drawn at any zoom, because hover text is
    /// what the user needs where tile text is too small to read.
    fn paint_hover_text(
        &self,
        state: &CanvasInput,
        bounds: Rect,
        measure: &dyn TextMeasure,
        painter: &mut dyn Painter,
    ) {
        if state.focus.has_overlay() {
            return;
        }
        let Some(target) = state.hover_text.shown() else {
            return;
        };
        let Some((label, hover, value, world)) = self.hover_content(target) else {
            return;
        };
        // A port with a clock mark says which transport signal it follows
        // (panels.md, Clock mark on transport inputs).
        let follows = match target {
            HoverTarget::Port(port) => self.scene.port(port).and_then(|p| p.follows_transport),
            HoverTarget::Control(..) | HoverTarget::Second(..) | HoverTarget::Title(_) => None,
        };
        let inner = crate::hover::text_width(bounds.width);
        // Each line is measured wrapped at word boundaries, then painted
        // in that size. The text, its style, its room, and its colour.
        let line = |content, size: f32, colour: Color, weight: Weight| {
            let style = TextStyle {
                weight,
                ..plain(size, 1.3)
            };
            let measured = measure.measure(content, style, Some(inner));
            (
                content,
                style,
                Size::new(measured.width.ceil() + 1.0, measured.height.ceil()),
                colour,
            )
        };
        // A light's or meter's label is bold, so it stands apart from its
        // value line (port-and-parameter-text.md, Display values).
        let font = Weight::Regular;
        let title = if value.is_some() { Weight::Bold } else { font };
        let mut lines = vec![line(label, HOVER_TEXT_PX, self.palette.text, title)];
        if !hover.description.is_empty() {
            lines.push(line(&hover.description, HOVER_TEXT_PX, self.palette.text_dim, font));
        }
        // What a control stands for across a sub-patch boundary
        // (sub-patches.md, Exposing).
        if let Some(note) = &hover.note {
            lines.push(line(note, HOVER_TEXT_PX, self.palette.text_dim, font));
        }
        if let Some(signal) = follows {
            lines.push(line(signal.hover_line(), HOVER_TEXT_PX, self.palette.text, font));
        }
        if let Some(value) = value {
            lines.push(line(value, HOVER_TEXT_PX, self.palette.text, font));
        }
        if let Some(name) = &hover.name {
            lines.push(line(name, HOVER_TEXT_PX - 1.0, self.palette.text_dim, font));
        }
        const LINE_GAP: f32 = 3.0;
        let width = lines.iter().map(|line| line.2.width).fold(0.0, f32::max);
        let height = lines.iter().map(|line| line.2.height).sum::<f32>() + LINE_GAP * (lines.len() - 1) as f32;
        let size = Size::new(width + 2.0 * HOVER_PADDING, height + 2.0 * HOVER_PADDING);

        let zoom = state.camera.zoom;
        let corner = state.camera.to_screen(world.position());
        let anchor = Rect::new(
            Point::new(bounds.x + corner.x, bounds.y + corner.y),
            world.size() * zoom,
        );
        let at = crate::hover::place(size, anchor, bounds);
        painter.begin_layer(bounds);
        painter.quad(Quad {
            rect: Rect::new(at, size),
            fill: self.palette.tile,
            border: self.palette.tile_border,
            border_width: 1.0,
            radius: 4.0,
        });
        let mut y = at.y + HOVER_PADDING;
        for (text, style, room, colour) in lines {
            painter.text(&TextRun {
                text,
                anchor: Point::new(at.x + HOVER_PADDING, y),
                bounds: room,
                align: Align::Left,
                vertical: Vertical::Top,
                style,
                wrap: true,
                colour,
                clip: bounds,
            });
            y += room.height + LINE_GAP;
        }
        painter.end_layer();
    }
}

#[cfg(test)]
mod tests;
