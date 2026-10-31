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
mod tests {
    use super::*;

    /// A patch with an envelope, whose light is `level`, and a Level Meter
    /// on its output, whose meter shares the name `out` with the output.
    fn patch() -> (crate::App, u32, u32) {
        let mut app = crate::App::new(crate::tests::app_args());
        let env = app.controller.add_module("loom.env.envelope").unwrap();
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

    /// Every port, control, light, meter, and badge of every module has a
    /// name that finds it again, with a prefix only where the plain name
    /// finds something else: the SVF's `cutoff` input shares its name with
    /// the `cutoff` parameter, so it is `input:cutoff`.
    #[test]
    fn every_part_of_every_module_round_trips_through_its_name() {
        let mut app = crate::App::new(crate::tests::app_args());
        for descriptor in loom_modules::ALL {
            // A new patch has its one Transport already.
            let id = match app.controller.add_module(descriptor.id) {
                Err(loom_core::LoomError::SecondTransport(existing)) => existing.0,
                added => added.unwrap().0,
            };
            let graph = app.controller.graph();
            let mut parts = Vec::new();
            for (index, widget) in descriptor.panel.widgets.iter().enumerate() {
                parts.push(if widget.is_control() {
                    Part::Control(id, index)
                } else {
                    Part::Display(id, index)
                });
            }
            for (output, ports) in [(false, descriptor.inputs), (true, descriptor.outputs)] {
                for index in 0..ports.len() {
                    let port = PortRef {
                        module: id,
                        output,
                        index,
                    };
                    parts.push(Part::Port(port));
                    if output {
                        parts.push(Part::Badge(port));
                    }
                }
            }
            for part in parts {
                let text = name(graph, part).unwrap_or_else(|| panic!("{}: no name for {part:?}", descriptor.id));
                let (module, rest) = text.split_once(':').unwrap();
                assert_eq!(module.parse::<u32>().unwrap(), id);
                assert_eq!(find(graph, id, rest), Ok(part), "{}: {text}", descriptor.id);
            }
        }
    }

    /// The prefixes, and what the lookup refuses.
    #[test]
    fn prefixes_and_errors() {
        let mut app = crate::App::new(crate::tests::app_args());
        let svf = app.controller.add_module("loom.filter.svf").unwrap().0;
        let graph = app.controller.graph();
        assert!(matches!(find(graph, svf, "cutoff"), Ok(Part::Control(..))));
        let input = find(graph, svf, "input:cutoff").unwrap();
        assert!(matches!(input, Part::Port(PortRef { output: false, .. })));
        assert_eq!(name(graph, input), Some(format!("{svf}:input:cutoff")));
        assert!(matches!(
            find(graph, svf, "output:lowpass"),
            Ok(Part::Port(PortRef { output: true, .. }))
        ));
        let error = find(graph, svf, "input:lowpass").unwrap_err();
        assert!(error.contains("no input `lowpass`"), "{error}");
        let error = find(graph, svf, "output:cutoff").unwrap_err();
        assert!(error.contains("no output `cutoff`"), "{error}");
        assert_eq!(find(graph, 999, "cutoff"), Err("no module 999".to_string()));
        assert_eq!(name(graph, Part::Control(999, 0)), None);
        assert_eq!(name(graph, Part::Control(svf, 99)), None);
        let gone = PortRef {
            module: svf,
            output: true,
            index: 99,
        };
        assert_eq!(name(graph, Part::Port(gone)), None);
        // Not on the canvas: the scene has no module 999.
        assert!(world_bounds(&app.scene, Part::Control(999, 0)).is_err());
        assert!(world_bounds(&app.scene, Part::Port(PortRef { module: 999, ..gone })).is_err());
    }
}
