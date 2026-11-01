use super::*;

fn port(label: &str) -> PortView {
    PortView {
        label: label.into(),
        hover: HoverText::default(),
        colour: Color::WHITE,
        shape: PortShape::Round,
        badge: 0,
        follows_transport: None,
    }
}

fn module(id: u32, x: f32, y: f32) -> ModuleView {
    ModuleView {
        size: None,
        unknown: false,
        id,
        title: format!("m{id}"),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position: Point::new(x, y),
        inputs: vec![port("a"), port("b")],
        outputs: vec![port("out")],
        panel: PanelView::default(),
    }
}

fn knob(x: u8, y: u8) -> WidgetView {
    WidgetView {
        kind: WidgetViewKind::Knob { steps: 0 },
        x,
        y,
        label: "k".into(),
        hover: HoverText::default(),
        hidden: false,
    }
}

/// A saved size grows displays, ports, and hit bounds while controls stay put.
#[test]
fn growable_tiles_use_saved_dimensions() {
    let mut m = module(1, 0.0, 0.0);
    m.panel = PanelView {
        width: 12,
        height: 6,
        widgets: vec![
            knob(0, 0),
            WidgetView {
                kind: WidgetViewKind::Scope,
                x: 3,
                y: 0,
                label: String::new(),
                hover: HoverText::default(),
                hidden: false,
            },
        ],
    };
    let initial = m.bounds();
    let knob = m.widget_bounds(0).unwrap();
    let scope = m.widget_bounds(1).unwrap();
    let first_port = m.input_position(0);
    let output = m.output_position(0);
    m.size = Some((40, 30));
    let bounds = m.bounds();
    assert_eq!(bounds.size(), iced::Size::new(40.0 * GRID, 30.0 * GRID));
    assert_eq!(m.widget_bounds(0).unwrap(), knob);
    let enlarged = m.widget_bounds(1).unwrap();
    assert_eq!(enlarged.width - scope.width, bounds.width - initial.width);
    assert_eq!(enlarged.height - scope.height, bounds.height - initial.height);
    assert!(m.input_position(0).y > first_port.y);
    assert!(m.output_position(0).x > output.x);
    let scene = Scene::new(vec![m], Vec::new());
    assert!(scene.module(1).unwrap().bounds().contains(Point::new(
        bounds.x + bounds.width - 1.0,
        bounds.y + bounds.height - 1.0
    )));
}

#[test]
fn panels_widen_and_heighten_tiles() {
    let mut m = module(1, 0.0, 0.0);
    assert_eq!(m.width(), TILE_WIDTH);
    m.panel = PanelView {
        width: 6,
        height: 8,
        widgets: vec![knob(0, 0), knob(3, 0)],
    };
    // 4 + 6 + 4 grid units.
    assert_eq!(m.width(), 14.0 * GRID);
    assert!(m.height() >= HEADER + 8.0 * GRID);
    assert_eq!(m.height() % GRID, 0.0);
    // Outputs move to the new right edge.
    assert_eq!(m.output_position(0).x, 14.0 * GRID);
    // The panel starts after the left label column.
    let first = m.widget_bounds(0).unwrap();
    assert_eq!(first.x, 4.0 * GRID);
    assert_eq!(first.width, 3.0 * GRID);
    assert_eq!(m.widget_bounds(1).unwrap().x, 7.0 * GRID);
}

#[test]
fn narrow_panels_are_centred() {
    let mut m = module(1, 0.0, 0.0);
    m.panel = PanelView {
        width: 1,
        height: 1,
        widgets: Vec::new(),
    };
    // 11 units wide, 8 for labels, 1 for the panel, 1 spare on each side.
    assert_eq!(m.panel_origin().x, 5.0 * GRID);
}

#[test]
fn controls_take_the_press_and_lights_do_not() {
    let mut m = module(1, 0.0, 0.0);
    m.panel = PanelView {
        width: 6,
        height: 3,
        widgets: vec![
            knob(0, 0),
            WidgetView {
                kind: WidgetViewKind::Light,
                x: 3,
                y: 0,
                label: "l".into(),
                hover: HoverText::default(),
                hidden: false,
            },
        ],
    };
    let knob_centre = m.widget_bounds(0).unwrap().center();
    let light_centre = m.widget_bounds(1).unwrap().center();
    let scene = Scene::new(vec![m], Vec::new());
    assert_eq!(
        scene.hit(knob_centre, PORT_HIT_RADIUS),
        Hit::Widget { module: 1, widget: 0 }
    );
    assert_eq!(scene.hit(light_centre, PORT_HIT_RADIUS), Hit::Module(1));
}

