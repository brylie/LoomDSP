//! Names for what is on a tile: `MODULE:NAME` as `--pointer`, `--center`,
//! and the control channel's `locate`, `screenshot`, and pointer commands
//! take it (`docs/design/control-channel.md`, rule 10).
//!
//! A name is looked up in a fixed order: a control by its parameter, an
//! output, an input, then a light or a meter by its display value. A
//! script reaches a display that shares its name with something earlier
//! with the `display:` prefix: the Level Meter's `11:out` is its output,
//! and `11:display:out` is its meter. `badge:OUTPUT` names an output's
//! assignment count badge, such as `5:badge:out`.

use loom_canvas::{PORT_HIT_RADIUS, PortRef, Scene};
use loom_core::{Graph, ModuleId};

/// The prefix that names a light or a meter only.
pub const DISPLAY_PREFIX: &str = "display:";
/// The prefix that names an output's assignment count badge.
pub const BADGE_PREFIX: &str = "badge:";

/// What a `MODULE:NAME` names on a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// A control on the panel, by widget index, bound to a parameter.
    Control(u32, usize),
    /// A port.
    Port(PortRef),
    /// A light or a meter on the panel, by widget index.
    Display(u32, usize),
    /// The assignment count badge of an output.
    Badge(PortRef),
}

/// Finds `name` on module `module`, in the order of rule 10: parameter,
/// output, input, then display value. `display:NAME` finds a display
/// value only.
pub fn find(graph: &Graph, module: u32, name: &str) -> Result<Part, String> {
    let node = graph
        .node(ModuleId(module))
        .ok_or_else(|| format!("no module {module}"))?;
    let descriptor = node.descriptor;
    let widgets = descriptor.panel.widgets;
    let display = |name: &str| {
        widgets
            .iter()
            .position(|w| !w.is_control() && w.bind == name)
            .map(|index| Part::Display(module, index))
    };
    if let Some(name) = name.strip_prefix(DISPLAY_PREFIX) {
        return display(name)
            .ok_or_else(|| format!("module {module} ({}) has no light or meter `{name}`", descriptor.name));
    }
    if let Some(name) = name.strip_prefix(BADGE_PREFIX) {
        return descriptor
            .outputs
            .iter()
            .position(|p| p.name == name)
            .map(|index| {
                Part::Badge(PortRef {
                    module,
                    output: true,
                    index,
                })
            })
            .ok_or_else(|| format!("module {module} ({}) has no output `{name}`", descriptor.name));
    }
    let port = |ports: &[loom_core::PortSpec], output| {
        ports
            .iter()
            .position(|p| p.name == name)
            .map(|index| Part::Port(PortRef { module, output, index }))
    };
    widgets
        .iter()
        .position(|w| w.is_control() && w.bind == name)
        .map(|index| Part::Control(module, index))
        .or_else(|| port(descriptor.outputs, true))
        .or_else(|| port(descriptor.inputs, false))
        .or_else(|| display(name))
        .ok_or_else(|| {
            format!(
                "module {module} ({}) has no control, port, light, or meter `{name}`",
                descriptor.name
            )
        })
}

/// The name that finds `part` again: `MODULE:NAME`, or
/// `MODULE:display:NAME` for a display whose name is taken by a control or
/// a port first. `None` when the module or port is gone.
pub fn name(graph: &Graph, part: Part) -> Option<String> {
    let (module, plain) = match part {
        Part::Control(module, widget) | Part::Display(module, widget) => (
            module,
            graph.node(ModuleId(module))?.descriptor.panel.widgets.get(widget)?.bind,
        ),
        Part::Port(port) | Part::Badge(port) => {
            let descriptor = graph.node(ModuleId(port.module))?.descriptor;
            let ports = if port.output {
                descriptor.outputs
            } else {
                descriptor.inputs
            };
            (port.module, ports.get(port.index)?.name)
        }
    };
    Some(match part {
        Part::Badge(_) => format!("{module}:{BADGE_PREFIX}{plain}"),
        Part::Display(..) if find(graph, module, plain) != Ok(part) => format!("{module}:{DISPLAY_PREFIX}{plain}"),
        _ => format!("{module}:{plain}"),
    })
}

/// Where a part is drawn, in world units: a widget's rectangle, the
/// square a port is hit in, or a badge. An error when it is not drawn.
pub fn world_bounds(scene: &Scene, part: Part) -> Result<iced::Rectangle, String> {
    let not_drawn = || "it is not on the canvas".to_string();
    match part {
        Part::Control(module, widget) | Part::Display(module, widget) => scene
            .module(module)
            .and_then(|m| m.widget_bounds(widget))
            .ok_or_else(not_drawn),
        Part::Badge(port) => scene
            .module(port.module)
            .and_then(|m| m.badge_bounds(port.index))
            .ok_or_else(|| "the output shows no badge: it drives no assignment".to_string()),
        Part::Port(port) => {
            let centre = scene.port_position(port).ok_or_else(not_drawn)?;
            let r = PORT_HIT_RADIUS;
            Ok(iced::Rectangle::new(
                iced::Point::new(centre.x - r, centre.y - r),
                iced::Size::new(2.0 * r, 2.0 * r),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A patch with an envelope, whose light is `level`, and a Level Meter
    /// on its output, whose meter shares the name `out` with the output.
    fn patch() -> (crate::App, u32, u32) {
        let mut app = crate::App::new(crate::tests::app_args());
        let env = app.controller.add_module("loom.env.adsr").unwrap();
        let meter = app.controller.add_module("loom.display.meter").unwrap();
        app.controller.connect_named(env, "out", meter, "in").unwrap();
        app.commit();
        (app, env.0, meter.0)
    }

    /// Rule 10: a parameter, then an output, then an input, then a display
    /// value. A light is found by its display value, and a meter that
    /// shares a port's name by `display:`.
    #[test]
    fn names_find_parameters_ports_then_lights_and_meters() {
        let (app, env, meter) = patch();
        let graph = app.controller.graph();
        assert!(matches!(find(graph, env, "attack"), Ok(Part::Control(..))));
        assert!(matches!(
            find(graph, env, "gate"),
            Ok(Part::Port(PortRef { output: false, .. }))
        ));
        let light = find(graph, env, "level").unwrap();
        assert!(matches!(light, Part::Display(..)));
        assert_eq!(name(graph, light), Some(format!("{env}:level")));

        let out = find(graph, meter, "out").unwrap();
        assert!(matches!(out, Part::Port(PortRef { output: true, .. })), "{out:?}");
        let display = find(graph, meter, "display:out").unwrap();
        assert!(matches!(display, Part::Display(..)));
        assert_eq!(name(graph, display), Some(format!("{meter}:display:out")));
        assert_eq!(name(graph, out), Some(format!("{meter}:out")));

        let error = find(graph, env, "nonsense").unwrap_err();
        assert!(
            error.contains("no control, port, light, or meter `nonsense`"),
            "{error}"
        );
        let error = find(graph, env, "display:attack").unwrap_err();
        assert!(error.contains("no light or meter `attack`"), "{error}");

        let badge = find(graph, env, "badge:out").unwrap();
        assert!(matches!(badge, Part::Badge(PortRef { output: true, .. })));
        assert_eq!(name(graph, badge), Some(format!("{env}:badge:out")));
        // No assignment, so no badge is drawn.
        assert!(world_bounds(&app.scene, badge).unwrap_err().contains("no badge"));
        assert!(find(graph, env, "badge:gate").is_err(), "inputs have no badge");
    }
}
