use super::*;
use crate::WidgetValue;
use crate::scene::{CableView, ModuleView, PanelView, PortShape, PortView, WidgetView};

fn module(id: u32, x: f32) -> ModuleView {
    let port = || PortView {
        label: "p".into(),
        hover: crate::scene::HoverText::default(),
        colour: Color::WHITE,
        shape: PortShape::Round,
        badge: 0,
        follows_transport: None,
    };
    ModuleView {
        size: None,
        unknown: false,
        id,
        title: String::new(),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position: Point::new(x, 0.0),
        inputs: vec![port()],
        outputs: vec![port()],
        panel: PanelView::default(),
    }
}

fn widget(kind: WidgetViewKind, x: u8) -> WidgetView {
    WidgetView {
        kind,
        x,
        y: 0,
        label: String::new(),
        hover: crate::scene::HoverText::default(),
        hidden: false,
    }
}

/// A knob, a light, and a toggle.
fn panel() -> PanelView {
    PanelView {
        width: 9,
        height: 3,
        widgets: vec![
            widget(WidgetViewKind::Knob { steps: 0 }, 0),
            widget(WidgetViewKind::Light, 3),
            widget(WidgetViewKind::Toggle, 6),
        ],
    }
}

/// Tab skips a control its module's mode hides (ADR-0030).
#[test]
fn tab_skips_hidden_controls() {
    let mut panel = panel();
    panel.widgets[2].hidden = true;
    assert_eq!(next_focus(&panel, None, false), Some(0));
    assert_eq!(next_focus(&panel, Some(0), false), Some(0), "only the knob is left");
    assert_eq!(focusable(&panel), [0], "Tab within a module walks the same list");
}

#[test]
fn tab_moves_focus_over_controls_and_skips_lights() {
    let panel = panel();
    assert_eq!(next_focus(&panel, None, false), Some(0));
    assert_eq!(next_focus(&panel, Some(0), false), Some(2));
    assert_eq!(next_focus(&panel, Some(2), false), Some(0));
    assert_eq!(next_focus(&panel, Some(0), true), Some(2));
    assert_eq!(next_focus(&panel, None, true), Some(2));
    assert_eq!(next_focus(&PanelView::default(), None, false), None);
}

#[test]
fn detail_follows_the_text_thresholds() {
    assert_eq!(detail(1.0), Detail::Close);
    // Port labels hidden, titles shown.
    assert_eq!(detail(0.7), Detail::Middle);
    assert_eq!(detail(0.3), Detail::Far);
    // Framing stops at the first zoom that still draws labels.
    assert_eq!(detail(READABLE_ZOOM), Detail::Close);
    assert_eq!(detail(READABLE_ZOOM * 0.9), Detail::Middle);
}

#[test]
fn only_lights_on_screen_are_reported() {
    let mut lit = module(1, 0.0);
    lit.panel = panel();
    let far_away = ModuleView {
        panel: panel(),
        ..module(2, 5_000.0)
    };
    let plain = module(3, 200.0);
    let scene = Scene::new(vec![lit, far_away, plain], Vec::new());
    let camera = Camera {
        origin: Point::new(-50.0, -50.0),
        zoom: 1.0,
    };
    let size = Size::new(800.0, 600.0);
    assert_eq!(visible_lights(&scene, &camera, size), vec![1]);
    // Zoomed far out, lights are not drawn, so none need reading.
    let far = Camera { zoom: 0.3, ..camera };
    assert!(visible_lights(&scene, &far, size).is_empty());
}

#[test]
fn a_control_focused_by_keyboard_has_a_ring_clear_of_its_caption() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let unfocused = canvas.overlay(&State::default()).shapes.len();
    let tabbed = State {
        focus: Some((1, 0)),
        tab_focus: true,
        ..State::default()
    };
    let overlay = canvas.overlay(&tabbed);
    assert_eq!(overlay.shapes.len(), unfocused + 1);
    // The ring stays above the caption, where the value shows.
    let ring = overlay.shapes.last().unwrap().rect();
    let module = &scene.modules()[0];
    let mut caption = None;
    tile_text::for_each(module, |item| {
        if item.kind == TextKind::Caption(0) {
            caption = Some(item.bounds());
        }
    });
    let caption = caption.unwrap();
    assert!(
        ring[1] + ring[3] <= caption.y,
        "ring {ring:?} clear of caption {caption:?}"
    );
    // Focus from a click or a drag shows no ring (#92).
    let clicked = State {
        focus: Some((1, 0)),
        ..State::default()
    };
    assert_eq!(canvas.overlay(&clicked).shapes.len(), unfocused);
}

#[test]
fn hover_text_is_about_the_port_or_control_under_the_pointer() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let port = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let on = |hover| State {
        hover: Some(hover),
        ..State::default()
    };
    assert_eq!(canvas.hover_target(&on(Hit::Port(port))), Some(HoverTarget::Port(port)));
    let knob = Hit::Widget { module: 1, widget: 0 };
    assert_eq!(canvas.hover_target(&on(knob)), Some(HoverTarget::Control(1, 0)));
    assert_eq!(canvas.hover_target(&on(Hit::Module(1))), None);
    assert_eq!(canvas.hover_target(&on(Hit::Nothing)), None);
    // On a port's label: the port's hover text (#92).
    let mut label = None;
    tile_text::for_each(&scene.modules()[0], |item| {
        if item.kind == TextKind::Port(port) {
            label = Some(item.bounds().center());
        }
    });
    let on_label = State {
        cursor: label,
        ..on(Hit::Module(1))
    };
    assert_eq!(canvas.hover_target(&on_label), Some(HoverTarget::Port(port)));
}

/// Resting on a tile's header shows the module's hover text: its family
/// and description (ADR-0029). The body below the header does not.
#[test]
fn the_header_hovers_with_the_module_family() {
    let mut m = module(1, 0.0);
    m.hover = HoverText {
        description: "Sources. A test module.".into(),
        name: Some("test.module".into()),
    };
    let tile = m.bounds();
    let scene = Scene::new(vec![m], Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let at = |y: f32| State {
        hover: Some(Hit::Module(1)),
        cursor: Some(Point::new(tile.center_x(), y)),
        ..State::default()
    };
    assert_eq!(
        canvas.hover_target(&at(tile.y + HEADER * 0.5)),
        Some(HoverTarget::Title(1))
    );
    assert_eq!(canvas.hover_target(&at(tile.y + HEADER + 30.0)), None);
    let (_, hover, value, anchor) = canvas.hover_content(HoverTarget::Title(1)).unwrap();
    assert!(hover.description.starts_with("Sources."));
    assert_eq!(value, None);
    assert_eq!(anchor.height, HEADER);
}

/// A port label drawn over another tile's header hovers as the port,
/// not as that tile's title, because text is drawn above every tile.
#[test]
fn a_port_label_over_a_header_hovers_as_the_port() {
    let port = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let mut below = module(1, 0.0);
    below.panel = panel();
    let mut label = None;
    tile_text::for_each(&below, |item| {
        if item.kind == TextKind::Port(port) {
            label = Some(item.bounds().center());
        }
    });
    let label = label.expect("a port label");
    let mut cover = module(2, 0.0);
    cover.inputs.clear();
    cover.outputs.clear();
    cover.position = Point::new(label.x - 40.0, label.y - HEADER * 0.5);
    let scene = Scene::new(vec![below, cover], Vec::new());
    let header = scene.module(2).unwrap().bounds();
    assert!(label.y < header.y + HEADER, "the label is in the cover's header");
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let state = State {
        hover: Some(Hit::Module(2)),
        cursor: Some(label),
        ..State::default()
    };
    assert_eq!(canvas.hover_target(&state), Some(HoverTarget::Port(port)));
}

/// A light with a description hovers like a control, and its hover
/// text has the value line the app wrote. A light with no description
/// shows none (port-and-parameter-text.md, Display values).
#[test]
fn a_light_shows_its_description_and_value() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let plain = m.clone();
    m.panel.widgets[1].hover = HoverText::new("Gate", "Lit while a key is held.", "gate");
    let mut meter = module(2, 600.0);
    meter.panel = PanelView {
        width: 3,
        height: 6,
        widgets: vec![widget(WidgetViewKind::Meter, 0)],
    };
    meter.panel.widgets[0].hover = HoverText::new("Out", "The signal.", "out");
    let mut values = PanelValues::new();
    values.set(
        1,
        1,
        WidgetValue {
            position: 1.0,
            text: "Gate: 1.00".to_string(),
            ..WidgetValue::default()
        },
    );
    let scene = Scene::new(vec![m, meter], Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e).values(&values);
    // The pointer rests on the middle of a display widget. The hit is
    // the tile, so a press there still moves it.
    let resting_on = |scene: &Scene, module: u32, widget: usize| {
        let point = scene.module(module).unwrap().widget_bounds(widget).unwrap().center();
        let hit = scene.hit(point, 8.0);
        assert_eq!(hit, Hit::Module(module), "a display belongs to the tile");
        State {
            hover: Some(hit),
            cursor: Some(point),
            ..State::default()
        }
    };
    assert_eq!(
        canvas.hover_target(&resting_on(&scene, 1, 1)),
        Some(HoverTarget::Control(1, 1))
    );
    assert_eq!(
        canvas.hover_target(&resting_on(&scene, 2, 0)),
        Some(HoverTarget::Control(2, 0))
    );
    let (_, hover, value, _) = canvas.hover_content(HoverTarget::Control(1, 1)).unwrap();
    assert_eq!(hover.description, "Lit while a key is held.");
    assert_eq!(value, Some("Gate: 1.00"));
    // A knob's value shows on the knob, not in the hover text.
    let (_, _, value, _) = canvas.hover_content(HoverTarget::Control(1, 0)).unwrap();
    assert_eq!(value, None);

    // A light with no description shows none.
    let scene = Scene::new(vec![plain], Vec::new());
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    assert_eq!(canvas.hover_target(&resting_on(&scene, 1, 1)), None);
}

