use super::*;
use crate::scene::{HoverText, PanelView, PortShape, PortView, Scene, WidgetView};
use iced::Color;

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

fn widget(kind: WidgetViewKind, x: u8, y: u8, label: &str) -> WidgetView {
    WidgetView {
        kind,
        x,
        y,
        label: label.into(),
        hover: HoverText::default(),
        hidden: false,
    }
}

fn module() -> ModuleView {
    ModuleView {
        size: None,
        unknown: false,
        id: 1,
        title: "Filter".into(),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position: Point::new(24.0, 36.0),
        inputs: vec![port("In"), port("Cutoff")],
        outputs: vec![port("Lowpass"), port("Bandpass")],
        panel: PanelView {
            width: 12,
            height: 8,
            widgets: vec![
                widget(WidgetViewKind::Knob { steps: 0 }, 0, 0, "Cutoff"),
                widget(WidgetViewKind::Knob { steps: 0 }, 3, 0, "Resonance"),
                widget(WidgetViewKind::Light, 6, 0, "level"),
                widget(WidgetViewKind::Toggle, 0, 3, "Drive"),
                widget(
                    WidgetViewKind::Radio {
                        options: vec!["12".into(), "24".into()],
                    },
                    3,
                    3,
                    "Slope",
                ),
                widget(WidgetViewKind::Meter, 9, 2, "Out"),
            ],
        },
    }
}

fn items(module: &ModuleView) -> Vec<TextItem<'_>> {
    let mut items = Vec::new();
    for_each(module, |item| items.push(item));
    items
}

/// Port labels and their hover bounds follow the markers on resized tiles.
#[test]
fn resized_port_labels_follow_port_positions() {
    let mut module = module();
    module.size = Some((40, 30));
    for item in items(&module) {
        if let TextKind::Port(port) = item.kind {
            let point = if port.output {
                module.output_position(port.index)
            } else {
                module.input_position(port.index)
            };
            assert_eq!(item.anchor.y, point.y);
            let scene = Scene::new(vec![module.clone()], Vec::new());
            assert_eq!(port_label_at(&scene, item.bounds().center()), Some(port));
        }
    }
}

/// A caption that fits its widget is centred on it. A longer one may
/// move toward empty space, but its centre stays over its widget.
#[test]
fn captions_are_centred_on_their_widgets() {
    // #92: captions were anchored at the widget's left edge, and Iced
    // centred them on it.
    let module = module();
    for item in items(&module) {
        if let TextKind::Caption(index) = item.kind {
            let widget = module.widget_bounds(index).unwrap();
            let text = item.bounds();
            if text_width(&item.content, LABEL_SIZE) <= widget.width {
                assert!(
                    (text.center_x() - widget.center_x()).abs() < 1e-3,
                    "{}: text centre {} vs widget centre {}",
                    item.content,
                    text.center_x(),
                    widget.center_x()
                );
            } else {
                assert!(
                    (widget.x..=widget.x + widget.width).contains(&text.center_x()),
                    "{} moved off its widget",
                    item.content
                );
            }
        }
    }
}

/// A caption with empty space beside it grows into it instead of being
/// cut (#455). Resonance has a light on its right, whose caption is on
/// another line, and Cutoff's caption on its left, so it moves a little
/// right, toward the space, and stays over its knob.
#[test]
fn a_caption_grows_into_empty_space() {
    let module = module();
    let resonance = items(&module)
        .into_iter()
        .find(|item| item.kind == TextKind::Caption(1))
        .expect("the Resonance caption");
    let knob = module.widget_bounds(1).unwrap();
    assert_eq!(resonance.content, "Resonance");
    assert!(resonance.bounds().width > knob.width, "wider than its knob");
    assert!(resonance.anchor.x > knob.center_x(), "moved toward the space");
    assert!(resonance.anchor.x < knob.x + knob.width, "still over its knob");
}

#[test]
fn every_text_stays_inside_its_tile() {
    let module = module();
    let tile = module.bounds();
    for item in items(&module) {
        let text = item.bounds();
        assert!(
            text.x >= tile.x
                && text.x + text.width <= tile.x + tile.width
                && text.y >= tile.y
                && text.y + text.height <= tile.y + tile.height,
            "{:?} {:?} spills out of {tile:?}: {text:?}",
            item.kind,
            item.content
        );
    }
}