#[test]
fn long_labels_are_cut() {
    assert_eq!(fit_label("in", 37.0, 9.0), "in");
    let cut = fit_label("a-very-long-label", 37.0, 9.0);
    assert!(cut.ends_with('…'), "{cut}");
    assert!(cut.chars().count() <= 8);
}

#[test]
fn narrow_letters_leave_room() {
    // A knob caption is 3 grid units wide (panels.md, Labels and hover
    // text): "Division" fits, "Bandpass" and "Resonance" are cut.
    let knob = 3.0 * GRID;
    assert_eq!(fit_label("Division", knob, 9.0), "Division");
    assert_eq!(fit_label("Bandpass", knob, 9.0), "Bandp…");
    assert!(fit_label("Resonance", knob, 9.0).ends_with('…'));
    assert_eq!(fit_label("", knob, 9.0), "");
    // A port label column beside a panel is 37 world units.
    assert_eq!(fit_label("Lowpass", 37.0, 9.0), "Lowpass");
    assert_eq!(fit_label("Velocity", 37.0, 9.0), "Velocity");
}

#[test]
fn height_snaps_to_grid() {
    let m = module(1, 0.0, 0.0);
    assert_eq!(m.height() % GRID, 0.0);
    assert!(m.height() >= HEADER + 2.0 * ROW);
}

#[test]
fn hits_ports_before_tiles() {
    let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 300.0, 0.0)], Vec::new());
    let m = scene.module(1).unwrap();
    let input = m.input_position(1);
    assert_eq!(
        scene.hit(input + Vector::new(2.0, 1.0), PORT_HIT_RADIUS),
        Hit::Port(PortRef {
            module: 1,
            output: false,
            index: 1
        })
    );
    let output = m.output_position(0);
    assert_eq!(
        scene.hit(output, PORT_HIT_RADIUS),
        Hit::Port(PortRef {
            module: 1,
            output: true,
            index: 0
        })
    );
    assert_eq!(scene.hit(Point::new(60.0, 10.0), PORT_HIT_RADIUS), Hit::Module(1));
    assert_eq!(scene.hit(Point::new(200.0, 10.0), PORT_HIT_RADIUS), Hit::Nothing);
}

#[test]
fn later_tiles_win() {
    let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 40.0, 0.0)], Vec::new());
    assert_eq!(scene.hit(Point::new(60.0, 10.0), PORT_HIT_RADIUS), Hit::Module(2));
}

#[test]
fn moving_changes_generation() {
    let mut scene = Scene::new(vec![module(1, 0.0, 0.0)], Vec::new());
    let before = scene.generation();
    scene.move_modules(&[1], Vector::new(GRID, 0.0));
    assert_ne!(scene.generation(), before);
    assert_eq!(scene.module(1).unwrap().position, Point::new(GRID, 0.0));
}

#[test]
fn hits_cables_where_they_are_drawn() {
    let cable = CableView {
        from: (1, 0),
        to: (2, 0),
        colour_from: Color::WHITE,
        colour_to: Color::WHITE,
        delayed: false,
    };
    let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 400.0, 200.0)], vec![cable]);
    let from = scene.module(1).unwrap().output_position(0);
    let to = scene.module(2).unwrap().input_position(0);
    // The middle of a forward curve is the midpoint of its ends.
    let middle = Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0);
    assert_eq!(scene.hit(middle, PORT_HIT_RADIUS), Hit::Cable(0));
    assert_eq!(
        scene.hit(middle + Vector::new(0.0, 40.0), PORT_HIT_RADIUS),
        Hit::Nothing
    );
}

#[test]
fn box_selection() {
    let scene = Scene::new(vec![module(1, 0.0, 0.0), module(2, 300.0, 0.0)], Vec::new());
    let area = Rectangle::new(Point::new(-10.0, -10.0), iced::Size::new(150.0, 20.0));
    assert_eq!(scene.modules_in(area), vec![1]);
}

#[test]
fn snapping() {
    assert_eq!(snap(Point::new(13.0, -7.0)), Point::new(12.0, -12.0));
}