#[test]
fn no_hover_text_during_a_drag() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let port = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let drags = [
        Interaction::Wiring {
            from: port,
            pulled: false,
        },
        Interaction::Turning {
            module: 1,
            widget: 0,
            last_y: 0.0,
            position: 0.5,
        },
        Interaction::Moving {
            ids: vec![1],
            start: Point::ORIGIN,
            applied: Vector::ZERO,
            click: None,
        },
        Interaction::Panning {
            last: Point::ORIGIN,
            click: None,
        },
    ];
    for interaction in drags {
        let state = State {
            hover: Some(Hit::Port(port)),
            interaction: interaction.clone(),
            ..State::default()
        };
        assert_eq!(canvas.hover_target(&state), None, "{interaction:?}");
    }
}

#[test]
fn tab_focus_shows_hover_text_but_a_click_does_not() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let focused = |tab_focus| State {
        focus: Some((1, 2)),
        tab_focus,
        ..State::default()
    };
    assert_eq!(canvas.hover_target(&focused(true)), Some(HoverTarget::Control(1, 2)));
    assert_eq!(canvas.hover_target(&focused(false)), None);
    // With the pointer resting on the knob, a Tab press to the toggle
    // shows the toggle's text until the pointer moves.
    let knob = Hit::Widget { module: 1, widget: 0 };
    let tabbed = State {
        hover: Some(knob),
        focus_first: true,
        ..focused(true)
    };
    assert_eq!(canvas.hover_target(&tabbed), Some(HoverTarget::Control(1, 2)));
    let moved = State {
        hover: Some(knob),
        focus_first: false,
        ..focused(true)
    };
    assert_eq!(canvas.hover_target(&moved), Some(HoverTarget::Control(1, 0)));
    // Focus on a module that is no longer selected shows nothing.
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    assert_eq!(canvas.hover_target(&focused(true)), None);
}

fn cable(from: u32, to: u32) -> CableView {
    CableView {
        from: (from, 0),
        to: (to, 0),
        colour_from: Color::WHITE,
        colour_to: Color::WHITE,
        delayed: false,
    }
}

/// Modules 1 -> 2 -> 3, so selecting 1 focuses one cable and selecting
/// 2 focuses both.
fn chain() -> Scene {
    Scene::new(
        vec![module(1, 0.0), module(2, 200.0), module(3, 400.0)],
        vec![cable(1, 2), cable(2, 3)],
    )
}

/// Activity fills both ends of a cable without changing cable geometry.
#[test]
fn activity_fills_connected_ports_from_overlay() {
    let scene = chain();
    let selection = BTreeSet::new();
    let idle = PatchCanvas::new(&scene, &selection, |event| event);
    let active = PatchCanvas::new(&scene, &selection, |event| event).activity([(1, 0)]);
    let state = State::default();
    let before = idle.overlay(&state);
    let after = active.overlay(&state);
    assert_eq!(after.cables.len(), before.cables.len());
    assert_eq!(after.shapes.len(), before.shapes.len() + 2);
    assert!(after.shapes.iter().take(2).all(|shape| shape.fill()[3] > 0.0));
}

#[test]
fn selected_modules_bring_their_cables_forward() {
    let scene = chain();
    let state = State::default();

    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    assert!(canvas.overlay(&state).cables.is_empty());

    let first = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &first, |e| e);
    assert_eq!(canvas.overlay(&state).cables.len(), 1);

    let middle = BTreeSet::from([2]);
    let canvas = PatchCanvas::new(&scene, &middle, |e| e);
    assert_eq!(canvas.overlay(&state).cables.len(), 2);
}

#[test]
fn tiles_stay_draggable_when_zoomed_out() {
    // At the smallest zoom, the port hit radius must not cover the whole
    // tile, or pressing anywhere on it would start a cable.
    let scene = chain();
    let mut state = State::default();
    state.camera.zoom = crate::camera::MIN_ZOOM;
    let centre = scene.module(2).unwrap().bounds().center();
    assert_eq!(scene.hit(centre, state.hit_radius()), Hit::Module(2));
}

#[test]
fn hovering_a_module_brings_its_cables_forward() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let state = State {
        hover: Some(Hit::Module(3)),
        ..State::default()
    };
    assert_eq!(canvas.overlay(&state).cables.len(), 1);
}

#[test]
fn hovering_a_port_brings_only_its_cables_forward() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    // Module 2's input carries cable 1 -> 2; its output carries 2 -> 3.
    let state = State {
        hover: Some(Hit::Port(PortRef {
            module: 2,
            output: false,
            index: 0,
        })),
        ..State::default()
    };
    assert_eq!(canvas.overlay(&state).cables.len(), 1);
    // A port with no cable highlights nothing.
    let state = State {
        hover: Some(Hit::Port(PortRef {
            module: 1,
            output: false,
            index: 0,
        })),
        ..State::default()
    };
    assert!(canvas.overlay(&state).cables.is_empty());
}

#[test]
fn hovering_a_cable_brings_only_that_cable_forward() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let state = State {
        hover: Some(Hit::Cable(1)),
        ..State::default()
    };
    assert_eq!(canvas.overlay(&state).cables.len(), 1);
}

fn button_module() -> ModuleView {
    let mut module = module(1, 0.0);
    module.panel = PanelView {
        width: 6,
        height: 3,
        widgets: vec![widget(WidgetViewKind::Button, 0)],
    };
    module
}

fn drive(canvas: &PatchCanvas<'_, CanvasEvent>, state: &mut State, event: &Event, at: Point) -> Vec<CanvasEvent> {
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    canvas.handle(state, event, bounds, mouse::Cursor::Available(at), &mut shell);
    messages
}

fn click(button: mouse::Button) -> Event {
    Event::Mouse(mouse::Event::ButtonPressed(button))
}

/// A select click opens a list and a list choice sets one value. Dragging
/// the closed selector still moves its tile without changing sound.
#[test]
fn select_popup_preserves_choice_and_tile_gestures() {
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![widget(
            WidgetViewKind::Select {
                options: vec!["ADSR".into(), "AD".into()],
            },
            0,
        )],
    };
    let scene = Scene::new(vec![view], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |event| event);
    let mut state = State::default();
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        centre,
    );
    assert!(state.select_popup.is_some());
    let list = canvas
        .select_popup_bounds(&state, Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0)))
        .unwrap();
    let choice = Point::new(list.x + 20.0, list.y + SELECT_ROW_PX * 1.5);
    let picked = drive(&canvas, &mut state, &click(mouse::Button::Left), choice);
    assert!(picked.contains(&CanvasEvent::SetControl {
        module: 1,
        widget: 0,
        position: 1.0
    }));
    assert!(state.select_popup.is_none());

    // Scrolling dismisses the open list without changing the canvas zoom.
    drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        centre,
    );
    assert!(state.select_popup.is_some());
    let zoom = state.camera.zoom;
    let events = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        }),
        centre,
    );
    assert!(events.is_empty());
    assert!(state.select_popup.is_none());
    assert_eq!(state.camera.zoom, zoom);

    drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    let moved = Point::new(centre.x + GRID * 2.0, centre.y);
    let events = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::CursorMoved { position: moved }),
        moved,
    );
    assert!(events.iter().any(|event| matches!(event, CanvasEvent::Move { .. })));
    let released = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        moved,
    );
    assert!(
        !released
            .iter()
            .any(|event| matches!(event, CanvasEvent::SetControl { .. }))
    );
    assert!(state.select_popup.is_none());
}

/// Focused select lists open with Space, move the highlight with vertical
/// arrows, and commit with Enter. Other navigation cannot edit or move
/// focus behind the list. Escape dismisses it without an edit.
#[test]
fn select_popup_keyboard_choices_are_explicit() {
    let mut view = module(1, 0.0);
    view.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![widget(
            WidgetViewKind::Select {
                options: vec!["ADSR".into(), "AD".into()],
            },
            0,
        )],
    };
    let scene = Scene::new(vec![view], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |event| event);
    let mut state = State {
        focus: Some((1, 0)),
        ..State::default()
    };
    let at = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    assert!(drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), at).is_empty());
    assert!(state.select_popup.is_some());
    assert!(
        drive(
            &canvas,
            &mut state,
            &key_event(keyboard::key::Named::ArrowDown, true),
            at
        )
        .is_empty()
    );
    for key in [
        keyboard::key::Named::ArrowLeft,
        keyboard::key::Named::ArrowRight,
        keyboard::key::Named::Tab,
    ] {
        assert!(drive(&canvas, &mut state, &key_event(key, true), at).is_empty());
        assert_eq!(state.focus, Some((1, 0)));
        assert_eq!(state.select_popup.map(|popup| popup.highlight), Some(1));
    }
    let chosen = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Enter, true), at);
    assert_eq!(
        chosen,
        vec![CanvasEvent::SetControl {
            module: 1,
            widget: 0,
            position: 1.0
        }]
    );
    assert!(state.select_popup.is_none());
    drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), at);
    assert!(drive(&canvas, &mut state, &key_event(keyboard::key::Named::Escape, true), at).is_empty());
    assert!(state.select_popup.is_none());
}

