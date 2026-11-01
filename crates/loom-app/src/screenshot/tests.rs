use super::*;

/// The window at its default size, drawn by `draw_window` with the wgpu
/// renderer. `None` on a machine without a GPU adapter.
fn render_window(app: &crate::App) -> Option<Vec<u8>> {
    let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))?;
    Some(draw_window(app, &mut renderer, crate::WINDOW, 1.0))
}

/// The headless app for an example patch, as `--screenshot-window`
/// builds it.
fn example_app(name: &str) -> crate::App {
    let mut args = crate::tests::app_args();
    args.patch = Some(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/patches")
            .join(name),
    );
    crate::snapshot::headless(&args).expect("example patch")
}

/// A window image shows enabled buttons as enabled. Iced picks a
/// button's look while it handles a redraw, and draws it as disabled
/// before the first one, so without a redraw every button looked
/// disabled (#460). The toolbar also follows its button roles: no
/// primary button while MIDI learn is off, and Panic in danger red
/// (visual-system.md, Toolbar).
#[test]
fn window_images_draw_enabled_and_disabled_buttons_apart() {
    let app = example_app("subtractive.ron");
    let Some(pixels) = render_window(&app) else {
        assert!(
            !std::env::var("LOOM_REQUIRE_GPU").is_ok_and(|value| value == "1"),
            "coverage requires a GPU adapter"
        );
        return;
    };
    let palette = app.theme().palette();
    let rgb = |c: iced::Color| [c.r, c.g, c.b].map(|v| (v * 255.0).round() as i32);
    let near = |got: [u8; 3], want: [i32; 3]| got.iter().zip(want).all(|(&g, w)| (i32::from(g) - w).abs() <= 3);
    // The same app laid out by the test harness, to find the buttons
    // by their text. A new patch has nothing to undo.
    let mut ui = crate::ui_test::Ui::with_app(app);
    assert!(ui.app.edits.history.undo_step().is_none());
    let width = crate::WINDOW.width as usize;
    let mut fill = |label: &str| {
        let text = ui.text(label).unwrap_or_else(|| panic!("no `{label}` button"));
        // Inside the button's padding, left of its text.
        let (x, y) = ((text.x - 4.0) as usize, text.center_y() as usize);
        let at = (y * width + x) * 4;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    };
    let new = fill("New");
    let undo = fill("Undo");
    assert_ne!(new, undo, "an enabled and a disabled button look the same");
    for label in ["New", "Open", "Add module", "Redo", "MIDI learn", "Preferences"] {
        assert!(!near(fill(label), rgb(palette.primary)), "{label} is primary");
    }
    let panic = fill("Panic");
    assert!(
        near(panic, rgb(palette.danger)),
        "Panic is drawn {panic:?}, not danger red"
    );
    let primary = palette.primary;
    let expected = rgb(primary);
    let is_primary = |rgb: &[u8]| {
        rgb.iter()
            .zip(expected)
            .all(|(&got, want)| (i32::from(got) - want).abs() <= 3)
    };

    // A checkbox keeps its status the same way: an assignment that is
    // on fills its box, left of its source, with the primary colour.
    let mut app = example_app("modulation.ron");
    app.selection.insert(4);
    let pixels = render_window(&app).expect("the renderer used above");
    let mut ui = crate::ui_test::Ui::with_app(app);
    let source = ui.text("LFO #5: Out").expect("the LFO assignment's source");
    let y = source.center_y() as usize;
    let left = (source.x - 30.0) as usize;
    let filled = (y - 4..y + 4)
        .flat_map(|y| (left..source.x as usize).map(move |x| (y * width + x) * 4))
        .filter(|&at| is_primary(&pixels[at..at + 3]))
        .count();
    assert!(filled > 20, "the On box is not filled with the primary colour");
}

/// A headless window draw exercises the second GPU primitive and shows
/// the dragged cable in the inspector, outside the canvas's clip.
#[test]
fn a_window_drag_draws_over_the_inspector() {
    let mut app = example_app("subtractive.ron");
    let Some(before) = render_window(&app) else {
        assert!(
            !std::env::var("LOOM_REQUIRE_GPU").is_ok_and(|value| value == "1"),
            "coverage requires a GPU adapter"
        );
        return;
    };
    let from = loom_canvas::PortRef {
        module: 7,
        index: 0,
        output: true,
    };
    assert!(app.scene.port(from).is_some());
    app.gestures.drag(crate::gestures::AssignDrag {
        from,
        start: Point::new(900.0, 400.0),
        at: Some(Point::new(1250.0, 150.0)),
        zoom: 1.0,
    });
    let after = render_window(&app).expect("the renderer used above");
    let width = crate::WINDOW.width as usize;
    let changed = (120..220)
        .flat_map(|y| (1180..1250).map(move |x| (y * width + x) * 4))
        .filter(|&pixel| before[pixel..pixel + 4] != after[pixel..pixel + 4])
        .count();
    assert!(changed > 10, "the cable did not cross the inspector");
}

