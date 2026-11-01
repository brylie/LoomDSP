//! Names for what is on a tile: `MODULE:NAME` as `--pointer`, `--center`,
//! and the control channel's `locate`, `screenshot`, and pointer commands
//! take it (`docs/design/control-channel.md`, rule 10).
//!
//! A name is looked up in a fixed order: a control by its parameter, an
//! output, an input, then a light or a meter by its display value. A
//! script reaches a display that shares its name with something earlier
//! with the `display:` prefix: the Level Meter's `11:out` is its output,
//! and `11:display:out` is its meter. In the same way `input:NAME` and
//! `output:NAME` name a port whose name a parameter or an output also has:
//! the SVF's `4:cutoff` is its knob, and `4:input:cutoff` its input.
//! `badge:OUTPUT` names an output's assignment count badge, such as
//! `5:badge:out`.

use loom_canvas::{PORT_HIT_RADIUS, PortRef, Scene};
use loom_core::{Graph, ModuleId};

/// The prefix that names a light or a meter only.
pub const DISPLAY_PREFIX: &str = "display:";
/// The prefix that names an output's assignment count badge.
pub const BADGE_PREFIX: &str = "badge:";
/// The prefix that names an input only.
pub const INPUT_PREFIX: &str = "input:";
/// The prefix that names an output only.
pub const OUTPUT_PREFIX: &str = "output:";

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
/// value only, `input:NAME` an input, `output:NAME` an output, and
/// `badge:NAME` an output's badge.
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
        return (0..node.output_count())
            .find(|&index| node.output_name(index) == Some(name))
            .map(|index| {
                Part::Badge(PortRef {
                    module,
                    output: true,
                    index,
                })
            })
            .ok_or_else(|| format!("module {module} ({}) has no output `{name}`", descriptor.name));
    }
    let port = |name: &str, output: bool| {
        let count = if output {
            node.output_count()
        } else {
            node.input_count()
        };
        (0..count)
            .find(|&index| if output { node.output_name(index) } else { node.input_name(index) } == Some(name))
            .map(|index| Part::Port(PortRef { module, output, index }))
    };
    for (prefix, output) in [(INPUT_PREFIX, false), (OUTPUT_PREFIX, true)] {
        if let Some(name) = name.strip_prefix(prefix) {
            let kind = if output { "output" } else { "input" };
            return port(name, output)
                .ok_or_else(|| format!("module {module} ({}) has no {kind} `{name}`", descriptor.name));
        }
    }
    widgets
        .iter()
        .position(|w| w.is_control() && w.bind == name)
        .map(|index| Part::Control(module, index))
        .or_else(|| port(name, true))
        .or_else(|| port(name, false))
        .or_else(|| display(name))
        .ok_or_else(|| {
            format!(
                "module {module} ({}) has no control, port, light, or meter `{name}`",
                descriptor.name
            )
        })
}

/// The name that finds `part` again: `MODULE:NAME` when that finds it, or
/// else the name with the prefix of its kind (`display:`, `input:`,
/// `output:`), for a part whose name something earlier in rule 10 also
/// has. A badge always has `badge:`. `None` when the module or the part is
/// gone.
pub fn name(graph: &Graph, part: Part) -> Option<String> {
    let (module, plain) = match part {
        Part::Control(module, widget) | Part::Display(module, widget) => (
            module,
            graph.node(ModuleId(module))?.descriptor.panel.widgets.get(widget)?.bind,
        ),
        Part::Port(port) | Part::Badge(port) => {
            let node = graph.node(ModuleId(port.module))?;
            (
                port.module,
                if port.output {
                    node.output_name(port.index)?
                } else {
                    node.input_name(port.index)?
                },
            )
        }
    };
    let prefix = match part {
        Part::Badge(_) => return Some(format!("{module}:{BADGE_PREFIX}{plain}")),
        _ if find(graph, module, plain) == Ok(part) => "",
        Part::Display(..) => DISPLAY_PREFIX,
        Part::Port(port) if port.output => OUTPUT_PREFIX,
        Part::Port(_) => INPUT_PREFIX,
        // A control comes first in rule 10. Only a second control bound to
        // the same parameter is not found by its name.
        Part::Control(..) => return None,
    };
    Some(format!("{module}:{prefix}{plain}"))
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
            .badge_bounds(port.module, port.index)
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
mod tests;