/// When a mode change hides the focused select with its list open, the
/// list closes, and the next arrow or Enter edits nothing and moves no
/// tile (ADR-0030).
#[test]
fn hiding_the_focused_select_closes_it_and_uses_up_the_key() {
    let mut view = module(1, 0.0);
    let mut select = widget(
        WidgetViewKind::Select {
            options: vec!["ADSR".into(), "AD".into()],
        },
        0,
    );
    select.hidden = true;
    view.panel = PanelView {
        width: 6,
        height: 2,
        widgets: vec![select],
    };
    let scene = Scene::new(vec![view], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |event| event);
    let at = scene.module(1).unwrap().bounds().center();
    for key in [keyboard::key::Named::ArrowDown, keyboard::key::Named::Enter] {
        let mut state = State {
            focus: Some((1, 0)),
            select_popup: Some(SelectPopup {
                module: 1,
                widget: 0,
                highlight: 1,
            }),
            ..State::default()
        };
        let events = drive(&canvas, &mut state, &key_event(key, true), at);
        assert!(events.is_empty(), "{key:?}: {events:?}");
        assert!(state.select_popup.is_none() && state.focus.is_none());
    }
}

/// A removed select cannot swallow the next key meant for the canvas.
#[test]
fn stale_select_popup_passes_escape_to_normal_key_handling() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |event| event);
    let at = scene.module(1).unwrap().bounds().center();
    for module in [1, 999] {
        let mut state = State {
            select_popup: Some(SelectPopup {
                module,
                widget: 999,
                highlight: 0,
            }),
            ..State::default()
        };
        let events = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Escape, true), at);
        assert_eq!(events, vec![CanvasEvent::Select(Vec::new())]);
        assert!(state.select_popup.is_none());
    }
}

/// A drag in progress when a pop-up opens ends at the next redraw,
/// which reaches the canvas even when the pop-up takes every pointer
/// and key event. A button held by the pointer is released too.
#[test]
fn a_redraw_ends_gestures_and_holds_when_the_canvas_is_not_interactive() {
    let redraw = Event::Window(window::Event::RedrawRequested(Instant::now()));

    let scene = chain();
    let selected = BTreeSet::from([1]);
    let tile = scene.module(1).unwrap().bounds().center();
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    let pressed = drive(&canvas, &mut state, &click(mouse::Button::Left), tile);
    assert_eq!(pressed, vec![CanvasEvent::GestureStart]);
    let blocked = PatchCanvas::new(&scene, &selected, |e| e).interactive(false);
    let events = drive(&blocked, &mut state, &redraw, tile);
    assert!(events.contains(&CanvasEvent::GestureEnd), "{events:?}");
    assert!(matches!(state.interaction, Interaction::Idle));
    // Nothing is held now, so later redraws send nothing more.
    let again = drive(&blocked, &mut state, &redraw, tile);
    assert!(!again.contains(&CanvasEvent::GestureEnd), "{again:?}");

    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    let blocked = PatchCanvas::new(&scene, &selected, |e| e).interactive(false);
    let events = drive(&blocked, &mut state, &redraw, centre);
    assert!(
        events.contains(&CanvasEvent::ButtonReleased { module: 1, widget: 0 }),
        "{events:?}"
    );
}

fn key_a(modifiers: keyboard::Modifiers) -> Event {
    let key = keyboard::Key::Character("a".into());
    Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyA),
        location: keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

/// `A` alone opens the browser at the pointer. With any modifier it does
/// not (module-browser.md, Rules).
#[test]
fn only_a_with_no_modifier_opens_the_browser() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let at = Point::new(100.0, 300.0);
    let mut state = State::default();
    let events = drive(&canvas, &mut state, &key_a(keyboard::Modifiers::default()), at);
    assert!(
        matches!(events.as_slice(), [CanvasEvent::OpenBrowser { at: p, .. }] if *p == at),
        "{events:?}"
    );
    for modifiers in [
        keyboard::Modifiers::SHIFT,
        keyboard::Modifiers::CTRL,
        keyboard::Modifiers::ALT,
        keyboard::Modifiers::LOGO,
    ] {
        let events = drive(&canvas, &mut state, &key_a(modifiers), at);
        assert!(
            !events.iter().any(|e| matches!(e, CanvasEvent::OpenBrowser { .. })),
            "{modifiers:?}: {events:?}"
        );
    }
}

/// #16: Cmd/Ctrl+0 over the canvas sets the zoom to 100% from any
/// zoom, keeping the point under the pointer in place. Without the
/// command modifier, 0 does nothing.
#[test]
fn command_zero_resets_the_zoom_to_100_percent() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let zero = |modifiers| {
        let key = keyboard::Key::Character("0".into());
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Digit0),
            location: keyboard::Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        })
    };
    let at = Point::new(300.0, 200.0);
    for start in [crate::camera::MIN_ZOOM, 0.5, 1.0, 2.25, crate::camera::MAX_ZOOM] {
        let mut state = State::default();
        state.camera.zoom = start;
        state.camera.origin = Point::new(-40.0, 60.0);
        let before = state.camera.to_world(at);
        drive(&canvas, &mut state, &zero(keyboard::Modifiers::default()), at);
        assert_eq!(state.camera.zoom, start, "0 alone leaves the zoom at {start}");
        drive(&canvas, &mut state, &zero(keyboard::Modifiers::COMMAND), at);
        assert_eq!(state.camera.zoom, 1.0, "from {start}");
        let after = state.camera.to_world(at);
        assert!(
            (after.x - before.x).abs() < 1e-3 && (after.y - before.y).abs() < 1e-3,
            "from {start}: {before:?} moved to {after:?}"
        );
    }
}

/// Ctrl+V or Cmd+V pastes at the pointer over the canvas, and at the
/// centre of the view with the pointer elsewhere (clipboard.md).
#[test]
fn paste_goes_to_the_pointer_or_the_centre_of_the_view() {
    let scene = chain();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let mut state = State::default();
    let key = keyboard::Key::Character("v".into());
    let paste = Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::KeyV),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::COMMAND,
        text: None,
        repeat: false,
    });
    let at = Point::new(100.0, 300.0);
    let events = drive(&canvas, &mut state, &paste, at);
    assert_eq!(
        events,
        vec![CanvasEvent::Paste {
            world: state.camera.to_world(at)
        }]
    );
    let events = drive(&canvas, &mut state, &paste, Point::new(900.0, 700.0));
    assert_eq!(
        events,
        vec![CanvasEvent::Paste {
            world: state.camera.to_world(Point::new(400.0, 300.0))
        }]
    );
    // While a pop-up has the keys, paste is the pop-up's.
    let blocked = PatchCanvas::new(&scene, &none, |e| e).interactive(false);
    assert!(drive(&blocked, &mut state, &paste, at).is_empty());
}

/// Shift pressed while a pop-up has the keys still extends the
/// selection on the first click after it closes.
#[test]
fn modifiers_stay_current_while_the_canvas_is_not_interactive() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let tile = scene.module(2).unwrap().bounds().center();
    let shift = Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::SHIFT));
    let mut state = State::default();

    let blocked = PatchCanvas::new(&scene, &selected, |e| e).interactive(false);
    assert!(drive(&blocked, &mut state, &shift, tile).is_empty());
    assert!(state.modifiers.shift());

    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let events = drive(&canvas, &mut state, &click(mouse::Button::Left), tile);
    assert!(events.contains(&CanvasEvent::Select(vec![1, 2])), "{events:?}");
}

/// Pointer down reports a press and does not reset on a second press.
/// The app turns the press into a flip or a hold.
#[test]
fn a_button_reports_press_and_a_second_press_is_not_a_reset() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    let first = drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    assert_eq!(first, vec![CanvasEvent::ButtonPressed { module: 1, widget: 0 }]);
    assert!(matches!(
        state.interaction,
        Interaction::Holding { module: 1, widget: 0 }
    ));

    let second = drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    assert_eq!(
        second,
        vec![
            CanvasEvent::ButtonReleased { module: 1, widget: 0 },
            CanvasEvent::ButtonPressed { module: 1, widget: 0 },
        ]
    );
    assert!(
        second
            .iter()
            .all(|event| !matches!(event, CanvasEvent::ResetControl { .. }))
    );
}

/// Release is reported even after the pointer has left the button.
/// Dragging does not move the tile.
#[test]
fn a_button_reports_release_outside() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let outside = Point::new(centre.x + 400.0, centre.y);
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();

    let down = drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    assert_eq!(down, vec![CanvasEvent::ButtonPressed { module: 1, widget: 0 }]);

    let moved = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::CursorMoved { position: outside }),
        outside,
    );
    assert!(moved.iter().all(|event| !matches!(event, CanvasEvent::Move { .. })));

    let up = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        outside,
    );
    assert_eq!(up, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
    assert!(matches!(state.interaction, Interaction::Idle));
}

/// Another mouse button ends the hold. Leaving the interaction as a pan
/// without a release would stick a momentary gate high.
#[test]
fn another_mouse_button_releases_a_hold() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    drive(&canvas, &mut state, &click(mouse::Button::Left), centre);

    let right = drive(&canvas, &mut state, &click(mouse::Button::Right), centre);
    assert_eq!(right, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
    assert!(matches!(state.interaction, Interaction::Panning { .. }));
}

