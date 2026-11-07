use super::*;

/// A patch with an envelope, whose light is `level`, and a Level Meter
/// on its output, whose meter shares the name `out` with the output.
fn patch() -> (crate::App, u32, u32) {
    let mut app = crate::App::new(crate::tests::app_args());
    let env = app.session.controller.add_module("loom.env.envelope").unwrap();
    let meter = app.session.controller.add_module("loom.display.meter").unwrap();
    app.session.controller.connect_named(env, "out", meter, "in").unwrap();
    app.commit();
    (app, env.0, meter.0)
}

/// Rule 10: a parameter, then an output, then an input, then a display
/// value. A light is found by its display value, and a meter that
/// shares a port's name by `display:`.
#[test]
fn names_find_parameters_ports_then_lights_and_meters() {
    let (app, env, meter) = patch();
    let graph = app.session.controller.graph();
    let h = &app.session.hierarchy;
    assert!(matches!(find(graph, h, env, "attack"), Ok(Part::Control(..))));
    assert!(matches!(
        find(graph, h, env, "gate"),
        Ok(Part::Port(PortRef { output: false, .. }))
    ));
    let light = find(graph, h, env, "level").unwrap();
    assert!(matches!(light, Part::Display(..)));
    assert_eq!(name(graph, h, light), Some(format!("{env}:level")));

    let out = find(graph, h, meter, "out").unwrap();
    assert!(matches!(out, Part::Port(PortRef { output: true, .. })), "{out:?}");
    let display = find(graph, h, meter, "display:out").unwrap();
    assert!(matches!(display, Part::Display(..)));
    assert_eq!(name(graph, h, display), Some(format!("{meter}:display:out")));
    assert_eq!(name(graph, h, out), Some(format!("{meter}:out")));

    let error = find(graph, h, env, "nonsense").unwrap_err();
    assert!(
        error.contains("no control, port, light, or meter `nonsense`"),
        "{error}"
    );
    let error = find(graph, h, env, "display:attack").unwrap_err();
    assert!(error.contains("no light or meter `attack`"), "{error}");

    let badge = find(graph, h, env, "badge:out").unwrap();
    assert!(matches!(badge, Part::Badge(PortRef { output: true, .. })));
    assert_eq!(name(graph, h, badge), Some(format!("{env}:badge:out")));
    // No assignment, so no badge is drawn.
    assert!(
        world_bounds(&app.session.scene, &app.session.values, badge)
            .unwrap_err()
            .contains("no badge")
    );
    assert!(find(graph, h, env, "badge:gate").is_err(), "inputs have no badge");
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
        let id = match app.session.controller.add_module(descriptor.id) {
            Err(loom_core::LoomError::SecondTransport(existing)) => existing.0,
            added => added.unwrap().0,
        };
        let graph = app.session.controller.graph();
        let h = &app.session.hierarchy;
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
            let text = name(graph, h, part).unwrap_or_else(|| panic!("{}: no name for {part:?}", descriptor.id));
            let (module, rest) = text.split_once(':').unwrap();
            assert_eq!(module.parse::<u32>().unwrap(), id);
            assert_eq!(find(graph, h, id, rest), Ok(part), "{}: {text}", descriptor.id);
        }
    }
}

