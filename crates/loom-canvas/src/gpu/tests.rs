use super::*;
use crate::scene::{CableView, ModuleView, PortView};

/// Disabled modulation keeps its dash pattern and halves both width
/// limits, so it stays thinner at close and far zoom (#197).
#[test]
fn disabled_modulation_line_is_half_width() {
    let normal = Cable::new(Point::ORIGIN, Point::new(100.0, 0.0), Color::WHITE, Color::WHITE, true).thin();
    let off = normal.half_width();
    assert_eq!(off.params[0], normal.params[0] * 0.5);
    assert_eq!(off.params[2], normal.params[2] * 0.5);
    assert_eq!(off.params[1], normal.params[1]);
}

fn port() -> PortView {
    PortView {
        label: "p".into(),
        hover: crate::scene::HoverText::default(),
        colour: Color::WHITE,
        shape: PortShape::Round,
        badge: 0,
        follows_transport: None,
    }
}

const SHADER: &str = include_str!("../canvas.wgsl");

#[test]
fn shader_is_valid() {
    let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parses");
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .expect("WGSL validates");
    // Translate to Metal Shading Language, as wgpu does on macOS.
    let options = naga::back::msl::Options::default();
    let pipeline = naga::back::msl::PipelineOptions::default();
    naga::back::msl::write_string(&module, &info, &options, &pipeline).expect("translates to Metal");
}

/// wgpu passes some WGSL names straight through to Metal and HLSL, where
/// they are reserved. `half` (a Metal type) crashed the app on macOS.
#[test]
fn shader_names() {
    const RESERVED: &[&str] = &[
        "half", "half2", "half3", "half4", "in", "out", "inout", "vector", "matrix", "sample", "texture", "sampler",
        "kernel", "vertex", "fragment", "device", "constant", "thread", "packed",
    ];
    for (number, line) in SHADER.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        // `@vertex` and `@fragment` are WGSL attributes, not names.
        let words = code
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '@'))
            .filter(|w| !w.starts_with('@'));
        for word in words {
            assert!(
                !RESERVED.contains(&word),
                "canvas.wgsl line {}: `{word}` is reserved in Metal or HLSL",
                number + 1
            );
        }
    }
}

#[test]
fn instance_sizes_match_the_shader() {
    // Four vec4 attributes each.
    assert_eq!(std::mem::size_of::<Shape>(), 64);
    assert_eq!(std::mem::size_of::<Cable>(), 64);
    assert_eq!(std::mem::size_of::<Arc>(), 64);
    assert_eq!(std::mem::size_of::<Segment>(), 64);
    assert_eq!(std::mem::size_of::<Globals>(), 32);
}

#[test]
fn builds_one_instance_per_item() {
    let module = |id, x| ModuleView {
        size: None,
        unknown: false,
        id,
        title: String::new(),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position: Point::new(x, 0.0),
        inputs: vec![port(), port()],
        outputs: vec![port()],
        panel: crate::scene::PanelView::default(),
    };
    let scene = Scene::new(
        vec![module(1, 0.0), module(2, 200.0)],
        vec![
            CableView {
                from: (1, 0),
                to: (2, 1),
                colour_from: Color::WHITE,
                colour_to: Color::WHITE,
                delayed: false,
            },
            // Unknown port: skipped.
            CableView {
                from: (1, 5),
                to: (2, 0),
                colour_from: Color::WHITE,
                colour_to: Color::WHITE,
                delayed: false,
            },
        ],
    );
    let geometry = Geometry::build(&scene, &Palette::default());
    assert_eq!(geometry.cables.len(), 1);
    // Three shapes per tile, one bar per port label (the titles are
    // empty, so they have none), and one marker per port.
    assert_eq!(geometry.shapes.len(), 2 * 3 + 6 + 6);
    assert_eq!(geometry.tiles, 2 * 3);
    assert_eq!(
        (geometry.caption_bars, geometry.port_bars, geometry.title_bars),
        (0, 6, 0)
    );
    assert_eq!(geometry.markers(), 2 * 3 + 6);
    assert_eq!(geometry.generation, scene.generation());
}