/// Losing the window while a key or the pointer is held reports a release.
/// The key-up event never arrives after the window is unfocused.
#[test]
fn losing_the_window_releases_a_hold() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State {
        interaction: Interaction::Holding { module: 1, widget: 0 },
        key_holds: [
            Some(KeyHold {
                key: keyboard::key::Named::Space,
                module: 1,
                widget: 0,
            }),
            None,
        ],
        ..State::default()
    };
    let events = drive(&canvas, &mut state, &Event::Window(window::Event::Unfocused), centre);
    assert_eq!(events, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
    assert!(matches!(state.interaction, Interaction::Idle));
    assert!(state.key_holds.iter().all(Option::is_none));
}

fn key_event(named: keyboard::key::Named, pressed: bool) -> Event {
    let key = keyboard::Key::Named(named);
    let code = match named {
        keyboard::key::Named::Enter => keyboard::key::Code::Enter,
        _ => keyboard::key::Code::Space,
    };
    let physical_key = keyboard::key::Physical::Code(code);
    if pressed {
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key,
            physical_key,
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })
    } else {
        Event::Keyboard(keyboard::Event::KeyReleased {
            modified_key: key.clone(),
            key,
            physical_key,
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
        })
    }
}

/// Space on one button and Enter on another. Releasing one key releases
/// only the button that key was holding.
#[test]
fn each_key_releases_only_its_own_hold() {
    let mut module = button_module();
    module.panel.width = 12;
    module.panel.widgets.push(widget(WidgetViewKind::Button, 6));
    let scene = Scene::new(vec![module], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State {
        focus: Some((1, 0)),
        ..State::default()
    };
    let at = Point::new(20.0, 20.0);

    let space = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), at);
    assert_eq!(space, vec![CanvasEvent::ButtonPressed { module: 1, widget: 0 }]);

    state.focus = Some((1, 1));
    let enter = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Enter, true), at);
    assert_eq!(enter, vec![CanvasEvent::ButtonPressed { module: 1, widget: 1 }]);

    let up_space = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, false), at);
    assert_eq!(up_space, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);

    let up_enter = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Enter, false), at);
    assert_eq!(up_enter, vec![CanvasEvent::ButtonReleased { module: 1, widget: 1 }]);
}

/// Both keys can hold one button. Releasing the first leaves it held.
#[test]
fn releasing_one_key_keeps_a_button_the_other_key_holds() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State {
        focus: Some((1, 0)),
        ..State::default()
    };
    let at = Point::new(20.0, 20.0);
    drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), at);
    drive(&canvas, &mut state, &key_event(keyboard::key::Named::Enter, true), at);

    let up_space = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, false), at);
    assert!(up_space.is_empty());

    let up_enter = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Enter, false), at);
    assert_eq!(up_enter, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
}

/// A right-button release does not end a hold the left button started.
#[test]
fn releasing_another_mouse_button_keeps_the_left_hold() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    drive(&canvas, &mut state, &click(mouse::Button::Left), centre);

    let right = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
        centre,
    );
    assert!(right.is_empty());
    assert!(matches!(
        state.interaction,
        Interaction::Holding { module: 1, widget: 0 }
    ));
}

/// The gate stays down only when neither the pointer nor a key is holding.
#[test]
fn a_key_hold_survives_pointer_up_and_the_pointer_survives_a_key_up() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let centre = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State {
        focus: Some((1, 0)),
        ..State::default()
    };
    drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    drive(
        &canvas,
        &mut state,
        &key_event(keyboard::key::Named::Space, true),
        centre,
    );

    let pointer_up = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        centre,
    );
    assert!(pointer_up.is_empty());
    assert!(matches!(state.interaction, Interaction::Idle));

    state.interaction = Interaction::Holding { module: 1, widget: 0 };
    let key_up = drive(
        &canvas,
        &mut state,
        &key_event(keyboard::key::Named::Space, false),
        centre,
    );
    assert!(key_up.is_empty());
    assert!(matches!(
        state.interaction,
        Interaction::Holding { module: 1, widget: 0 }
    ));

    let pointer_up = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        centre,
    );
    assert_eq!(pointer_up, vec![CanvasEvent::ButtonReleased { module: 1, widget: 0 }]);
}

/// Space with no focused control asks the app to toggle Run. Enter does
/// not, a repeat does not, and a focused button still takes Space.
#[test]
fn space_with_no_focus_toggles_run() {
    let scene = Scene::new(vec![button_module()], Vec::new());
    let at = Point::new(10.0, 10.0);
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();

    let space = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), at);
    assert_eq!(space, vec![CanvasEvent::ToggleRun]);

    let mut repeat = key_event(keyboard::key::Named::Space, true);
    if let Event::Keyboard(keyboard::Event::KeyPressed { repeat, .. }) = &mut repeat {
        *repeat = true;
    }
    let again = drive(&canvas, &mut state, &repeat, at);
    assert!(again.is_empty());

    let enter = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Enter, true), at);
    assert!(enter.is_empty());

    state.focus = Some((1, 0));
    let focused = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), at);
    assert_eq!(focused, vec![CanvasEvent::ButtonPressed { module: 1, widget: 0 }]);
}

fn learning<'a>(scene: &'a Scene, selected: &'a BTreeSet<u32>, learn: &'a LearnView) -> PatchCanvas<'a, CanvasEvent> {
    PatchCanvas::new(scene, selected, |e| e).learn(Some(learn))
}

fn arrow_up() -> Event {
    let key = keyboard::Key::Named(keyboard::key::Named::ArrowUp);
    Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::ArrowUp),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    })
}

/// A key event with Shift held, for reverse focus and coarse movement.
fn shifted(named: keyboard::key::Named) -> Event {
    let mut event = key_event(named, true);
    if let Event::Keyboard(keyboard::Event::KeyPressed { modifiers, .. }) = &mut event {
        *modifiers = keyboard::Modifiers::SHIFT;
    }
    event
}

/// Tab follows reading order across tiles and skips lights within a panel.
#[test]
fn keyboard_focus_follows_tiles_and_controls() {
    let mut first = module(1, 300.0);
    first.panel = panel();
    first.position.y = 100.0;
    let mut second = module(2, 0.0);
    second.panel = panel();
    let scene = Scene::new(vec![first, second], Vec::new());
    let at = Point::new(700.0, 500.0);
    let mut state = State::default();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    assert_eq!(
        drive(&canvas, &mut state, &key_event(keyboard::key::Named::Tab, true), at),
        vec![CanvasEvent::Select(vec![2])]
    );
    assert_eq!(
        drive(&canvas, &mut state, &shifted(keyboard::key::Named::Tab), at),
        vec![CanvasEvent::Select(vec![1])],
        "Shift-Tab starts at the last tile"
    );
    let selected = BTreeSet::from([2]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    assert_eq!(
        drive(&canvas, &mut state, &key_event(keyboard::key::Named::Tab, true), at),
        vec![CanvasEvent::Select(vec![1])]
    );
    let first_selected = BTreeSet::from([1]);
    let first_canvas = PatchCanvas::new(&scene, &first_selected, |e| e);
    assert_eq!(
        drive(&first_canvas, &mut state, &shifted(keyboard::key::Named::Tab), at),
        vec![CanvasEvent::Select(vec![2])]
    );
    assert!(drive(&canvas, &mut state, &key_event(keyboard::key::Named::Enter, true), at).is_empty());
    assert_eq!(state.focus, Some((2, 0)));
    assert!(drive(&canvas, &mut state, &key_event(keyboard::key::Named::Tab, true), at).is_empty());
    assert_eq!(state.focus, Some((2, 2)), "the light is skipped");
    assert!(drive(&canvas, &mut state, &shifted(keyboard::key::Named::Tab), at).is_empty());
    assert_eq!(state.focus, Some((2, 0)));
    let toggle = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), at);
    assert_eq!(
        toggle,
        vec![CanvasEvent::ToggleRun],
        "a focused knob does not use Space"
    );
}

/// An unfocused selected tile moves by one grid cell, or ten with Shift.
/// The next key closes its pending arrow group before acting.
#[test]
fn keyboard_moves_selected_tile_and_closes_group_on_next_key() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    let at = Point::new(700.0, 500.0);
    let first = drive(&canvas, &mut state, &arrow_up(), at);
    assert_eq!(
        first,
        vec![
            CanvasEvent::GestureStart,
            CanvasEvent::Move {
                ids: vec![1],
                delta: Vector::new(0.0, -GRID)
            }
        ]
    );
    assert_eq!(
        drive(
            &canvas,
            &mut state,
            &key_event(keyboard::key::Named::ArrowUp, false),
            at
        ),
        vec![CanvasEvent::ArrowGroupPending]
    );
    let next = drive(&canvas, &mut state, &shifted(keyboard::key::Named::ArrowRight), at);
    assert_eq!(
        next,
        vec![
            CanvasEvent::ArrowGroupResumed,
            CanvasEvent::Move {
                ids: vec![1],
                delta: Vector::new(10.0 * GRID, 0.0)
            }
        ]
    );
    assert_eq!(
        drive(
            &canvas,
            &mut state,
            &key_event(keyboard::key::Named::ArrowRight, false),
            at
        ),
        vec![CanvasEvent::ArrowGroupPending]
    );
    assert_eq!(
        drive(&canvas, &mut state, &escape(), at),
        vec![CanvasEvent::ArrowGroupEnd, CanvasEvent::Select(Vec::new())]
    );
}

/// A pointer press replaces a pending arrow group before starting its
/// own interaction, so its eventual release cannot close the arrow step.
#[test]
fn pointer_press_ends_a_pending_arrow_group() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    let at = Point::new(700.0, 500.0);
    drive(&canvas, &mut state, &arrow_up(), at);
    drive(
        &canvas,
        &mut state,
        &key_event(keyboard::key::Named::ArrowUp, false),
        at,
    );
    let press = drive(&canvas, &mut state, &click(mouse::Button::Left), at);
    assert_eq!(press.first(), Some(&CanvasEvent::ArrowGroupEnd), "{press:?}");
    assert!(!state.arrow_gesture);
    assert!(state.arrow_deadline.is_none());
}