/// A tile with two outputs, the second driving 12 assignments.
fn badged(id: u32, position: Point) -> ModuleView {
    let mut m = ModuleView {
        size: None,
        unknown: false,
        id,
        title: String::new(),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position,
        inputs: vec![port("in")],
        outputs: vec![port("a"), port("b")],
        panel: PanelView::default(),
    };
    m.outputs[1].badge = 12;
    m
}

/// A badge sits above and right of its port: clear of the marker and of
/// the cable, which leaves the port to the right at the port's height.
#[test]
fn a_badge_clears_its_port_and_cable() {
    let scene = Scene::new(vec![badged(1, Point::ORIGIN)], Vec::new());
    let m = scene.module(1).unwrap();
    assert_eq!(scene.badge_bounds(1, 0), None);
    let badge = scene.badge_bounds(1, 1).unwrap();
    let port = m.output_position(1);
    assert!(badge.x > port.x + PORT_RADIUS);
    assert!(badge.y + badge.height < port.y);
    // Wider than tall for two digits, and clear of the port above.
    assert!(badge.width > badge.height);
    assert!(badge.y > m.output_position(0).y);
}

/// #249: with another tile close to its right, a badge moves inside
/// its own tile, so it covers none of the other tile's ports or the
/// cables into them. It still clears its own port and cable. Moving the
/// other tile away puts it back outside.
#[test]
fn a_badge_moves_inside_its_tile_when_another_tile_is_close() {
    let left = badged(1, Point::ORIGIN);
    let edge = left.bounds().x + left.bounds().width;
    let right = badged(2, Point::new(edge + GRID, 0.0));
    let mut scene = Scene::new(vec![left, right], Vec::new());

    let tile = scene.module(1).unwrap().bounds();
    let badge = scene.badge_bounds(1, 1).unwrap();
    let port = scene.module(1).unwrap().output_position(1);
    assert!(
        badge.x >= tile.x && badge.x + badge.width < port.x - PORT_RADIUS,
        "{badge:?}"
    );
    assert!(badge.y + badge.height < port.y);
    let neighbour = scene.module(2).unwrap();
    for index in 0..neighbour.inputs.len() {
        let input = neighbour.input_position(index);
        let reach = Rectangle::new(
            Point::new(input.x - GRID, input.y - PORT_RADIUS),
            iced::Size::new(GRID + PORT_RADIUS, 2.0 * PORT_RADIUS),
        );
        assert!(!badge.intersects(&reach), "badge {badge:?} covers input {index}");
    }
    assert!(!badge.intersects(&neighbour.bounds()));
    // The click target moves with it.
    assert_eq!(
        scene.hit(badge.center(), PORT_HIT_RADIUS),
        Hit::Badge(PortRef {
            module: 1,
            output: true,
            index: 1
        })
    );
    // The rightmost tile has nothing beside it, so its badge stays out.
    let far = scene.badge_bounds(2, 1).unwrap();
    assert!(far.x > scene.module(2).unwrap().output_position(1).x);

    // Four grid units apart, as the auto layout leaves columns, the
    // badge goes back outside.
    scene.move_modules(&[2], Vector::new(3.0 * GRID, 0.0));
    let badge = scene.badge_bounds(1, 1).unwrap();
    assert!(badge.x > port.x + PORT_RADIUS, "{badge:?}");
}

/// A tile moved on top of an inside badge keeps its input: a click
/// that reaches both the badge and that input hits the input, not the
/// badge (#249).
#[test]
fn a_neighbour_input_wins_over_a_badge_under_it() {
    let left = badged(1, Point::ORIGIN);
    let edge = left.bounds().x + left.bounds().width;
    let right = badged(2, Point::new(edge + GRID, 0.0));
    let mut scene = Scene::new(vec![left, right], Vec::new());
    let badge = scene.badge_bounds(1, 1).unwrap().center();

    // Move the neighbour so its first input sits on the badge.
    let input = scene.module(2).unwrap().input_position(0);
    scene.move_modules(&[2], badge - input);
    assert!(scene.badge_bounds(1, 1).unwrap().contains(badge));
    assert_eq!(
        scene.hit(badge, PORT_HIT_RADIUS),
        Hit::Port(PortRef {
            module: 2,
            output: false,
            index: 0
        })
    );
}
