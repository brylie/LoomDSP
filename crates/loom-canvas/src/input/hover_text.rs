//! What the pointer is over, and the hover text drawn for it.

use super::*;

impl CanvasProps<'_> {
    /// Updates hover text after an event (panels.md, Labels and hover
    /// text). It asks for a frame only when the text appears or hides, and
    /// for a single timed frame while it waits, so it never draws while
    /// nothing changes.
    pub(super) fn sync_hover_text(
        &self,
        state: &mut CanvasInput,
        event: Option<&Event>,
        now: Instant,
        out: &mut Reaction,
    ) {
        let redrawing = matches!(event, Some(Event::Redraw));
        let target = self.hover_target(state);
        let mut changed = state.hover_text.rest(target, now);
        // This frame is being drawn already, so text due now needs no other.
        changed |= state.hover_text.tick(now) && !redrawing;
        if changed {
            out.redraw();
        }
        if let Some(due) = state.hover_text.due() {
            out.redraw_at(due);
        }
    }

    /// What hover text would be about now: the port or control under the
    /// pointer, or else the control focused with Tab. Right after a Tab
    /// press, the focused control comes first, until the pointer moves.
    /// Nothing during a drag.
    pub(super) fn hover_target(&self, state: &CanvasInput) -> Option<HoverTarget> {
        if state.focus.has_overlay() || !matches!(state.interaction, Interaction::Idle) {
            return None;
        }
        let focused = state.focus.tab_control().and_then(|(module, widget)| {
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
                // Each edge of an XY pad has its parameter's hover text.
                let second = state
                    .cursor
                    .is_some_and(|world| self.drop_part(module, widget, world) == ControlPart::Second);
                Some(if second {
                    HoverTarget::Second(module, widget)
                } else {
                    HoverTarget::Control(module, widget)
                })
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
    pub(super) fn pin(&self, state: &CanvasInput) -> Option<PortRef> {
        state
            .pinned
            .filter(|p| self.scene.badge_bounds(p.module, p.index).is_some())
    }

    /// The cursor for the pointer resting over the canvas with no gesture.
    /// The ring of a modulated knob reaches a little past the knob's cell,
    /// so it is checked first, as `press` does: there a press drags depth,
    /// and the cursor says so (modulation.md, Arc drag).
    pub(super) fn hover_interaction(&self, state: &CanvasInput) -> CursorShape {
        if state.cursor.is_some_and(|point| self.resize_at(point).is_some()) {
            return CursorShape::ResizeDiagonal;
        }
        let on_range = state
            .hover
            .zip(state.cursor)
            .and_then(|(hit, world)| self.depth_drag_at(&hit, world));
        if let Some((module, widget, part)) = on_range {
            if matches!(self.widget_kind(module, widget), Some(WidgetViewKind::XyPad { .. })) {
                return drag_cursor(&ValueDrag::xy_axis(
                    self.scene
                        .module(module)
                        .and_then(|m| m.widget_bounds(widget))
                        .map_or(Rect::default(), xy_area),
                    part,
                    state.camera.zoom,
                ));
            }
            return drag_cursor(&self.value_drag(module, widget, state.camera.zoom));
        }
        match state.hover {
            Some(Hit::Port(_) | Hit::Badge(_)) => CursorShape::Pointer,
            Some(Hit::Widget { module, widget }) => match self.widget_kind(module, widget) {
                Some(WidgetViewKind::XyPad { .. }) => CursorShape::Crosshair,
                Some(WidgetViewKind::Range { .. }) => drag_cursor(&self.value_drag(module, widget, state.camera.zoom)),
                Some(kind) if kind.is_one_number() => drag_cursor(&self.value_drag(module, widget, state.camera.zoom)),
                _ => CursorShape::Pointer,
            },
            Some(Hit::Module(_)) => CursorShape::Grab,
            _ => CursorShape::default(),
        }
    }

    /// The module tile under the cursor, if any (not counting its ports).
    pub(super) fn hovered_module(state: &CanvasInput) -> Option<u32> {
        match state.hover? {
            Hit::Module(id) => Some(id),
            _ => None,
        }
    }

    /// The port under the cursor, if any.
    pub(super) fn hovered_port(state: &CanvasInput) -> Option<PortRef> {
        match state.hover? {
            Hit::Port(port) => Some(port),
            _ => None,
        }
    }

    /// The cable under the cursor, if any.
    pub(super) fn hovered_cable(state: &CanvasInput) -> Option<usize> {
        match state.hover? {
            Hit::Cable(index) => Some(index),
            _ => None,
        }
    }

    /// The mapping of one value of a widget that binds two, in learn mode.
    fn part_label(&self, module: u32, widget: usize, part: ControlPart) -> Option<&str> {
        self.learn?.part_labels.get(&(module, widget, part)).map(String::as_str)
    }

    /// The label, hover text, and world bounds of what hover text is about.
    /// The label, hover text, value line, and anchor of a hover target. Only
    /// lights and meters have a value line: their current value, which the
    /// app writes as the widget's text (port-and-parameter-text.md, Display
    /// values). A control's value shows on the control itself.
    pub(crate) fn hover_content(&self, target: HoverTarget) -> Option<(&str, &HoverText, Option<&str>, Rect)> {
        match target {
            HoverTarget::Port(port) => {
                let view = self.scene.port(port)?;
                let at = self.scene.port_position(port)?;
                let marker = Rect::new(
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
                    Some(badge) => marker.union(badge),
                    None => marker,
                };
                Some((&view.label, &view.hover, None, anchor))
            }
            HoverTarget::Second(id, widget) => {
                let module = self.scene.module(id)?;
                let view = module.panel.widgets.get(widget)?;
                let part = &view.kind.parts()?[1];
                let value = self.part_label(id, widget, ControlPart::Second);
                Some((&part.label, &part.hover, value, module.widget_bounds(widget)?))
            }
            HoverTarget::Control(id, widget) => {
                let module = self.scene.module(id)?;
                let view = module.panel.widgets.get(widget)?;
                if let Some(parts) = view.kind.parts() {
                    let value = self.part_label(id, widget, ControlPart::Main);
                    return Some((&parts[0].label, &parts[0].hover, value, module.widget_bounds(widget)?));
                }
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
                let header = Rect::new(tile.position(), Size::new(tile.width, HEADER));
                Some((&module.title, &module.hover, None, header))
            }
        }
    }
}
