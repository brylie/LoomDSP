//! Drawing the canvas's text through Iced: tile text, the choice pop-up, the
//! typed field, and hover text. Text is measured with Iced here, so none of
//! this is in `crate::input`.

use super::*;

impl CanvasProps<'_> {
    pub(super) fn draw_text<Renderer>(
        &self,
        state: &CanvasInput,
        values: &PanelValues,
        renderer: &mut Renderer,
        bounds: Rect,
    ) where
        Renderer: text::Renderer<Font = Font>,
    {
        let zoom = state.camera().zoom;
        // Quantise sizes so zooming does not create a new text layout every
        // frame for every label.
        let quantise = |size: f32| (size * 2.0).round() / 2.0;
        let title_px = quantise(TITLE_SIZE * zoom);
        if title_px < TITLE_MIN_PX {
            return;
        }
        let label_px = quantise(LABEL_SIZE * zoom);
        let show_labels = label_px >= LABEL_MIN_PX;
        let visible = state.camera().visible(bounds.size());
        let font = renderer.default_font();
        let to_screen = |world: Point| {
            let s = state.camera().to_screen(world);
            Point::new(bounds.x + s.x, bounds.y + s.y)
        };
        let text = |content: &str, size: f32, width: f32, align: text::Alignment| Text {
            content: content.to_string(),
            bounds: iced::Size::new(width, size * 1.4),
            size: Pixels(size),
            line_height: text::LineHeight::Relative(1.2),
            font,
            align_x: align,
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
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
                        let align = match item.align {
                            Align::Left => text::Alignment::Left,
                            Align::Center => text::Alignment::Center,
                            Align::Right => text::Alignment::Right,
                        };
                        renderer.fill_text(
                            text(&shown, label_px, item.width * zoom, align),
                            to_screen(item.anchor).to_iced(),
                            self.palette.text.to_iced(),
                            clip.to_iced(),
                        );
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
                let align = match item.align {
                    Align::Left => text::Alignment::Left,
                    Align::Center => text::Alignment::Center,
                    Align::Right => text::Alignment::Right,
                };
                renderer.fill_text(
                    text(value.unwrap_or(&item.content), size, item.width * zoom, align),
                    to_screen(item.anchor).to_iced(),
                    if value.is_some() { self.palette.text } else { colour }.to_iced(),
                    clip.to_iced(),
                );
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
                    renderer.fill_text(
                        text(&label, badge_px, rect.width * zoom, text::Alignment::Center),
                        to_screen(rect.center()).to_iced(),
                        self.palette.background.to_iced(),
                        bounds.to_iced(),
                    );
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

    /// Draws the open choice list above all tile text, at a fixed screen size.
    pub(super) fn draw_select_popup<Renderer>(&self, state: &CanvasInput, renderer: &mut Renderer, bounds: Rect)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        let (Some(popup), Some((module, widget))) = (state.popup().copied(), state.focused_control()) else {
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
        let font = renderer.default_font();
        renderer.with_layer(bounds.to_iced(), |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: area.to_iced(),
                    border: iced::Border {
                        color: self.palette.tile_border.to_iced(),
                        width: 1.0,
                        radius: 5.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                self.palette.control_body.to_iced(),
            );
            let rows = (rect.height / SELECT_ROW_PX).round() as usize;
            for (index, option) in options.iter().enumerate().skip(popup.scroll).take(rows) {
                let row = (index - popup.scroll) as f32;
                if index == popup.highlight {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rect::new(
                                Point::new(at.x + 2.0, at.y + SELECT_ROW_PX * row + 1.0),
                                Size::new(rect.width - 4.0, SELECT_ROW_PX - 2.0),
                            )
                            .to_iced(),
                            border: iced::Border {
                                radius: 3.0.into(),
                                ..iced::Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        self.palette.control_track.to_iced(),
                    );
                }
                let chosen = self.position(module, widget).round().max(0.0) as usize == index;
                let label = if chosen {
                    format!("✓  {option}")
                } else {
                    format!("    {option}")
                };
                renderer.fill_text(
                    Text {
                        content: label,
                        bounds: iced::Size::new(rect.width - 12.0, 17.0),
                        size: Pixels(12.0),
                        line_height: text::LineHeight::Relative(1.2),
                        font,
                        align_x: text::Alignment::Left,
                        align_y: alignment::Vertical::Center,
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(at.x + 7.0, at.y + SELECT_ROW_PX * (row + 0.5)).to_iced(),
                    self.palette.text.to_iced(),
                    bounds.to_iced(),
                );
            }
            // A longer list has a bar at its right edge. Its length and place
            // show how much is out of view (rule 8: a shape, not a colour).
            if rows < options.len() {
                let share = rows as f32 / options.len() as f32;
                let start = popup.scroll as f32 / options.len() as f32;
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rect::new(
                            Point::new(at.x + rect.width - 5.0, at.y + 2.0 + (rect.height - 4.0) * start),
                            Size::new(3.0, (rect.height - 4.0) * share),
                        )
                        .to_iced(),
                        border: iced::Border {
                            radius: 1.5.into(),
                            ..iced::Border::default()
                        },
                        ..renderer::Quad::default()
                    },
                    self.palette.text_dim.to_iced(),
                );
            }
        });
    }

    /// Draws the open field above all tile text. Bad text gets a warning
    /// outline, and its reason on a line below (rule 8: not colour alone).
    pub(super) fn draw_typed_field<Renderer>(&self, state: &CanvasInput, renderer: &mut Renderer, bounds: Rect)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        let Some(field) = state.typed_field() else {
            return;
        };
        let Some(rect) = self.typed_field_bounds(state, Rect::new(Point::ORIGIN, bounds.size())) else {
            return;
        };
        let area = Rect::new(Point::new(bounds.x + rect.x, bounds.y + rect.y), rect.size());
        let font = renderer.default_font();
        let error = field.draft.error().map(str::to_string);
        let line = |content: String, width: f32| Text {
            content,
            bounds: iced::Size::new(width, FIELD_HEIGHT_PX),
            size: Pixels(FIELD_TEXT_PX),
            line_height: text::LineHeight::Relative(1.2),
            font,
            align_x: text::Alignment::Left,
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        };
        let inner = FIELD_WIDTH_PX - 2.0 * FIELD_PADDING_PX;
        let width_of = |content: &str| {
            Renderer::Paragraph::with_text(Text {
                content,
                bounds: iced::Size::new(f32::INFINITY, FIELD_HEIGHT_PX),
                size: Pixels(FIELD_TEXT_PX),
                line_height: text::LineHeight::Relative(1.2),
                font,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Center,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::None,
            })
            .min_bounds()
            .width
        };
        let text_at = Point::new(area.x + FIELD_PADDING_PX, area.center_y());
        renderer.with_layer(bounds.to_iced(), |renderer| {
            let outline = if error.is_some() {
                (self.palette.warning, 2.0)
            } else {
                (self.palette.control_value, 1.0)
            };
            renderer.fill_quad(
                renderer::Quad {
                    bounds: area.to_iced(),
                    border: iced::Border {
                        color: outline.0.to_iced(),
                        width: outline.1,
                        radius: 4.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                self.palette.control_body.to_iced(),
            );
            if field.selected && !field.draft.text.is_empty() {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rect::new(
                            Point::new(text_at.x - 1.0, area.y + 3.0),
                            Size::new(width_of(&field.draft.text).min(inner) + 2.0, FIELD_HEIGHT_PX - 6.0),
                        )
                        .to_iced(),
                        ..renderer::Quad::default()
                    },
                    self.palette.control_track.to_iced(),
                );
            }
            renderer.fill_text(
                line(field.draft.text.clone(), inner),
                text_at.to_iced(),
                self.palette.text.to_iced(),
                area.to_iced(),
            );
            if !field.selected {
                let x = text_at.x + width_of(&field.draft.text[..field.caret]).min(inner);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rect::new(Point::new(x, area.y + 4.0), Size::new(1.0, FIELD_HEIGHT_PX - 8.0)).to_iced(),
                        ..renderer::Quad::default()
                    },
                    self.palette.text.to_iced(),
                );
            }
            if let Some(reason) = error {
                let place = typed_reason_bounds(area, width_of(&reason) + 2.0 * FIELD_PADDING_PX, bounds);
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: place.to_iced(),
                        border: iced::Border {
                            color: self.palette.tile_border.to_iced(),
                            width: 1.0,
                            radius: 4.0.into(),
                        },
                        ..renderer::Quad::default()
                    },
                    self.palette.control_body.to_iced(),
                );
                renderer.fill_text(
                    line(reason, place.width),
                    Point::new(place.x + FIELD_PADDING_PX, place.center_y()).to_iced(),
                    self.palette.text.to_iced(),
                    bounds.to_iced(),
                );
            }
        });
    }

    /// Hover text (panels.md, Labels and hover text): the label, the
    /// description, and the name when it differs, at a fixed screen size in
    /// a layer above the tiles. Drawn at any zoom, because hover text is
    /// what the user needs where tile text is too small to read.
    pub(super) fn draw_hover_text<Renderer>(&self, state: &CanvasInput, renderer: &mut Renderer, bounds: Rect)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        if state.has_overlay() {
            return;
        }
        let Some(target) = state.hover_text().shown() else {
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
        let font = renderer.default_font();
        let inner = crate::hover::text_width(bounds.width);
        // Each line is measured as a paragraph, then drawn as text in that
        // size, wrapped at word boundaries.
        let line = |content: &str, size: f32, colour: Color, font: Font| {
            let text = Text {
                content: content.to_string(),
                bounds: iced::Size::new(inner, f32::INFINITY),
                size: Pixels(size),
                line_height: text::LineHeight::Relative(1.3),
                font,
                align_x: text::Alignment::Left,
                align_y: alignment::Vertical::Top,
                shaping: text::Shaping::Basic,
                wrapping: text::Wrapping::Word,
            };
            let measured = Renderer::Paragraph::with_text(text.as_ref()).min_bounds();
            (
                Text {
                    bounds: iced::Size::new(measured.width.ceil() + 1.0, measured.height.ceil()),
                    ..text
                },
                colour,
            )
        };
        // A light's or meter's label is bold, so it stands apart from its
        // value line (port-and-parameter-text.md, Display values).
        let bold = Font {
            weight: iced::font::Weight::Bold,
            ..font
        };
        let title = if value.is_some() { bold } else { font };
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
        let width = lines.iter().map(|(t, _)| t.bounds.width).fold(0.0, f32::max);
        let height = lines.iter().map(|(t, _)| t.bounds.height).sum::<f32>() + LINE_GAP * (lines.len() - 1) as f32;
        let size = Size::new(width + 2.0 * HOVER_PADDING, height + 2.0 * HOVER_PADDING);

        let zoom = state.camera().zoom;
        let corner = state.camera().to_screen(world.position());
        let anchor = Rect::new(
            Point::new(bounds.x + corner.x, bounds.y + corner.y),
            world.size() * zoom,
        );
        let at = crate::hover::place(size, anchor, bounds);
        renderer.with_layer(bounds.to_iced(), |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rect::new(at, size).to_iced(),
                    border: iced::Border {
                        color: self.palette.tile_border.to_iced(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                self.palette.tile.to_iced(),
            );
            let mut y = at.y + HOVER_PADDING;
            for (text, colour) in lines {
                let height = text.bounds.height;
                renderer.fill_text(
                    text,
                    Point::new(at.x + HOVER_PADDING, y).to_iced(),
                    colour.to_iced(),
                    bounds.to_iced(),
                );
                y += height + LINE_GAP;
            }
        });
    }
}