/// The prefixes, and what the lookup refuses.
#[test]
fn prefixes_and_errors() {
    let mut app = crate::App::new(crate::tests::app_args());
    let svf = app.session.controller.add_module("loom.filter.svf").unwrap().0;
    let graph = app.session.controller.graph();
    let h = &app.session.hierarchy;
    assert!(matches!(find(graph, h, svf, "cutoff"), Ok(Part::Control(..))));
    let input = find(graph, h, svf, "input:cutoff").unwrap();
    assert!(matches!(input, Part::Port(PortRef { output: false, .. })));
    assert_eq!(name(graph, h, input), Some(format!("{svf}:input:cutoff")));
    assert!(matches!(
        find(graph, h, svf, "output:lowpass"),
        Ok(Part::Port(PortRef { output: true, .. }))
    ));
    let error = find(graph, h, svf, "input:lowpass").unwrap_err();
    assert!(error.contains("no input `lowpass`"), "{error}");
    let error = find(graph, h, svf, "output:cutoff").unwrap_err();
    assert!(error.contains("no output `cutoff`"), "{error}");
    assert_eq!(find(graph, h, 999, "cutoff"), Err("no module 999".to_string()));
    assert_eq!(name(graph, h, Part::Control(999, 0)), None);
    assert_eq!(name(graph, h, Part::Control(svf, 99)), None);
    let gone = PortRef {
        module: svf,
        output: true,
        index: 99,
    };
    assert_eq!(name(graph, h, Part::Port(gone)), None);
    // Not on the canvas: the scene has no module 999.
    assert!(world_bounds(&app.session.scene, &app.session.values, Part::Control(999, 0)).is_err());
    assert!(
        world_bounds(
            &app.session.scene,
            &app.session.values,
            Part::Port(PortRef { module: 999, ..gone })
        )
        .is_err()
    );
}

/// On a sub-patch tile a name finds a surface control by its exposed name
/// and a port by its name, with the `input:` and `output:` prefixes, and
/// the name round-trips; a port tile inside has its one port. Displays and
/// badges are not on these tiles.
#[test]
fn names_find_sub_patch_tiles_and_port_tiles() {
    let ui = crate::ui_test::Ui::open("nested-subtractive.ron");
    let graph = ui.app.session.controller.graph();
    let h = &ui.app.session.hierarchy;
    let port = |module, output, index| Part::Port(PortRef { module, output, index });
    assert_eq!(find(graph, h, 2, "cutoff"), Ok(Part::Control(2, 0)));
    assert_eq!(find(graph, h, 2, "pitch"), Ok(port(2, false, 0)));
    assert_eq!(find(graph, h, 2, "gate"), Ok(port(2, false, 1)));
    assert_eq!(find(graph, h, 2, "out"), Ok(port(2, true, 0)));
    assert_eq!(find(graph, h, 2, "filter"), Ok(port(2, true, 1)));
    assert_eq!(find(graph, h, 2, "output:out"), Ok(port(2, true, 0)));
    assert_eq!(find(graph, h, 2, "input:pitch"), Ok(port(2, false, 0)));
    assert!(find(graph, h, 2, "input:out").unwrap_err().contains("no input `out`"));
    assert!(find(graph, h, 2, "display:cutoff").unwrap_err().contains("no light"));
    assert!(find(graph, h, 2, "badge:out").is_err());
    assert!(find(graph, h, 2, "nonsense").unwrap_err().contains("(Voice)"));
    assert_eq!(name(graph, h, Part::Control(2, 0)), Some("2:cutoff".to_string()));
    assert_eq!(name(graph, h, port(2, false, 0)), Some("2:pitch".to_string()));
    assert_eq!(name(graph, h, port(2, true, 1)), Some("2:filter".to_string()));
    assert_eq!(name(graph, h, Part::Control(2, 9)), None);
    assert_eq!(name(graph, h, port(2, true, 9)), None);

    let voice = loom_patch::ModulePath::top(2);
    let input = h.id_at(&voice, 1).unwrap().0;
    let output = h.id_at(&voice, 8).unwrap().0;
    assert_eq!(find(graph, h, input, "out"), Ok(port(input, true, 0)));
    assert!(find(graph, h, input, "in").is_err(), "a Patch input has no input");
    assert_eq!(find(graph, h, output, "in"), Ok(port(output, false, 0)));
    assert_eq!(name(graph, h, port(input, true, 0)), Some(format!("{input}:out")));
    assert_eq!(name(graph, h, port(input, false, 0)), None);
    assert_eq!(find(graph, h, 999, "cutoff"), Err("no module 999".to_string()));
}