/// Keyboard navigation selects a tile, enters its control, edits it,
/// and leaves both focus levels without a pointer gesture (#101).
#[test]
fn keys_reach_and_leave_a_panel_control() {
    let mut first = module(1, 0.0);
    first.panel = panel();
    let scene = Scene::new(vec![first], Vec::new());
    let none = BTreeSet::new();
    let mut state = State::default();
    let at = Point::new(700.0, 500.0);
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    assert_eq!(
        drive(&canvas, &mut state, &key_event(keyboard::key::Named::Tab, true), at),
        vec![CanvasEvent::Select(vec![1])]
    );
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    assert!(drive(&canvas, &mut state, &key_event(keyboard::key::Named::Enter, true), at).is_empty());
    assert_eq!(state.focus, Some((1, 0)));
    let changed = drive(&canvas, &mut state, &arrow_up(), at);
    assert!(
        changed.iter().any(|e| matches!(
            e,
            CanvasEvent::SetControl {
                module: 1,
                widget: 0,
                ..
            }
        )),
        "{changed:?}"
    );
    drive(
        &canvas,
        &mut state,
        &key_event(keyboard::key::Named::ArrowUp, false),
        at,
    );
    assert_eq!(
        drive(&canvas, &mut state, &escape(), at),
        vec![CanvasEvent::ArrowGroupEnd]
    );
    assert_eq!(state.focus, None);
    assert_eq!(
        drive(&canvas, &mut state, &escape(), at),
        vec![CanvasEvent::Select(Vec::new())]
    );
}

/// In MIDI learn mode a click arms a knob or a button and takes the
/// keyboard focus. It never turns the knob or presses the button.
#[test]
fn a_learn_click_arms_and_focuses_without_changing_a_value() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let button = ModuleView {
        id: 2,
        position: Point::new(400.0, 0.0),
        ..button_module()
    };
    let scene = Scene::new(vec![m, button], Vec::new());
    let selected = BTreeSet::from([1]);
    let learn = LearnView::default();
    let canvas = learning(&scene, &selected, &learn);
    let mut state = State {
        focus: Some((1, 2)),
        tab_focus: true,
        ..State::default()
    };
    let knob = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    let events = drive(&canvas, &mut state, &click(mouse::Button::Left), knob);
    assert_eq!(events, vec![CanvasEvent::Learn { module: 1, widget: 0 }]);
    assert!(matches!(state.interaction, Interaction::Idle));
    assert_eq!(state.focus, Some((1, 0)));

    let button = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    let events = drive(&canvas, &mut state, &click(mouse::Button::Left), button);
    assert!(
        events.contains(&CanvasEvent::Learn { module: 2, widget: 0 }),
        "{events:?}"
    );
    assert!(
        events.iter().all(|e| !matches!(e, CanvasEvent::ButtonPressed { .. })),
        "{events:?}"
    );
}

/// In MIDI learn mode Space and Enter arm the focused control, arrows do
/// nothing, and Escape is left for the app, which ends the mode.
#[test]
fn learn_mode_keys_arm_and_never_change_a_value() {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let selected = BTreeSet::from([1]);
    let learn = LearnView::default();
    let canvas = learning(&scene, &selected, &learn);
    let at = Point::new(10.0, 10.0);
    let mut state = State {
        focus: Some((1, 0)),
        tab_focus: true,
        ..State::default()
    };
    for key in [keyboard::key::Named::Space, keyboard::key::Named::Enter] {
        let events = drive(&canvas, &mut state, &key_event(key, true), at);
        assert_eq!(events, vec![CanvasEvent::Learn { module: 1, widget: 0 }]);
    }
    assert!(drive(&canvas, &mut state, &arrow_up(), at).is_empty());

    let escape = {
        let key = keyboard::Key::Named(keyboard::key::Named::Escape);
        Event::Keyboard(keyboard::Event::KeyPressed {
            modified_key: key.clone(),
            key: key.clone(),
            physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
            location: keyboard::Location::Standard,
            modifiers: keyboard::Modifiers::default(),
            text: None,
            repeat: false,
        })
    };
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    canvas.handle(&mut state, &escape, bounds, mouse::Cursor::Available(at), &mut shell);
    assert_eq!(
        shell.event_status(),
        iced::event::Status::Ignored,
        "the app must see Escape"
    );
    assert_eq!(state.focus, None);

    let mut next_messages = Vec::new();
    let mut next_shell = Shell::new(&mut next_messages);
    canvas.handle(
        &mut state,
        &escape,
        bounds,
        mouse::Cursor::Available(at),
        &mut next_shell,
    );
    assert_eq!(
        next_shell.event_status(),
        iced::event::Status::Ignored,
        "learn mode must receive Escape even with a selected tile"
    );

    // With nothing focused, Space is still the Run shortcut.
    let events = drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), at);
    assert_eq!(events, vec![CanvasEvent::ToggleRun]);
}

/// Learn mode outlines controls on screen only.
#[test]
fn learn_outlines_cover_only_visible_tiles() {
    let mut near = module(1, 0.0);
    near.panel = panel();
    let far = ModuleView {
        panel: panel(),
        ..module(2, 5_000.0)
    };
    let scene = Scene::new(vec![near, far], Vec::new());
    let none = BTreeSet::new();
    let learn = LearnView::default();
    let canvas = learning(&scene, &none, &learn);
    let plain = canvas.overlay(&State::default()).shapes.len();
    let view = Rectangle::new(Point::new(-50.0, -50.0), Size::new(800.0, 600.0));
    // Two controls (a knob and a toggle) on the one visible tile.
    assert_eq!(canvas.overlay_in(&State::default(), view).shapes.len(), plain + 2);
}

fn knob_scene() -> (Scene, Point) {
    let mut m = module(1, 0.0);
    m.panel = panel();
    let scene = Scene::new(vec![m], Vec::new());
    let knob = scene.module(1).unwrap().widget_bounds(0).unwrap().center();
    (scene, knob)
}

fn moved(to: Point) -> Event {
    Event::Mouse(mouse::Event::CursorMoved { position: to })
}

/// A source tile with one output, and a tile with a knob, a light, and
/// a toggle further right.
fn assign_scene() -> Scene {
    let target = ModuleView {
        panel: panel(),
        ..module(2, 300.0)
    };
    Scene::new(vec![module(1, 0.0), target], Vec::new())
}

const OUT: PortRef = PortRef {
    module: 1,
    output: true,
    index: 0,
};

/// Drags from the source output to widget `widget` on tile 2 and lets
/// go there. Returns the events and the overlay shape count while over
/// the widget.
fn drop_on(canvas: &PatchCanvas<'_, CanvasEvent>, scene: &Scene, widget: usize) -> (Vec<CanvasEvent>, usize) {
    let mut state = State::default();
    let start = scene.port_position(OUT).unwrap();
    drive(canvas, &mut state, &click(mouse::Button::Left), start);
    let over = scene.module(2).unwrap().widget_bounds(widget).unwrap().center();
    drive(canvas, &mut state, &moved(over), over);
    let shapes = canvas.overlay(&state).shapes.len();
    (drive(canvas, &mut state, &released(), over), shapes)
}

/// Dropping an output on a knob or toggle assigns it, and the control is
/// highlighted while the drag is over it. A light takes nothing
/// (modulation.md, Making an assignment).
#[test]
fn dropping_an_output_on_a_control_assigns_it() {
    let scene = assign_scene();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let (events, on_knob) = drop_on(&canvas, &scene, 0);
    assert_eq!(
        events,
        vec![
            CanvasEvent::Assign {
                from: OUT,
                module: 2,
                widget: 0
            },
            CanvasEvent::AssignCancel,
            CanvasEvent::GestureEnd
        ]
    );
    let (events, _) = drop_on(&canvas, &scene, 2);
    assert!(events.contains(&CanvasEvent::Assign {
        from: OUT,
        module: 2,
        widget: 2
    }));
    let (events, on_light) = drop_on(&canvas, &scene, 1);
    assert_eq!(events, vec![CanvasEvent::AssignCancel, CanvasEvent::GestureEnd]);
    assert_eq!(
        on_knob,
        on_light + 1,
        "only a control that takes the drop is highlighted"
    );
}

/// In MIDI learn mode a drop on a control makes no assignment, and
/// nothing is highlighted.
#[test]
fn learn_mode_takes_no_assignment_drop() {
    let scene = assign_scene();
    let none = BTreeSet::new();
    let learn = LearnView::default();
    let canvas = learning(&scene, &none, &learn);
    let (events, _) = drop_on(&canvas, &scene, 0);
    assert_eq!(events, vec![CanvasEvent::AssignCancel, CanvasEvent::GestureEnd]);
}

/// The assign scene with one assignment from the source output to the
/// knob on tile 2, and the knob showing its range.
fn modulated_scene() -> (Scene, PanelValues) {
    let mut source = module(1, 0.0);
    source.outputs[0].badge = 1;
    let target = ModuleView {
        panel: panel(),
        ..module(2, 300.0)
    };
    let scene = Scene::new(vec![source, target], Vec::new()).with_modulations(vec![crate::scene::ModulationView {
        id: 1,
        from: (1, 0),
        to: 2,
        widget: Some(0),
        enabled: true,
    }]);
    let mut values = PanelValues::new();
    values.set(
        2,
        0,
        crate::controls::WidgetValue {
            position: 0.5,
            modulation: crate::controls::ModulationMark::Range { low: 0.5, high: 0.75 },
            ..Default::default()
        },
    );
    (scene, values)
}