/// #394: a long option on a narrow radio is cut with "…" to fit its
/// widget, as captions are, so it never wraps onto a second line.
/// Options that fit are left as they are.
#[test]
fn long_radio_options_are_cut_to_fit() {
    let mut module = module();
    module.panel = PanelView {
        width: 6,
        height: 4,
        widgets: vec![widget(
            WidgetViewKind::Radio {
                options: vec!["Pink".into(), "Exponential decay".into(), "Brown".into()],
            },
            0,
            0,
            "Colour",
        )],
    };
    let rect = module.widget_bounds(0).unwrap();
    let options: Vec<_> = items(&module)
        .into_iter()
        .filter(|item| matches!(item.kind, TextKind::Option(..)))
        .collect();
    assert_eq!(options.len(), 3);
    for item in &options {
        assert!(
            text_width(&item.content, item.size) <= item.width,
            "{} fits on one line",
            item.content
        );
        let text = item.bounds();
        assert!(
            text.x + text.width <= rect.x + rect.width,
            "{} stays in its widget",
            item.content
        );
    }
    assert_eq!(options[0].content, "Pink");
    assert!(options[1].content.ends_with('…'), "{}", options[1].content);
    assert!("Exponential decay".starts_with(options[1].content.trim_end_matches('…')));
    assert_eq!(options[2].content, "Brown");
}

#[test]
fn port_labels_sit_on_their_rows_beside_their_ports() {
    let module = module();
    for item in items(&module) {
        let TextKind::Port(port) = item.kind else {
            continue;
        };
        let at = if port.output {
            module.output_position(port.index)
        } else {
            module.input_position(port.index)
        };
        assert_eq!(item.anchor.y, at.y, "{} on its port's row", item.content);
        let text = item.bounds();
        assert!(
            text.width <= module.label_width() + 1e-3,
            "{} fits its column",
            item.content
        );
        // Inputs read from the left edge, outputs end at the right edge.
        if port.output {
            assert!(text.x + text.width <= at.x);
        } else {
            assert!(text.x >= at.x);
        }
    }
}

#[test]
fn bars_sit_inside_their_text() {
    let module = module();
    for item in items(&module) {
        let (text, bar) = (item.bounds(), item.bar());
        assert_eq!((bar.x, bar.width), (text.x, text.width), "{}", item.content);
        assert!(bar.y > text.y && bar.y + bar.height < text.y + text.height);
        assert!(bar.width > 0.0 && bar.height > 0.0);
    }
}

#[test]
fn labels_are_found_under_the_pointer() {
    let scene = Scene::new(vec![module()], Vec::new());
    let module = &scene.modules()[0];
    let lowpass = PortRef {
        module: 1,
        output: true,
        index: 0,
    };
    let mut label = None;
    for_each(module, |item| {
        if item.kind == TextKind::Port(lowpass) {
            label = Some(item.bounds());
        }
    });
    let label = label.unwrap();
    assert_eq!(port_label_at(&scene, label.center()), Some(lowpass));
    // Empty panel space and the header are not labels.
    let knob = module.widget_bounds(0).unwrap();
    assert_eq!(port_label_at(&scene, knob.center()), None);
    assert_eq!(
        port_label_at(&scene, Point::new(module.bounds().center_x(), module.bounds().y + 2.0)),
        None
    );
    assert_eq!(port_label_at(&scene, Point::new(-500.0, -500.0)), None);

    // A tile on top that covers the label but has no label there
    // itself: the label still hovers, as it is drawn above both.
    let mut cover = self::module();
    cover.id = 2;
    cover.inputs.clear();
    cover.outputs.clear();
    cover.position = Point::new(label.x - 60.0, label.y - 40.0);
    let overlapped = Scene::new(vec![self::module(), cover], Vec::new());
    assert!(overlapped.modules()[1].bounds().contains(label.center()));
    assert_eq!(port_label_at(&overlapped, label.center()), Some(lowpass));
}

/// An input with a clock mark has its label moved right, clear of the
/// mark, and the label keeps within its column.
#[test]
fn a_clock_mark_moves_the_input_label_clear_of_it() {
    let plain = module();
    let mut marked = module();
    marked.inputs[1].follows_transport = Some(crate::scene::TransportSignal::BarPhase);
    fn label(m: &ModuleView, index: usize) -> TextItem<'_> {
        items(m)
            .into_iter()
            .find(|item| {
                item.kind
                    == TextKind::Port(PortRef {
                        module: 1,
                        output: false,
                        index,
                    })
            })
            .unwrap()
    }
    let (before, after) = (label(&plain, 1), label(&marked, 1));
    assert_eq!(after.anchor.x, before.anchor.x + CLOCK_MARK_SPACE);
    assert_eq!(after.anchor.x + after.width, before.anchor.x + before.width);
    let mark = marked.clock_mark_centre(1).unwrap();
    assert!(mark.x + crate::scene::CLOCK_MARK_RADIUS < after.bounds().x);
    assert!(mark.x - crate::scene::CLOCK_MARK_RADIUS > marked.input_position(1).x + crate::scene::PORT_RADIUS);
    // The other input has no mark and keeps its place.
    assert_eq!(label(&marked, 0), label(&plain, 0));
    assert_eq!(marked.clock_mark_centre(0), None);
}
