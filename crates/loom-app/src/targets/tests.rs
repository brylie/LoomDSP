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