/// #249: an output's hover text is placed above its badge as well as
/// its port, so the box never covers the count. An input's stays on
/// its port.
#[test]
fn output_hover_text_goes_above_its_badge() {
    let (scene, _) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let out = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let badge = scene.badge_bounds(1, 0).unwrap();
    let (_, _, _, anchor) = canvas.hover_content(HoverTarget::Port(out)).unwrap();
    assert!(
        anchor.y <= badge.y && anchor.x + anchor.width >= badge.x + badge.width,
        "{anchor:?}"
    );
    let input = PortRef { output: false, ..out };
    let (_, _, _, anchor) = canvas.hover_content(HoverTarget::Port(input)).unwrap();
    assert!(anchor.width <= 2.0 * PORT_RADIUS + 1e-3, "{anchor:?}");
}

fn escape() -> Event {
    let key = keyboard::Key::Named(keyboard::key::Named::Escape);
    Event::Keyboard(keyboard::Event::KeyPressed {
        modified_key: key.clone(),
        key,
        physical_key: keyboard::key::Physical::Code(keyboard::key::Code::Escape),
        location: keyboard::Location::Standard,
        modifiers: keyboard::Modifiers::default(),
        text: None,
        repeat: false,
    })
}

/// A still image starts with the pointer, pin, drag, and hover text it
/// asks for, all in the first frame (`loom-app --screenshot`).
#[test]
fn a_still_seeds_pointer_pin_drag_and_hover_text() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let knob = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    let still = Still {
        pointer: Some(knob),
        pinned: Some(OUT),
        hover_text: true,
        ..Still::default()
    };
    let canvas = PatchCanvas::new(&scene, &none, |e| e).values(&values).still(still);
    let state = canvas.still_state(still);
    assert_eq!(state.hover, Some(Hit::Widget { module: 2, widget: 0 }));
    assert_eq!(state.pinned, Some(OUT));
    assert_eq!(state.hover_text.shown(), Some(HoverTarget::Control(2, 0)));
    assert_eq!(canvas.overlay(&state).cables.len(), 1, "the modulation line");

    // Wiring from the output to the knob: the loose cable and the drop
    // highlight, and no hover text during a drag.
    let wiring = Still {
        drag: Some(StillDrag::Wire {
            from: OUT,
            pulled: false,
        }),
        pinned: None,
        ..still
    };
    let state = canvas.still_state(wiring);
    assert!(matches!(
        state.interaction,
        Interaction::Wiring {
            from: OUT,
            pulled: false
        }
    ));
    assert_eq!(state.hover_text.shown(), None);
    let overlay = canvas.overlay(&state);
    assert_eq!(overlay.cables.len(), 2, "the line and the loose cable");
    let resting = canvas.overlay(&canvas.still_state(Still { drag: None, ..wiring }));
    assert_eq!(overlay.shapes.len(), resting.shapes.len() + 1, "the drop highlight");

    // A cable pulled off an input assigns nothing on a drop, so the
    // control is not highlighted.
    let pulled = canvas.overlay(&canvas.still_state(Still {
        drag: Some(StillDrag::Wire {
            from: OUT,
            pulled: true,
        }),
        ..wiring
    }));
    assert_eq!(pulled.shapes.len(), resting.shapes.len());
}

/// Lines show while the pointer is on the source output, its badge, or
/// the modulated control, and not elsewhere. A badge click pins them;
/// a second click or Escape unpins, and Escape is used up only then.
#[test]
fn modulation_lines_follow_hover_and_the_pinned_badge() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e).values(&values);
    let lines = |state: &State| canvas.overlay(state).cables.len();
    let badge = scene.badge_bounds(1, 0).unwrap().center();
    assert_eq!(scene.hit(badge, PORT_HIT_RADIUS), Hit::Badge(OUT));
    let knob = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    let port = scene.port_position(OUT).unwrap();
    let empty = Point::new(200.0, 400.0);

    let mut state = State::default();
    for (at, shown) in [(port, 1), (badge, 1), (knob, 1), (empty, 0)] {
        drive(&canvas, &mut state, &moved(at), at);
        assert_eq!(lines(&state), shown, "pointer at {at:?}");
    }

    drive(&canvas, &mut state, &moved(badge), badge);
    assert!(drive(&canvas, &mut state, &click(mouse::Button::Left), badge).is_empty());
    drive(&canvas, &mut state, &released(), badge);
    drive(&canvas, &mut state, &moved(empty), empty);
    assert_eq!(state.pinned, Some(OUT));
    assert_eq!(lines(&state), 1, "pinned lines stay");

    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    canvas.handle(
        &mut state,
        &escape(),
        bounds,
        mouse::Cursor::Available(empty),
        &mut shell,
    );
    assert_eq!(shell.event_status(), iced::event::Status::Captured);
    assert_eq!(state.pinned, None);
    assert_eq!(lines(&state), 0);
    let mut shell = Shell::new(&mut messages);
    canvas.handle(
        &mut state,
        &escape(),
        bounds,
        mouse::Cursor::Available(empty),
        &mut shell,
    );
    assert_eq!(
        shell.event_status(),
        iced::event::Status::Ignored,
        "with nothing pinned the app sees Escape"
    );

    // Two clicks on the badge pin and unpin.
    drive(&canvas, &mut state, &click(mouse::Button::Left), badge);
    drive(&canvas, &mut state, &click(mouse::Button::Left), badge);
    assert_eq!(state.pinned, None);
}

/// A pointed source has two rings and its target one; selection alone
/// still draws lines without outlines (#407).
#[test]
fn modulation_outlines_show_direction_only_for_pointing() {
    let (scene, _) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let mut pointed = Overlay::default();
    canvas.build_modulation_lines(
        &State {
            hover: Some(Hit::Port(OUT)),
            ..State::default()
        },
        &mut pointed,
    );
    assert_eq!(pointed.cables.len(), 1);
    assert_eq!(pointed.shapes.len(), 3, "two source rings and one target ring");

    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut selected_only = Overlay::default();
    canvas.build_modulation_lines(&State::default(), &mut selected_only);
    assert_eq!(selected_only.cables.len(), 1);
    assert!(selected_only.shapes.is_empty());
}

/// Turning an assignment off keeps its dashed line and direction rings,
/// while the line uses the disabled style (#197).
#[test]
fn disabled_assignment_keeps_a_visible_line() {
    let (mut scene, _) = modulated_scene();
    scene.set_modulation_enabled(1, false);
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let mut overlay = Overlay::default();
    canvas.build_modulation_lines(
        &State {
            hover: Some(Hit::Port(OUT)),
            ..State::default()
        },
        &mut overlay,
    );
    assert_eq!(overlay.cables.len(), 1);
    assert_eq!(overlay.shapes.len(), 3);
}

/// Toggling one assignment updates its line cache without changing
/// the scene generation used for tiles and cables (#197).
#[test]
fn assignment_state_updates_only_modulation_overlay() {
    let (mut scene, _) = modulated_scene();
    let geometry = scene.generation();
    let lines = scene.overlay_generation();
    scene.set_modulation_enabled(1, false);
    assert_eq!(scene.generation(), geometry);
    assert_ne!(scene.overlay_generation(), lines);
    assert!(!scene.modulations()[0].enabled);
}

/// A drag on the ring around a modulated knob changes depth, up to
/// raise it, as one gesture, and carries the pinned output. A drag on
/// the knob body still turns it.
#[test]
fn dragging_the_range_ring_changes_depth() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e).values(&values);
    let rect = scene.module(2).unwrap().widget_bounds(0).unwrap();
    let centre = crate::controls::knob_centre(rect);
    let ring = Point::new(centre.x, centre.y - 12.0);

    let mut state = State {
        pinned: Some(OUT),
        ..State::default()
    };
    let down = drive(&canvas, &mut state, &click(mouse::Button::Left), ring);
    assert!(down.contains(&CanvasEvent::GestureStart), "{down:?}");
    // The selection stays: a selected source picks the assignment.
    assert!(!down.iter().any(|e| matches!(e, CanvasEvent::Select(_))), "{down:?}");
    let up = Point::new(ring.x, ring.y - 20.0);
    let moved_events = drive(&canvas, &mut state, &moved(up), up);
    let [
        CanvasEvent::DragDepth {
            module: 2,
            widget: 0,
            pinned: Some(pinned),
            delta,
        },
    ] = moved_events.as_slice()
    else {
        panic!("{moved_events:?}");
    };
    assert_eq!(*pinned, OUT);
    assert!((delta - 0.1).abs() < 1e-6, "{delta}");
    assert_eq!(
        drive(&canvas, &mut state, &released(), up),
        vec![CanvasEvent::GestureEnd]
    );

    let mut state = State::default();
    drive(&canvas, &mut state, &click(mouse::Button::Left), centre);
    let up = Point::new(centre.x, centre.y - 20.0);
    let turned = drive(&canvas, &mut state, &moved(up), up);
    assert!(
        matches!(turned.as_slice(), [CanvasEvent::SetControl { .. }]),
        "{turned:?}"
    );
}

