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
//! `5:badge:out`. Displays that show the same thing are counted: the Level
//! Meter's `11:display:out` is its meter, and `11:display:out:2` its level
//! history.

use loom_canvas::{ControlPart, PORT_HIT_RADIUS, PanelValues, PortRef, Scene};
use loom_core::ModuleNames;
use loom_core::{Graph, ModuleId};
use loom_patch::{Hierarchy, Tile};

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
    /// The second parameter of an XY pad or a range, by widget index.
    Second(u32, usize),
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
/// `badge:NAME` an output's badge. On a sub-patch tile a parameter is an
/// exposed name, and the ports are the tile's; a port tile has its one
/// port.
pub fn find(graph: &Graph, hierarchy: &Hierarchy, module: u32, name: &str) -> Result<Part, String> {
    let Some(node) = graph.node(ModuleId(module)) else {
        return find_on_tile(graph, hierarchy, module, name);
    };
    let descriptor = node.descriptor;
    let widgets = descriptor.panel.widgets;
    let display = |name: &str| {
        widgets
            .iter()
            .position(|w| !w.is_control() && w.binds(name))
            .map(|index| Part::Display(module, index))
    };
    if let Some(name) = name.strip_prefix(DISPLAY_PREFIX) {
        // The second display on one name is `NAME:2`.
        let counted = name.rsplit_once(':').and_then(|(plain, count)| {
            let nth = count.parse::<usize>().ok()?.checked_sub(1)?;
            let index = (0..widgets.len())
                .filter(|&i| !widgets[i].is_control() && widgets[i].binds(plain))
                .nth(nth)?;
            Some(Part::Display(module, index))
        });
        return counted
            .or_else(|| display(name))
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
        .position(|w| w.is_control() && w.bind() == name)
        .map(|index| Part::Control(module, index))
        .or_else(|| {
            let second = widgets.iter().position(|w| w.second() == Some(name))?;
            Some(Part::Second(module, second))
        })
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

/// Finds `name` on a tile that is not a graph module: a sub-patch tile's
/// exposed parameter or port, or a port tile's one port, in the order of
/// rule 10.
fn find_on_tile(graph: &Graph, hierarchy: &Hierarchy, module: u32, name: &str) -> Result<Part, String> {
    let id = ModuleId(module);
    let tile = hierarchy.tile(graph, id).ok_or_else(|| format!("no module {module}"))?;
    let tile_name = hierarchy.tile_name(id).unwrap_or_default();
    let port = |name: &str, output: bool| tile_port(graph, hierarchy, module, name, output);
    for (prefix, output) in [(INPUT_PREFIX, false), (OUTPUT_PREFIX, true)] {
        if let Some(name) = name.strip_prefix(prefix) {
            let kind = if output { "output" } else { "input" };
            return port(name, output).ok_or_else(|| format!("module {module} ({tile_name}) has no {kind} `{name}`"));
        }
    }
    if name.starts_with(DISPLAY_PREFIX) || name.starts_with(BADGE_PREFIX) {
        return Err(format!(
            "module {module} ({tile_name}) has no light, meter, or badge `{name}`"
        ));
    }
    let control = match tile {
        Tile::Instance(_) | Tile::Slot(_) => {
            crate::surface::find_control(graph, hierarchy, module, name).map(|i| Part::Control(module, i))
        }
        _ => None,
    };
    control
        .or_else(|| port(name, true))
        .or_else(|| port(name, false))
        .ok_or_else(|| format!("module {module} ({tile_name}) has no control or port `{name}`"))
}

/// The port named `name` facing `output` on a tile that is not a graph
/// module: one of a sub-patch tile's ports, or a port tile's one port.
fn tile_port(graph: &Graph, hierarchy: &Hierarchy, module: u32, name: &str, output: bool) -> Option<Part> {
    let id = ModuleId(module);
    let index = match hierarchy.tile(graph, id)? {
        Tile::Instance(_) => {
            let (inputs, outputs) = hierarchy.instance_ports(id);
            let list = if output { outputs } else { inputs };
            list.iter().position(|(_, port)| port == name)?
        }
        Tile::Port(direction, _) => {
            let faces_out = direction == loom_patch::Direction::Input;
            (faces_out == output && direction.own_port() == name).then_some(0)?
        }
        Tile::Slot(_) => {
            let state = hierarchy.slot(id)?;
            let list = if output { state.role.outputs } else { state.role.inputs };
            list.iter().position(|port| port.name == name)?
        }
        Tile::Module => return None,
    };
    Some(Part::Port(PortRef { module, output, index }))
}

/// The name of a port on a tile, by its place: a module's port, a sub-patch
/// tile's, or a port tile's one port.
fn port_name(graph: &Graph, hierarchy: &Hierarchy, port: PortRef) -> Option<String> {
    let id = ModuleId(port.module);
    if let Some(node) = graph.node(id) {
        return Some(if port.output {
            node.output_name(port.index)?.to_string()
        } else {
            node.input_name(port.index)?.to_string()
        });
    }
    match hierarchy.tile(graph, id)? {
        Tile::Instance(_) => {
            let (inputs, outputs) = hierarchy.instance_ports(id);
            let list = if port.output { outputs } else { inputs };
            Some(list.get(port.index)?.1.clone())
        }
        Tile::Port(direction, _) => {
            let faces_out = direction == loom_patch::Direction::Input;
            (faces_out == port.output && port.index == 0).then(|| direction.own_port().to_string())
        }
        Tile::Slot(_) => {
            let state = hierarchy.slot(id)?;
            let list = if port.output {
                state.role.outputs
            } else {
                state.role.inputs
            };
            Some(list.get(port.index)?.name.to_string())
        }
        Tile::Module => None,
    }
}

/// The name that finds `part` again: `MODULE:NAME` when that finds it, or
/// else the name with the prefix of its kind (`display:`, `input:`,
/// `output:`), for a part whose name something earlier in rule 10 also
/// has. A badge always has `badge:`. `None` when the module or the part is
/// gone.
pub fn name(graph: &Graph, hierarchy: &Hierarchy, part: Part) -> Option<String> {
    let plain: String = match part {
        Part::Control(module, widget) | Part::Display(module, widget) => {
            crate::surface::widget_name(graph, hierarchy, module, widget)?
        }
        Part::Second(module, widget) => graph
            .node(ModuleId(module))?
            .descriptor
            .panel
            .widgets
            .get(widget)?
            .second()?
            .to_string(),
        Part::Port(port) | Part::Badge(port) => port_name(graph, hierarchy, port)?,
    };
    let plain = plain.as_str();
    let module = match part {
        Part::Control(module, _) | Part::Second(module, _) | Part::Display(module, _) => module,
        Part::Port(port) | Part::Badge(port) => port.module,
    };
    if let Part::Display(_, widget) = part {
        let widgets = graph.node(ModuleId(module))?.descriptor.panel.widgets;
        let earlier = widgets[..widget]
            .iter()
            .filter(|w| !w.is_control() && w.binds(plain))
            .count();
        if earlier > 0 {
            return Some(format!("{module}:{DISPLAY_PREFIX}{plain}:{}", earlier + 1));
        }
    }
    let prefix = match part {
        Part::Badge(_) => return Some(format!("{module}:{BADGE_PREFIX}{plain}")),
        _ if find(graph, hierarchy, module, plain) == Ok(part) => "",
        Part::Display(..) => DISPLAY_PREFIX,
        Part::Port(port) if port.output => OUTPUT_PREFIX,
        Part::Port(_) => INPUT_PREFIX,
        // A control comes first in rule 10. Only a second control bound to
        // the same parameter is not found by its name.
        Part::Control(..) | Part::Second(..) => return None,
    };
    Some(format!("{module}:{prefix}{plain}"))
}

/// Where a part is drawn, in world units: a widget's rectangle, the
/// square a port is hit in, or a badge. An error when it is not drawn.
///
/// A name on an XY pad or a range aims at that value: inside the pad, at the
/// middle of that value's edge, or at the handle (`docs/design/control-channel.md`,
/// rule 10).
pub fn world_bounds(scene: &Scene, values: &PanelValues, part: Part) -> Result<loom_ui_core::Rect, String> {
    let not_drawn = || "it is not on the canvas".to_string();
    if let Part::Control(module, widget) | Part::Second(module, widget) = part
        && let Some(aim) = scene.module(module).and_then(|m| {
            let rect = m.widget_bounds(widget)?;
            let control = if matches!(part, Part::Second(..)) {
                ControlPart::Second
            } else {
                ControlPart::Main
            };
            loom_canvas::part_bounds(
                &m.panel.widgets.get(widget)?.kind,
                rect,
                values.part_position(module, widget, control),
                control,
            )
        })
    {
        return Ok(aim);
    }
    match part {
        Part::Control(module, widget) | Part::Second(module, widget) | Part::Display(module, widget) => scene
            .module(module)
            .and_then(|m| m.widget_bounds(widget))
            .ok_or_else(not_drawn),
        Part::Badge(port) => scene
            .badge_bounds(port.module, port.index)
            .ok_or_else(|| "the output shows no badge: it drives no assignment".to_string()),
        Part::Port(port) => {
            let centre = scene.port_position(port).ok_or_else(not_drawn)?;
            let r = PORT_HIT_RADIUS;
            Ok(loom_ui_core::Rect::new(
                loom_ui_core::Point::new(centre.x - r, centre.y - r),
                loom_ui_core::Size::new(2.0 * r, 2.0 * r),
            ))
        }
    }
}

#[cfg(test)]
mod tests;
