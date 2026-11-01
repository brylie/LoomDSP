//! What the pointer is over, and the hover text drawn for it.

use super::*;

impl<Message> PatchCanvas<'_, Message> {
    /// Updates hover text after an event (panels.md, Labels and hover
    /// text). It asks for a frame only when the text appears or hides, and
    /// for a single timed frame while it waits, so it never draws while
    /// nothing changes.
    pub(super) fn sync_hover_text(&self, state: &mut State, event: &Event, shell: &mut Shell<'_, Message>) {
        let (now, redrawing) = match event {
            Event::Window(window::Event::RedrawRequested(now)) => (*now, true),
            _ => (Instant::now(), false),
        };
        let target = self.hover_target(state);
        let mut changed = state.hover_text.rest(target, now);
        // This frame is being drawn already, so text due now needs no other.
        changed |= state.hover_text.tick(now) && !redrawing;
        if changed {
            shell.request_redraw();
        }
        if let Some(due) = state.hover_text.due() {
            shell.request_redraw_at(due);
        }
    }

    /// What hover text would be about now: the port or control under the
    /// pointer, or else the control focused with Tab. Right after a Tab
    /// press, the focused control comes first, until the pointer moves.
    /// Nothing during a drag.
    pub(super) fn hover_target(&self, state: &State) -> Option<HoverTarget> {
        if state.select_popup.is_some() || !matches!(state.interaction, Interaction::Idle) {
            return None;
        }
        let focused = state.focus.filter(|_| state.tab_focus).and_then(|(module, widget)| {
            let selected = self.selection.len() == 1 && self.selection.contains(&module);
            (selected && self.is_control(module, widget)).then_some(HoverTarget::Control(module, widget))
        });
        if state.focus_first && focused.is_some() {
            return focused;
        }
        match state.hover {
            // A port in hit range wins over a label under the pointer, even
            // another port's label. That port has the hover ring, and a
            // press starts a cable from it, so the hover text must name it
            // too.
            Some(Hit::Port(port)) => Some(HoverTarget::Port(port)),
            Some(Hit::Widget { module, widget }) if self.is_control(module, widget) => {
                Some(HoverTarget::Control(module, widget))
            }
            // A light or meter belongs to the tile for presses, so the hit
            // is the module. Its hover text still shows. A port's label, or
            // the bar standing in for it, hovers like the port (panels.md,
            // Hover text).
            Some(Hit::Module(id)) => state
                .cursor
                .and_then(|world| {
                    let display = self
                        .scene
                        .module(id)
                        .and_then(|m| m.display_at(world))
                        .filter(|&widget| self.has_hover_text(id, widget))
                        .map(|widget| HoverTarget::Control(id, widget));
                    // The header names the module's family (ADR-0029).
                    let title = self
                        .scene
                        .module(id)
                        .filter(|m| world.y < m.bounds().y + HEADER)
                        .map(|_| HoverTarget::Title(id));
                    // Text is drawn above every tile, so a port label that
                    // shows over this tile's header hovers before the title.
                    display
                        .or_else(|| tile_text::port_label_at(self.scene, world).map(HoverTarget::Port))
                        .or(title)
                })
                .or(focused),
            _ => focused,
        }
    }

    /// True for controls, and for lights and meters with a description
    /// (port-and-parameter-text.md, Display values).
    pub(super) fn has_hover_text(&self, module: u32, widget: usize) -> bool {
        self.scene
            .module(module)
            .and_then(|m| m.panel.widgets.get(widget))
            .is_some_and(|w| w.is_control() || !w.hover.description.is_empty())
    }

    /// The pinned output, while its badge shows. A pin on an output that
    /// lost its last assignment, or whose module is gone, counts as none:
    /// it draws nothing, so it must not take Escape or pick an arc drag.
    pub(super) fn pin(&self, state: &State) -> Option<PortRef> {
        state
            .pinned
            .filter(|p| self.scene.badge_bounds(p.module, p.index).is_some())
    }

    /// The cursor for the pointer resting over the canvas with no gesture.
    /// The ring of a modulated knob reaches a little past the knob's cell,
    /// so it is checked first, as `press` does: there a press drags depth,
    /// and the cursor says so (modulation.md, Arc drag).
    pub(super) fn hover_interaction(&self, state: &State) -> mouse::Interaction {
        if state.cursor.is_some_and(|point| self.resize_at(point).is_some()) {
            return mouse::Interaction::ResizingDiagonallyDown;
        }
        let on_ring = state
            .hover
            .zip(state.cursor)
            .is_some_and(|(hit, world)| self.depth_drag_at(&hit, world).is_some());
        if on_ring {
            return mouse::Interaction::ResizingVertically;
        }
        match state.hover {
            Some(Hit::Port(_) | Hit::Badge(_)) => mouse::Interaction::Pointer,
            Some(Hit::Widget { module, widget }) => match self.widget_kind(module, widget) {
                Some(WidgetViewKind::Knob { .. }) => mouse::Interaction::ResizingVertically,
                _ => mouse::Interaction::Pointer,
            },
            Some(Hit::Module(_)) => mouse::Interaction::Grab,
            _ => mouse::Interaction::default(),
        }
    }

    /// The module tile under the cursor, if any (not counting its ports).
    pub(super) fn hovered_module(state: &State) -> Option<u32> {
        match state.hover? {
            Hit::Module(id) => Some(id),
            _ => None,
        }
    }

    /// The port under the cursor, if any.
    pub(super) fn hovered_port(state: &State) -> Option<PortRef> {
        match state.hover? {
            Hit::Port(port) => Some(port),
            _ => None,
        }
    }

    /// The cable under the cursor, if any.
    pub(super) fn hovered_cable(state: &State) -> Option<usize> {
        match state.hover? {
            Hit::Cable(index) => Some(index),
            _ => None,
        }
    }

    /// The label, hover text, and world bounds of what hover text is about.
    /// The label, hover text, value line, and anchor of a hover target. Only
    /// lights and meters have a value line: their current value, which the
    /// app writes as the widget's text (port-and-parameter-text.md, Display
    /// values). A control's value shows on the control itself.
    pub(super) fn hover_content(&self, target: HoverTarget) -> Option<(&str, &HoverText, Option<&str>, Rectangle)> {
        match target {
            HoverTarget::Port(port) => {
                let view = self.scene.port(port)?;
                let at = self.scene.port_position(port)?;
                let marker = Rectangle::new(
                    Point::new(at.x - PORT_RADIUS, at.y - PORT_RADIUS),
                    Size::new(2.0 * PORT_RADIUS, 2.0 * PORT_RADIUS),
                );
                // An output's hover text goes above its badge too, so it
                // does not cover the count (#249).
                let anchor = match port
                    .output
                    .then(|| self.scene.badge_bounds(port.module, port.index))
                    .flatten()
                {
                    Some(badge) => marker.union(&badge),
                    None => marker,
                };
                Some((&view.label, &view.hover, None, anchor))
            }
            HoverTarget::Control(id, widget) => {
                let module = self.scene.module(id)?;
                let view = module.panel.widgets.get(widget)?;
                let value = (!view.is_control())
                    .then(|| self.values.and_then(|v| v.get(id, widget)))
                    .flatten()
                    .map(|v| v.text.as_str())
                    .filter(|text| !text.is_empty());
                Some((&view.label, &view.hover, value, module.widget_bounds(widget)?))
            }
            HoverTarget::Title(id) => {
                let module = self.scene.module(id)?;
                let tile = module.bounds();
                let header = Rectangle::new(tile.position(), Size::new(tile.width, HEADER));
                Some((&module.title, &module.hover, None, header))
            }
        }
    }

    /// Hover text (panels.md, Labels and hover text): the label, the
    /// description, and the name when it differs, at a fixed screen size in
    /// a layer above the tiles. Drawn at any zoom, because hover text is
    /// what the user needs where tile text is too small to read.
    pub(super) fn draw_hover_text<Renderer>(&self, state: &State, renderer: &mut Renderer, bounds: Rectangle)
    where
        Renderer: text::Renderer<Font = Font>,
    {
        if state.select_popup.is_some() {
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
            HoverTarget::Control(..) | HoverTarget::Title(_) => None,
        };
        let font = renderer.default_font();
        let inner = crate::hover::text_width(bounds.width);
        // Each line is measured as a paragraph, then drawn as text in that
        // size, wrapped at word boundaries.
        let line = |content: &str, size: f32, colour: Color, font: Font| {
            let text = Text {
                content: content.to_string(),
                bounds: Size::new(inner, f32::INFINITY),
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
                    bounds: Size::new(measured.width.ceil() + 1.0, measured.height.ceil()),
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

        let zoom = state.camera.zoom;
        let corner = state.camera.to_screen(world.position());
        let anchor = Rectangle::new(
            Point::new(bounds.x + corner.x, bounds.y + corner.y),
            world.size() * zoom,
        );
        let at = crate::hover::place(size, anchor, bounds);
        renderer.with_layer(bounds, |renderer| {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: Rectangle::new(at, size),
                    border: iced::Border {
                        color: self.palette.tile_border,
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..renderer::Quad::default()
                },
                self.palette.tile,
            );
            let mut y = at.y + HOVER_PADDING;
            for (text, colour) in lines {
                let height = text.bounds.height;
                renderer.fill_text(text, Point::new(at.x + HOVER_PADDING, y), colour, bounds);
                y += height + LINE_GAP;
            }
        });
    }
}