/// A pin whose badge is gone (its last assignment removed) counts as
/// none: Escape goes to the app, and an arc drag carries no pin.
#[test]
fn a_pin_without_its_badge_takes_no_escape() {
    let (mut scene, values) = modulated_scene();
    let modules = scene.modules().to_vec();
    let mut source = modules[0].clone();
    source.outputs[0].badge = 0;
    scene = Scene::new(vec![source, modules[1].clone()], Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e).values(&values);
    let mut state = State {
        pinned: Some(OUT),
        ..State::default()
    };
    assert_eq!(canvas.pin(&state), None);
    let mut messages = Vec::new();
    let mut shell = Shell::new(&mut messages);
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    canvas.handle(
        &mut state,
        &escape(),
        bounds,
        mouse::Cursor::Available(Point::ORIGIN),
        &mut shell,
    );
    assert_eq!(shell.event_status(), iced::event::Status::Ignored);
}

/// A new patch drops the pin and old arrow timer. Module IDs repeat
/// between patches, and the new app has a fresh undo history.
#[test]
fn a_new_patch_clears_old_canvas_state() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e).values(&values).frame_key(2);
    let mut state = State {
        pinned: Some(OUT),
        framed_key: Some(1),
        arrow_gesture: true,
        arrow_deadline: Some(Instant::now() - Duration::from_millis(1)),
        ..State::default()
    };
    let redraw = Event::Window(window::Event::RedrawRequested(Instant::now()));
    let events = drive(&canvas, &mut state, &redraw, Point::ORIGIN);
    assert!(!events.contains(&CanvasEvent::ArrowGroupEnd), "{events:?}");
    assert!(!state.arrow_gesture);
    assert!(state.arrow_deadline.is_none());
    assert_eq!(state.pinned, None);
}

/// The first frame has no old patch key, so it keeps a key gesture that
/// began before that redraw.
#[test]
fn first_frame_keeps_a_current_arrow_group() {
    let scene = Scene::new(vec![module(1, 0.0)], Vec::new());
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e).frame_key(2);
    let now = Instant::now();
    let mut state = State {
        arrow_gesture: true,
        arrow_deadline: Some(now + Duration::from_millis(100)),
        ..State::default()
    };
    let events = drive(
        &canvas,
        &mut state,
        &Event::Window(window::Event::RedrawRequested(now)),
        Point::ORIGIN,
    );
    assert!(!events.contains(&CanvasEvent::ArrowGroupEnd), "{events:?}");
    assert!(state.arrow_gesture);
    assert!(state.arrow_deadline.is_some());
}

/// The lines are built once for a given pin, pointer, and selection,
/// and reused while those stay the same.
#[test]
fn modulation_lines_are_reused_until_their_inputs_change() {
    let (scene, values) = modulated_scene();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e).values(&values);
    let state = State::default();
    let first = canvas.overlay(&state).cables.len();
    let key = state.lines.borrow().as_ref().map(|(k, _)| k.clone());
    assert_eq!(canvas.overlay(&state).cables.len(), first);
    assert_eq!(state.lines.borrow().as_ref().map(|(k, _)| k.clone()), key);
    assert_eq!(first, 1, "the selected source's line");
}

/// A matrix-selected row draws its line with no hover, canvas selection,
/// or badge pin, and leaves the pin state alone.
#[test]
fn matrix_row_shows_one_line_without_pinning() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let plain = PatchCanvas::new(&scene, &none, |e| e).values(&values);
    assert!(plain.overlay(&State::default()).cables.is_empty());
    let line = scene.modulations()[0].clone();
    let canvas = PatchCanvas::new(&scene, &none, |e| e)
        .values(&values)
        .matrix_line(Some(line));
    let state = State::default();
    assert_eq!(canvas.overlay(&state).cables.len(), 1);
    assert_eq!(state.pinned, None);
}

/// A matrix line adds to lines shown by a pinned or hovered output.
#[test]
fn matrix_line_preserves_pinned_and_hovered_lines() {
    let (original, values) = modulated_scene();
    let mut source = module(3, 600.0);
    source.outputs[0].badge = 1;
    let modules = original.modules();
    let selected = original.modulations()[0].clone();
    let pin = PortRef {
        module: 3,
        output: true,
        index: 0,
    };
    let scene = Scene::new(vec![modules[0].clone(), modules[1].clone(), source], Vec::new()).with_modulations(vec![
        selected.clone(),
        crate::scene::ModulationView {
            id: 2,
            from: (3, 0),
            to: 2,
            widget: Some(0),
            enabled: true,
        },
    ]);
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e)
        .values(&values)
        .matrix_line(Some(selected));
    let mut state = State {
        pinned: Some(pin),
        ..State::default()
    };
    assert_eq!(canvas.overlay(&state).cables.len(), 2);
    state.pinned = None;
    state.hover = Some(Hit::Port(pin));
    assert_eq!(canvas.overlay(&state).cables.len(), 2);
}

/// Escape can clear a canvas pin while matrix keyboard focus owns the key.
#[test]
fn matrix_escape_clears_canvas_pin() {
    let (scene, values) = modulated_scene();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e)
        .values(&values)
        .matrix_keyboard(true)
        .matrix_escape(true);
    let mut state = State {
        pinned: Some(OUT),
        ..State::default()
    };
    drive(&canvas, &mut state, &escape(), Point::ORIGIN);
    assert_eq!(state.pinned, None);
}

/// Matrix keyboard focus leaves canvas clicks active but owns ordinary keys.
#[test]
fn matrix_keyboard_focus_keeps_canvas_pointer_active() {
    let scene = chain();
    let selected = BTreeSet::from([1]);
    let tile = scene.module(2).unwrap().bounds().center();
    let canvas = PatchCanvas::new(&scene, &selected, |e| e).matrix_keyboard(true);
    let mut state = State::default();

    assert!(drive(&canvas, &mut state, &key_event(keyboard::key::Named::Space, true), tile).is_empty());
    let events = drive(&canvas, &mut state, &click(mouse::Button::Left), tile);
    assert!(events.contains(&CanvasEvent::Select(vec![2])), "{events:?}");
}

/// A knob whose arc has zero length shows the dot, and its ring still
/// takes a depth drag, so depth 0 can be dragged back out of.
#[test]
fn a_zero_length_arc_still_takes_a_depth_drag() {
    let (scene, _) = modulated_scene();
    let mut values = PanelValues::new();
    values.set(
        2,
        0,
        crate::controls::WidgetValue {
            modulation: crate::controls::ModulationMark::Dot,
            ..Default::default()
        },
    );
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e).values(&values);
    let centre = crate::controls::knob_centre(scene.module(2).unwrap().widget_bounds(0).unwrap());
    let mut state = State::default();
    drive(
        &canvas,
        &mut state,
        &click(mouse::Button::Left),
        Point::new(centre.x + 12.0, centre.y),
    );
    assert!(matches!(state.interaction, Interaction::Depth { .. }));
}

/// Just above a modulated knob's cell, on its ring, the pointer shows
/// the depth drag cursor. Over an unmodulated knob there, and in MIDI
/// learn mode, it shows the tile's grab cursor, as a press there grabs
/// the tile (#198).
#[test]
fn the_range_ring_shows_the_resize_cursor_above_the_cell() {
    let (scene, values) = modulated_scene();
    let rect = scene.module(2).unwrap().widget_bounds(0).unwrap();
    // On the ring, just above the cell: the hit test finds the tile.
    let (centre, distance) = crate::controls::knob_range_ring(rect);
    let above = Point::new(centre.x, centre.y - distance);
    assert!(above.y < rect.y, "the point should be above the cell");
    let none = BTreeSet::new();
    let cursor_at = |canvas: &PatchCanvas<'_, CanvasEvent>| {
        let mut state = State::default();
        drive(canvas, &mut state, &moved(above), above);
        assert!(matches!(state.hover, Some(Hit::Module(2))), "{:?}", state.hover);
        canvas.hover_interaction(&state)
    };

    let modulated = PatchCanvas::new(&scene, &none, |e| e).values(&values);
    assert_eq!(cursor_at(&modulated), mouse::Interaction::ResizingVertically);

    let plain_values = PanelValues::new();
    let plain = PatchCanvas::new(&scene, &none, |e| e).values(&plain_values);
    assert_eq!(cursor_at(&plain), mouse::Interaction::Grab);

    let learn = LearnView::default();
    let learning = PatchCanvas::new(&scene, &none, |e| e)
        .values(&values)
        .learn(Some(&learn));
    assert_eq!(cursor_at(&learning), mouse::Interaction::Grab);
}

/// A knob with no enabled assignment has no ring to grab: a press there
/// turns the knob.
#[test]
fn an_unmodulated_knob_has_no_range_ring() {
    let (scene, _) = modulated_scene();
    let values = PanelValues::new();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e).values(&values);
    let centre = crate::controls::knob_centre(scene.module(2).unwrap().widget_bounds(0).unwrap());
    // Beside the knob, inside its cell, where the ring would be.
    let ring = Point::new(centre.x + 12.0, centre.y);
    let mut state = State::default();
    drive(&canvas, &mut state, &click(mouse::Button::Left), ring);
    assert!(matches!(state.interaction, Interaction::Turning { .. }));
}

/// A cable pulled off a connected input and dropped on a control is
/// discarded. It does not become an assignment.
#[test]
fn a_pulled_cable_dropped_on_a_control_does_not_assign() {
    let cable = CableView {
        from: (1, 0),
        to: (2, 0),
        colour_from: Color::WHITE,
        colour_to: Color::WHITE,
        delayed: false,
    };
    let target = ModuleView {
        panel: panel(),
        ..module(2, 300.0)
    };
    let scene = Scene::new(vec![module(1, 0.0), target], vec![cable]);
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let mut state = State::default();
    let input = PortRef {
        module: 2,
        output: false,
        index: 0,
    };
    let start = scene.port_position(input).unwrap();
    let down = drive(&canvas, &mut state, &click(mouse::Button::Left), start);
    assert!(down.contains(&CanvasEvent::Disconnect(input)), "{down:?}");
    let knob = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    drive(&canvas, &mut state, &moved(knob), knob);
    let up = drive(&canvas, &mut state, &released(), knob);
    assert_eq!(up, vec![CanvasEvent::GestureEnd]);
}