/// `--pointer` and `--center` find a light by its display value, and a
/// meter whose name its output also has by `display:` (#254).
#[test]
fn pointer_and_center_find_lights_and_meters() {
    let mut app = crate::App::new(crate::tests::app_args());
    let env = app.controller.add_module("loom.env.envelope").unwrap().0;
    let meter = app.controller.add_module("loom.display.meter").unwrap().0;
    app.commit();
    let named = |s: String| s.parse::<Named>().unwrap();
    let bounds = |module: u32, widget: usize| app.scene.module(module).unwrap().widget_bounds(widget).unwrap();
    let light = app
        .controller
        .graph()
        .node(ModuleId(env))
        .unwrap()
        .descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.bind == "level")
        .unwrap();
    let point = find_point(&app.controller, &app.scene, &named(format!("{env}:level"))).unwrap();
    assert_eq!(point, bounds(env, light).center());
    let point = find_point(&app.controller, &app.scene, &named(format!("{meter}:display:out"))).unwrap();
    assert_eq!(point, bounds(meter, 0).center());
    let port = find_point(&app.controller, &app.scene, &named(format!("{meter}:out"))).unwrap();
    assert_ne!(port, bounds(meter, 0).center(), "without the prefix, the output");
}

#[test]
fn parses_options() {
    assert_eq!(
        "640x480".parse::<ImageSize>(),
        Ok(ImageSize {
            width: 640,
            height: 480
        })
    );
    assert!("640".parse::<ImageSize>().is_err());
    assert!("0x480".parse::<ImageSize>().is_err());
    assert_eq!("10.5,-3".parse::<WorldPoint>(), Ok(WorldPoint(Point::new(10.5, -3.0))));
    let cutoff = Named {
        module: 4,
        name: "cutoff".to_string(),
    };
    assert_eq!("4:cutoff".parse::<Named>(), Ok(cutoff.clone()));
    assert!("cutoff".parse::<Named>().is_err());
    assert!("x:cutoff".parse::<Named>().is_err());
    assert!("4:".parse::<Named>().is_err());
    assert_eq!("4:cutoff".parse::<Pointer>(), Ok(Pointer::Named(cutoff)));
    assert_eq!("10,20".parse::<Pointer>(), Ok(Pointer::World(Point::new(10.0, 20.0))));
    assert!("10".parse::<Pointer>().is_err());
    assert!("NaN,0".parse::<WorldPoint>().is_err());
    assert!("0,inf".parse::<WorldPoint>().is_err());
    assert_eq!(positive("2"), Ok(2.0));
    for bad in ["0", "-1", "NaN", "inf"] {
        assert!(positive(bad).is_err(), "{bad}");
    }
}

/// The new options parse together, and clap enforces how they combine.
#[test]
fn parses_state_options() {
    use clap::Parser;
    let parse =
        |args: &[&str]| crate::Args::try_parse_from([&["loom-app", "p.ron", "--screenshot", "o.png"], args].concat());
    let args = parse(&[
        "--pointer",
        "4:cutoff",
        "--pin",
        "5:out",
        "--wire",
        "5:out",
        "--hover-text",
    ])
    .unwrap();
    assert_eq!(args.view.pin.as_ref().map(|n| n.name.as_str()), Some("out"));
    assert!(args.view.hover_text);
    // A wire needs a pointer for its loose end.
    assert!(parse(&["--wire", "5:out"]).is_err());
    // One drag at a time.
    assert!(parse(&["--pointer", "1,1", "--wire", "5:out", "--depth", "4:cutoff"]).is_err());
    // Without --screenshot, the state options mean nothing.
    assert!(crate::Args::try_parse_from(["loom-app", "--pin", "5:out"]).is_err());
}

/// The canvas options and --select go with --screenshot-window, and the
/// two screenshots exclude each other.
#[test]
fn parses_window_options() {
    use clap::Parser;
    let parse = |args: &[&str]| crate::Args::try_parse_from([&["loom-app", "p.ron"], args].concat());
    let args = parse(&["--screenshot-window", "o.png", "--select", "4", "--focus", "4:cutoff"]).unwrap();
    assert_eq!(args.select, [4]);
    assert!(args.view.screenshot_window.is_some());
    assert!(parse(&["--screenshot-window", "o.png", "--size", "800x600"]).is_ok());
    assert!(parse(&["--screenshot-window", "o.png", "--screenshot", "c.png"]).is_err());
    assert!(parse(&["--screenshot-window", "o.png", "--dump-ui"]).is_err());
    // --select still needs something that shows the inspector.
    assert!(parse(&["--select", "4"]).is_err());
    assert!(parse(&["--screenshot", "c.png", "--select", "4"]).is_err());
    assert!(parse(&["--dump-ui", "--select", "4"]).is_ok());
}

#[test]
fn scale_keeps_the_image_in_range() {
    let size = Size::new(640.0, 400.0);
    assert_eq!(physical_size(size, 2.0), Ok(Size::new(1280, 800)));
    assert!(physical_size(Size::new(1.0, 1.0), 0.1).is_err());
    assert!(physical_size(size, 100.0).is_err());
}

#[test]
fn camera_centres_the_given_point_at_the_given_zoom() {
    let scene = Scene::new(Vec::new(), Vec::new());
    let size = Size::new(800.0, 600.0);
    let view = camera(&scene, size, Some(2.0), Some(Point::new(100.0, 50.0)));
    assert_eq!(view.zoom, 2.0);
    assert_eq!(view.to_screen(Point::new(100.0, 50.0)), Point::new(400.0, 300.0));
    // Out of range zoom is clamped, as in the window.
    assert_eq!(camera(&scene, size, Some(100.0), None).zoom, MAX_ZOOM);
}
