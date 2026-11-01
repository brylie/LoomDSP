//! Drawing on top of the GPU scene: the overlay, cable highlights, panel text, and choice pop-ups.

use super::*;

impl<Message> PatchCanvas<'_, Message> {
    /// The overlay for a view whose visible world area is `visible`. Learn
    /// mode outlines only the controls there, so its cost follows what is on
    /// screen, not the size of the patch.
    pub(super) fn overlay_in(&self, state: &State, visible: Rectangle) -> Overlay {
        let mut overlay = self.overlay(state);
        let p = &self.palette;
        // MIDI learn mode outlines every control on screen, and the armed
        // one more strongly, so the user sees what a click will do.
        if let Some(learn) = self.learn {
            let visible_modules = self
                .scene
                .modules()
                .iter()
                .filter(|module| module.bounds().intersects(&visible));
            for module in visible_modules {
                for (index, view) in module.panel.widgets.iter().enumerate() {
                    if !view.is_control() {
                        continue;
                    }
                    let Some(rect) = module.widget_bounds(index) else {
                        continue;
                    };
                    let (ring, radius) = focus_ring(&view.kind, rect);
                    let (colour, width) = if learn.armed == Some((module.id, index)) {
                        (p.selection, 2.5)
                    } else {
                        (p.focus, 1.0)
                    };
                    overlay
                        .shapes
                        .push(Shape::new(ring, radius, Color::TRANSPARENT, colour, width));
                }
            }
        }
        overlay
    }

    pub(super) fn overlay(&self, state: &State) -> Overlay {
        let mut overlay = Overlay::default();
        let p = &self.palette;

        for tile in self.scene.modules().iter().filter(|tile| tile.is_growable()) {
            let bounds = tile.bounds();
            let grip = Rectangle::new(
                Point::new(
                    bounds.x + bounds.width - GRID * 0.65,
                    bounds.y + bounds.height - GRID * 0.65,
                ),
                Size::new(GRID * 0.4, GRID * 0.4),
            );
            overlay
                .shapes
                .push(Shape::new(grip, 1.0, p.focus, Color::TRANSPARENT, 0.0));
        }

        // Live port markers use the overlay buffer. A state change uploads
        // only these shapes and never rebuilds tiles or cables (#63).
        if detail(state.camera.zoom) == Detail::Close {
            let mut drawn = std::collections::HashSet::new();
            for cable in self.scene.cables() {
                if !self.activity.contains(&cable.from) {
                    continue;
                }
                for port in [
                    PortRef {
                        module: cable.from.0,
                        output: true,
                        index: cable.from.1,
                    },
                    PortRef {
                        module: cable.to.0,
                        output: false,
                        index: cable.to.1,
                    },
                ] {
                    if !drawn.insert(port) {
                        continue;
                    }
                    if let (Some(centre), Some(view)) = (self.scene.port_position(port), self.scene.port(port)) {
                        let r = PORT_RADIUS - 1.0;
                        let rect = Rectangle::new(Point::new(centre.x - r, centre.y - r), Size::new(2.0 * r, 2.0 * r));
                        let corner = if view.shape == crate::scene::PortShape::Round {
                            r
                        } else {
                            r * 0.3
                        };
                        overlay
                            .shapes
                            .push(Shape::new(rect, corner, view.colour, Color::TRANSPARENT, 0.0));
                    }
                }
            }
        }

        // Cables in focus come to the front at full opacity: the cable under
        // the cursor, the cables on the port under the cursor, and the cables
        // of the module under the cursor and of selected modules. Everything
        // else stays at the same dim level, so the patch never flickers.
        let hovered = Self::hovered_module(state);
        let hovered_port = Self::hovered_port(state);
        let hovered_cable = Self::hovered_cable(state);
        let module_in_focus = |id: u32| self.selection.contains(&id) || hovered == Some(id);
        let on_port = |cable: &crate::scene::CableView| {
            hovered_port.is_some_and(|p| {
                if p.output {
                    cable.from == (p.module, p.index)
                } else {
                    cable.to == (p.module, p.index)
                }
            })
        };
        if !self.selection.is_empty() || hovered.is_some() || hovered_port.is_some() || hovered_cable.is_some() {
            for (index, cable) in self.scene.cables().iter().enumerate() {
                let focused = hovered_cable == Some(index)
                    || on_port(cable)
                    || module_in_focus(cable.from.0)
                    || module_in_focus(cable.to.0);
                if !focused {
                    continue;
                }
                let from = PortRef {
                    module: cable.from.0,
                    output: true,
                    index: cable.from.1,
                };
                let to = PortRef {
                    module: cable.to.0,
                    output: false,
                    index: cable.to.1,
                };
                if let (Some(a), Some(b)) = (self.scene.port_position(from), self.scene.port_position(to)) {
                    overlay
                        .cables
                        .push(Cable::new(a, b, cable.colour_from, cable.colour_to, cable.delayed).focused());
                }
            }
        }

        self.modulation_lines(state, &mut overlay);

        for id in self.selection {
            if let Some(module) = self.scene.module(*id) {
                let b = module.bounds().expand(3.0);
                overlay
                    .shapes
                    .push(Shape::new(b, 7.0, Color::TRANSPARENT, p.selection, 1.5));
            }
        }
        if let Some(Hit::Port(port)) = state.hover
            && let Some(centre) = self.scene.port_position(port)
        {
            let r = PORT_RADIUS + 3.0;
            let rect = Rectangle::new(Point::new(centre.x - r, centre.y - r), Size::new(2.0 * r, 2.0 * r));
            overlay
                .shapes
                .push(Shape::new(rect, r, Color::TRANSPARENT, p.hover, 1.2));
        }
        // The focus ring shows for keyboard focus only. After a click or a
        // drag the pointer already shows where the user is, and the ring
        // would only crowd the value text (#92).
        if let Some((module, widget)) = state.focus
            && state.tab_focus
            && self.selection.contains(&module)
            && let Some(m) = self.scene.module(module)
            && let (Some(rect), Some(view)) = (m.widget_bounds(widget), m.panel.widgets.get(widget))
        {
            let (ring, radius) = focus_ring(&view.kind, rect);
            overlay
                .shapes
                .push(Shape::new(ring, radius, Color::TRANSPARENT, p.focus, 1.5));
        }
        match &state.interaction {
            Interaction::Wiring { from, pulled } => {
                // A control that would take this output as an assignment is
                // highlighted, like an input port for a cable.
                if let Some(Hit::Widget { module, widget }) = state.hover
                    && !pulled
                    && self.takes_assignment(*from, module, widget)
                    && let Some(m) = self.scene.module(module)
                    && let (Some(rect), Some(view)) = (m.widget_bounds(widget), m.panel.widgets.get(widget))
                {
                    let (ring, radius) = focus_ring(&view.kind, rect);
                    overlay
                        .shapes
                        .push(Shape::new(ring, radius, Color::TRANSPARENT, p.hover, 1.5));
                }
                if !(self.window_drag_line && from.output && !pulled)
                    && let (Some(start), Some(port), Some(cursor)) =
                        (self.scene.port_position(*from), self.scene.port(*from), state.cursor)
                {
                    // Snap the loose end onto a port under the cursor.
                    let end = match state.hover {
                        Some(Hit::Port(target)) if target.output != from.output => {
                            self.scene.port_position(target).unwrap_or(cursor)
                        }
                        _ => cursor,
                    };
                    let cable = if from.output {
                        Cable::new(start, end, port.colour, port.colour, false)
                    } else {
                        Cable::new(end, start, port.colour, port.colour, false)
                    }
                    .focused();
                    overlay.cables.push(cable);
                }
            }
            Interaction::Selecting { start, current, .. } => {
                let rect = Rectangle::new(
                    Point::new(start.x.min(current.x), start.y.min(current.y)),
                    Size::new((start.x - current.x).abs(), (start.y - current.y).abs()),
                );
                overlay
                    .shapes
                    .push(Shape::new(rect, 2.0, p.marquee, p.selection, 1.0 / state.camera.zoom));
            }
            Interaction::Resizing { module, current, .. } => {
                if let Some(tile) = self.scene.module(*module) {
                    overlay.shapes.push(Shape::new(
                        Rectangle::new(tile.position, *current),
                        2.0,
                        Color::TRANSPARENT,
                        p.selection,
                        1.0 / state.camera.zoom,
                    ));
                }
            }
            _ => {}
        }
        overlay
    }

    /// Modulation lines in focus (modulation.md, Lines): thin dashed lines
    /// from a source port to its targets, for the pinned output, the output
    /// or badge under the pointer, the control under the pointer, and the
    /// modules in focus as for cables. Pointed and pinned sources get a
    /// double outline; their targets get a single one. A pinned badge gets a ring.
    pub(super) fn modulation_lines(&self, state: &State, overlay: &mut Overlay) {
        let key = LinesKey {
            scene: self.scene.generation(),
            overlay: self.scene.overlay_generation(),
            pinned: self.pin(state),
            hover: state.hover,
            selection: self.selection.iter().copied().collect(),
            matrix_line: self.matrix_line.clone(),
        };
        let mut cache = state.lines.borrow_mut();
        if cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut lines = Overlay::default();
            self.build_modulation_lines(state, &mut lines);
            *cache = Some((key, lines));
        }
        if let Some((_, lines)) = cache.as_ref() {
            overlay.cables.extend_from_slice(&lines.cables);
            overlay.shapes.extend_from_slice(&lines.shapes);
        }
    }

    pub(super) fn build_modulation_lines(&self, state: &State, overlay: &mut Overlay) {
        let p = &self.palette;
        if let Some(port) = self.pin(state)
            && let Some(badge) = self.scene.badge_bounds(port.module, port.index)
        {
            let ring = badge.expand(1.5);
            overlay
                .shapes
                .push(Shape::new(ring, ring.height * 0.5, Color::TRANSPARENT, p.text, 1.0));
        }
        let pointed = match state.hover {
            Some(Hit::Port(port) | Hit::Badge(port)) if port.output => Some(port),
            _ => None,
        };
        let sources: Vec<(u32, usize)> = [self.pin(state), pointed]
            .into_iter()
            .flatten()
            .map(|port| (port.module, port.index))
            .collect();
        let control = match state.hover {
            Some(Hit::Widget { module, widget }) => Some((module, widget)),
            _ => None,
        };
        let hovered = Self::hovered_module(state);
        let module_in_focus = |id: u32| self.selection.contains(&id) || hovered == Some(id);
        let mut targets: Vec<u32> = Vec::new();
        let mut sources_outlined: Vec<u32> = Vec::new();
        for m in self.scene.modulations() {
            let by_source = sources.contains(&m.from);
            let by_control = control.is_some_and(|(module, widget)| m.to == module && m.widget == Some(widget));
            if !(self.matrix_line.as_ref() == Some(m)
                || by_source
                || by_control
                || module_in_focus(m.from.0)
                || module_in_focus(m.to))
            {
                continue;
            }
            let from = PortRef {
                module: m.from.0,
                output: true,
                index: m.from.1,
            };
            let (Some(start), Some(end)) = (self.scene.port_position(from), self.scene.modulation_end(m)) else {
                continue;
            };
            let colour = if m.enabled {
                p.modulation
            } else {
                Color {
                    a: p.modulation.a * 0.4,
                    ..p.modulation
                }
            };
            let mut line = Cable::new(start, end, colour, colour, true).thin().focused();
            if !m.enabled {
                line = line.half_width();
            }
            overlay.cables.push(line);
            if by_source || by_control {
                if !targets.contains(&m.to) {
                    targets.push(m.to);
                }
                if !sources_outlined.contains(&m.from.0) {
                    sources_outlined.push(m.from.0);
                }
            }
        }
        for id in targets.into_iter().filter(|id| !sources_outlined.contains(id)) {
            if let Some(module) = self.scene.module(id) {
                overlay.shapes.push(Shape::new(
                    module.bounds().expand(3.0),
                    7.0,
                    Color::TRANSPARENT,
                    p.modulation,
                    1.2,
                ));
            }
        }
        for id in sources_outlined {
            if let Some(module) = self.scene.module(id) {
                for distance in [3.0, 6.0] {
                    overlay.shapes.push(Shape::new(
                        module.bounds().expand(distance),
                        7.0 + distance - 3.0,
                        Color::TRANSPARENT,
                        p.modulation,
                        1.2,
                    ));
                }
            }
        }
    }

    pub(super) fn draw_text<Renderer>(
        &self,
        state: &State,
        values: &PanelValues,
        renderer: &mut Renderer,
        bounds: Rectangle,
    ) where
        Renderer: text::Renderer<Font = Font>,
    {
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
        let font = renderer.default_font();
        let to_screen = |world: Point| {
            let s = state.camera.to_screen(world);
            Point::new(bounds.x + s.x, bounds.y + s.y)
        };
        let text = |content: &str, size: f32, width: f32, align: text::Alignment| Text {
            content: content.to_string(),
            bounds: Size::new(width, size * 1.4),
            size: Pixels(size),
            line_height: text::LineHeight::Relative(1.2),
            font,
            align_x: align,
            align_y: alignment::Vertical::Center,
            shaping: text::Shaping::Basic,
            wrapping: text::Wrapping::None,
        };
        // The control whose value shows instead of its label.
        let active = match state.interaction {
            Interaction::Turning { module, widget, .. } => Some((module, widget)),
            _ => state.focus,
        };

        for module in self.scene.modules() {
            let tile = module.bounds();
            if !tile.intersects(&visible) {
                continue;
            }
            let screen_tile = Rectangle::new(to_screen(tile.position()), tile.size() * zoom);
            let Some(clip) = screen_tile.intersection(&bounds) else {
                continue;
            };
            tile_text::for_each(module, |item| {
                let (size, colour) = match item.kind {
                    TextKind::Title => (title_px, self.palette.text),
                    _ if !show_labels => return,
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
                let learned = match item.kind {
                    TextKind::Caption(widget) => self
                        .learn
                        .and_then(|learn| learn.labels.get(&(module.id, widget)))
                        .map(String::as_str),
                    _ => None,
                };
                let value = match item.kind {
                    _ if learned.is_some() => learned,
                    TextKind::Caption(widget) if active == Some((module.id, widget)) => values
                        .get(module.id, widget)
                        .filter(|_| matches!(module.panel.widgets[widget].kind, WidgetViewKind::Knob { .. }))
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
                    to_screen(item.anchor),
                    if value.is_some() { self.palette.text } else { colour },
                    clip,
                );
            });
            if show_labels {
                for (index, widget) in module.panel.widgets.iter().enumerate() {
                    let WidgetViewKind::Select { options } = &widget.kind else {
                        continue;
                    };
                    let Some(rect) = module.widget_bounds(index) else {
                        continue;
                    };
                    let box_rect = select_box(rect);
                    let chosen = values.position(module.id, index).round().max(0.0) as usize;
                    // Cut to the box with "…", never wrapped; the open list
                    // shows the whole name. The text starts 7 units in, and
                    // the arrow takes the last 11.
                    let room = box_rect.width - 18.0;
                    let choice = options.get(chosen).map_or("", String::as_str);
                    let choice = crate::scene::fit_label(choice, room, crate::tile_text::LABEL_SIZE);
                    renderer.fill_text(
                        text(&choice, label_px, room * zoom, text::Alignment::Left),
                        to_screen(Point::new(box_rect.x + 7.0, box_rect.center_y())),
                        self.palette.text,
                        clip,
                    );
                }
            }
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
            if !module.bounds().expand(BADGE_REACH).intersects(&visible) {
                continue;
            }
            for (index, port) in module.outputs.iter().enumerate() {
                if let Some(rect) = self
                    .scene
                    .badge_bounds(module.id, index)
                    .filter(|r| r.intersects(&visible))
                {
                    let label = port.badge.to_string();
                    renderer.fill_text(
                        text(&label, badge_px, rect.width * zoom, text::Alignment::Center),
                        to_screen(rect.center()),
                        self.palette.background,
                        bounds,
                    );
                }
            }
        }
    }

    /// Draws the open choice list above all tile text, at a fixed screen size.
    pub(super) fn draw_select_popup<Renderer>(&self, state: &State, renderer: &mut Renderer, bounds: Rectangle)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        let Some(popup) = state.select_popup else {
            return;
        };
        let Some(rect) = self.select_popup_bounds(state, Rectangle::new(Point::ORIGIN, bounds.size())) else {
            return;
        };
        let Some(WidgetViewKind::Select { options }) = self.widget_kind(popup.module, popup.widget) else {
            return;
        };
        let at = Point::new(bounds.x + rect.x, bounds.y + rect.y);
        let area = Rectangle::new(at, rect.size());
        let font = renderer.default_font();
        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: area,
                    border: iced::Border {
                        color: self.palette.tile_border,
                        width: 1.0,
                        radius: 5.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                self.palette.control_body,
            );
            for (index, option) in options.iter().enumerate() {
                if index == popup.highlight {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle::new(
                                Point::new(at.x + 2.0, at.y + SELECT_ROW_PX * index as f32 + 1.0),
                                Size::new(rect.width - 4.0, SELECT_ROW_PX - 2.0),
                            ),
                            border: iced::Border {
                                radius: 3.0.into(),
                                ..iced::Border::default()
                            },
                            ..renderer::Quad::default()
                        },
                        self.palette.control_track,
                    );
                }
                let chosen = self.position(popup.module, popup.widget).round().max(0.0) as usize == index;
                let label = if chosen {
                    format!("✓  {option}")
                } else {
                    format!("    {option}")
                };
                renderer.fill_text(
                    Text {
                        content: label,
                        bounds: Size::new(rect.width - 12.0, 17.0),
                        size: Pixels(12.0),
                        line_height: text::LineHeight::Relative(1.2),
                        font,
                        align_x: text::Alignment::Left,
                        align_y: alignment::Vertical::Center,
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::None,
                    },
                    Point::new(at.x + 7.0, at.y + SELECT_ROW_PX * (index as f32 + 0.5)),
                    self.palette.text,
                    bounds,
                );
            }
        });
    }
}