/// A drag that starts on an input never assigns.
#[test]
fn an_input_drag_does_not_assign() {
    let scene = assign_scene();
    let none = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &none, |e| e);
    let mut state = State::default();
    let input = PortRef { output: false, ..OUT };
    let start = scene.port_position(input).unwrap();
    drive(&canvas, &mut state, &click(mouse::Button::Left), start);
    let knob = scene.module(2).unwrap().widget_bounds(0).unwrap().center();
    drive(&canvas, &mut state, &moved(knob), knob);
    assert_eq!(
        drive(&canvas, &mut state, &released(), knob),
        vec![CanvasEvent::GestureEnd]
    );
}

fn released() -> Event {
    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
}

/// A knob drag is one gesture: start at pointer down, end at pointer up
/// (undo.md, Gestures).
#[test]
fn a_knob_drag_is_bracketed_by_a_gesture_start_and_end() {
    let (scene, knob) = knob_scene();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    let down = drive(&canvas, &mut state, &click(mouse::Button::Left), knob);
    assert_eq!(down, vec![CanvasEvent::GestureStart]);
    let up = Point::new(knob.x, knob.y - 20.0);
    let during = drive(&canvas, &mut state, &moved(up), up);
    assert!(
        matches!(during.as_slice(), [CanvasEvent::SetControl { .. }]),
        "{during:?}"
    );
    let end = drive(&canvas, &mut state, &released(), up);
    assert_eq!(end, vec![CanvasEvent::GestureEnd]);
}

/// A drag cut off by the window losing focus still ends its gesture, once.
#[test]
fn losing_the_window_ends_a_drag_gesture() {
    let (scene, knob) = knob_scene();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    drive(&canvas, &mut state, &click(mouse::Button::Left), knob);
    let lost = drive(&canvas, &mut state, &Event::Window(window::Event::Unfocused), knob);
    assert_eq!(lost, vec![CanvasEvent::GestureEnd]);
    let later = drive(&canvas, &mut state, &released(), knob);
    assert!(later.is_empty(), "{later:?}");
}

/// A second button pressed in a drag replaces the interaction, and must
/// end the gesture first.
#[test]
fn another_button_during_a_drag_ends_the_gesture() {
    let (scene, knob) = knob_scene();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State::default();
    drive(&canvas, &mut state, &click(mouse::Button::Left), knob);
    let events = drive(&canvas, &mut state, &click(mouse::Button::Right), knob);
    assert_eq!(events, vec![CanvasEvent::GestureEnd]);
}

/// A held arrow key repeats. The whole hold, key down to key up, is one
/// gesture.
#[test]
fn a_held_arrow_key_is_one_gesture() {
    let (scene, knob) = knob_scene();
    let selected = BTreeSet::from([1]);
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let mut state = State {
        focus: Some((1, 0)),
        ..State::default()
    };
    let first = drive(&canvas, &mut state, &arrow_up(), knob);
    assert_eq!(first.first(), Some(&CanvasEvent::GestureStart), "{first:?}");
    let Event::Keyboard(keyboard::Event::KeyPressed {
        key,
        modified_key,
        physical_key,
        location,
        modifiers,
        text,
        ..
    }) = arrow_up()
    else {
        unreachable!()
    };
    let again = Event::Keyboard(keyboard::Event::KeyPressed {
        key: key.clone(),
        modified_key,
        physical_key,
        location,
        modifiers,
        text,
        repeat: true,
    });
    let repeat = drive(&canvas, &mut state, &again, knob);
    assert!(
        repeat.iter().all(|e| !matches!(e, CanvasEvent::GestureStart)),
        "{repeat:?}"
    );
    let up = Event::Keyboard(keyboard::Event::KeyReleased {
        modified_key: key.clone(),
        key,
        physical_key,
        location: keyboard::Location::Standard,
        modifiers,
    });
    let end = drive(&canvas, &mut state, &up, knob);
    assert_eq!(end, vec![CanvasEvent::ArrowGroupPending]);
    let quick = drive(&canvas, &mut state, &arrow_up(), knob);
    assert!(
        quick.iter().all(|e| !matches!(e, CanvasEvent::GestureStart)),
        "{quick:?}"
    );
    assert_eq!(quick.first(), Some(&CanvasEvent::ArrowGroupResumed));
    assert_eq!(
        drive(&canvas, &mut state, &up, knob),
        vec![CanvasEvent::ArrowGroupPending]
    );
    let due = state.arrow_deadline.expect("a release schedules the end");
    let end = drive(
        &canvas,
        &mut state,
        &Event::Window(window::Event::RedrawRequested(due)),
        knob,
    );
    assert!(end.contains(&CanvasEvent::ArrowGroupEnd), "{end:?}");
}

/// A right press and release at `at`, with no move in between.
fn right_click(canvas: &PatchCanvas<'_, CanvasEvent>, state: &mut State, at: Point) -> Vec<CanvasEvent> {
    let mut events = drive(canvas, state, &click(mouse::Button::Right), at);
    events.extend(drive(
        canvas,
        state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
        at,
    ));
    events
}

/// Right-clicking an output opens the probe menu at that output
/// (panels.md, Add scope and Add meter).
#[test]
fn right_click_on_an_output_opens_the_probe_menu() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let output = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let port = scene.port_position(output).unwrap();
    let events = right_click(&canvas, &mut State::default(), port);
    assert_eq!(
        events,
        vec![CanvasEvent::ProbeMenu {
            output,
            world: port,
            at: port,
            area: Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0)),
        }]
    );
}

/// Right-clicking a cable opens the menu for the cable's source, at the
/// point clicked.
#[test]
fn right_click_on_a_cable_opens_the_menu_for_its_source() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let from = scene.port_position(PortRef {
        module: 2,
        output: true,
        index: 0,
    });
    let to = scene.port_position(PortRef {
        module: 3,
        output: false,
        index: 0,
    });
    let [a, b, c, d] = crate::scene::cable_curve(from.unwrap(), to.unwrap());
    let middle = Point::new(
        (a.x + 3.0 * b.x + 3.0 * c.x + d.x) / 8.0,
        (a.y + 3.0 * b.y + 3.0 * c.y + d.y) / 8.0,
    );
    let events = right_click(&canvas, &mut State::default(), middle);
    assert!(
        matches!(
            events.as_slice(),
            [CanvasEvent::ProbeMenu { output: PortRef { module: 2, output: true, index: 0 }, world, .. }]
                if *world == middle
        ),
        "{events:?}"
    );
}

/// A right-drag pans and opens no menu, even if it ends where it began.
#[test]
fn right_drag_pans_and_opens_no_menu() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let port = scene
        .port_position(PortRef {
            module: 1,
            output: true,
            index: 0,
        })
        .unwrap();
    let mut state = State::default();
    let mut events = drive(&canvas, &mut state, &click(mouse::Button::Right), port);
    let away = port + Vector::new(30.0, 0.0);
    let moved = Event::Mouse(mouse::Event::CursorMoved { position: away });
    events.extend(drive(&canvas, &mut state, &moved, away));
    let back = Event::Mouse(mouse::Event::CursorMoved { position: port });
    events.extend(drive(&canvas, &mut state, &back, port));
    events.extend(drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
        port,
    ));
    assert!(events.is_empty(), "{events:?}");
}

/// Letting go of another button during a right click opens no menu and
/// keeps the click, so the right release still opens it.
#[test]
fn only_the_right_release_opens_the_menu() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let port = scene
        .port_position(PortRef {
            module: 1,
            output: true,
            index: 0,
        })
        .unwrap();
    let mut state = State::default();
    let pressed = drive(&canvas, &mut state, &click(mouse::Button::Right), port);
    let left = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        port,
    );
    assert!(pressed.is_empty() && left.is_empty(), "{left:?}");
    let right = drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)),
        port,
    );
    assert!(matches!(right.as_slice(), [CanvasEvent::ProbeMenu { .. }]), "{right:?}");
}

/// A tile opens its menu; an input, empty canvas, and middle click do not.
#[test]
fn right_click_elsewhere_opens_no_menu() {
    let scene = chain();
    let selected = BTreeSet::new();
    let canvas = PatchCanvas::new(&scene, &selected, |e| e);
    let input = scene
        .port_position(PortRef {
            module: 2,
            output: false,
            index: 0,
        })
        .unwrap();
    let tile = scene.module(1).unwrap().bounds().center();
    assert!(matches!(
        right_click(&canvas, &mut State::default(), tile).as_slice(),
        [CanvasEvent::TileMenu { module: 1, .. }]
    ));
    for at in [input, Point::new(300.0, 500.0)] {
        let events = right_click(&canvas, &mut State::default(), at);
        assert!(events.is_empty(), "{at:?}: {events:?}");
    }
    let output = scene
        .port_position(PortRef {
            module: 1,
            output: true,
            index: 0,
        })
        .unwrap();
    let mut state = State::default();
    let mut events = drive(&canvas, &mut state, &click(mouse::Button::Middle), output);
    events.extend(drive(
        &canvas,
        &mut state,
        &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Middle)),
        output,
    ));
    assert!(events.is_empty(), "{events:?}");
}