/// An output that drives assignments gets one badge pill, drawn with
/// the markers over the cables. Its count is text.
#[test]
fn a_badge_is_one_shape_with_the_markers() {
    let plain = ModuleView {
        size: None,
        unknown: false,
        id: 1,
        title: String::new(),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position: Point::ORIGIN,
        inputs: vec![],
        outputs: vec![port(), port()],
        panel: Default::default(),
    };
    let mut badged = plain.clone();
    badged.outputs[1].badge = 3;
    let shapes = |m: ModuleView| Geometry::build(&Scene::new(vec![m], vec![]), &Palette::default());
    let (before, after) = (shapes(plain), shapes(badged));
    assert_eq!(after.shapes.len(), before.shapes.len() + 1);
    assert_eq!(after.markers(), before.markers());
}

#[test]
fn each_zoom_draws_the_bars_for_the_text_it_hides() {
    let module = ModuleView {
        size: None,
        unknown: false,
        id: 1,
        title: "Saw".into(),
        hover: crate::scene::HoverText::default(),
        accent: Color::BLACK,
        position: Point::ORIGIN,
        inputs: vec![port()],
        outputs: vec![port()],
        panel: crate::scene::PanelView {
            width: 3,
            height: 3,
            widgets: vec![crate::scene::WidgetView {
                kind: crate::scene::WidgetViewKind::Knob { steps: 0 },
                x: 0,
                y: 0,
                label: "Gain".into(),
                hover: crate::scene::HoverText::default(),
                hidden: false,
            }],
        },
    };
    let palette = Palette::default();
    let geometry = Geometry::build(&Scene::new(vec![module], Vec::new()), &palette);
    assert_eq!(
        (geometry.caption_bars, geometry.port_bars, geometry.title_bars),
        (1, 2, 1)
    );
    let colours = |range: std::ops::Range<u32>| -> Vec<[f32; 4]> {
        geometry.shapes[range.start as usize..range.end as usize]
            .iter()
            .map(|s| s.fill)
            .collect()
    };
    let label = pack(palette.text_bar).components();
    let title = pack(palette.title_bar).components();
    // Close up: all text is drawn, so no bars.
    assert!(geometry.bars(false, false).is_empty());
    // Middle: the caption and both port labels, not the title.
    assert_eq!(colours(geometry.bars(true, false)), vec![label; 3]);
    // Far: both port labels and the title. Controls are not drawn, so
    // neither is the caption's bar.
    assert_eq!(colours(geometry.bars(true, true)), vec![label, label, title]);
}

/// A transport input gets a clock mark (a ring and two hands) close up,
/// a plain dot at middle zoom, and nothing far out. Ports keep their
/// markers, and the text bars their ranges.
#[test]
fn a_clock_mark_shows_close_up_and_as_a_dot_at_middle_zoom() {
    let mut followed = port();
    followed.follows_transport = Some(crate::scene::TransportSignal::BarPhase);
    let module = |inputs: Vec<PortView>| ModuleView {
        size: None,
        unknown: false,
        id: 1,
        title: "LFO".into(),
        hover: crate::scene::HoverText::default(),
        accent: Color::from_rgb8(0x5A, 0x4A, 0x82),
        position: Point::ORIGIN,
        inputs,
        outputs: vec![port()],
        panel: Default::default(),
    };
    let palette = Palette::default();
    let build = |m: ModuleView| Geometry::build(&Scene::new(vec![m], Vec::new()), &palette);
    let plain = build(module(vec![port(), port()]));
    let marked = build(module(vec![port(), followed]));
    assert_eq!((plain.clock_marks, plain.clock_dots), (0, 0));
    assert_eq!((marked.clock_marks, marked.clock_dots), (3, 1));
    assert_eq!(marked.shapes.len(), plain.shapes.len() + 4);
    // Close up: only the mark's three shapes, a ring first.
    let close = marked.bars(false, false);
    assert_eq!(close.len(), 3);
    let ring = &marked.shapes[close.start as usize];
    assert_eq!(ring.fill(), pack(Color::TRANSPARENT).components());
    assert!(ring.border_width() > 0.0);
    // Middle: the dot, then the same bars as without the mark.
    let middle = marked.bars(true, false);
    assert_eq!(middle.len(), plain.bars(true, false).len() + 1);
    // Far: no mark, just the bars.
    assert_eq!(marked.bars(true, true).len(), plain.bars(true, true).len());
    assert!(marked.bars(true, true).start > middle.start);
    // The mark sits between the port marker and the label.
    let [x, _, w, _] = ring.rect();
    assert!(x > PORT_RADIUS && x + w < 9.0 + crate::scene::CLOCK_MARK_SPACE);
}
