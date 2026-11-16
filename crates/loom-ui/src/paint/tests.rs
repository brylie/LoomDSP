use super::testing::{Painted, Recorder};
use super::*;

/// A plain style for the tests.
fn style(size: f32) -> TextStyle {
    TextStyle {
        size,
        line_height: 1.2,
        weight: Weight::Regular,
    }
}

/// A filled box has no outline.
#[test]
fn a_filled_quad_has_no_outline() {
    let rect = Rect::new(Point::new(1.0, 2.0), Size::new(3.0, 4.0));
    let quad = Quad::filled(rect, Color::WHITE, 2.0);
    assert_eq!(quad.rect, rect);
    assert_eq!(quad.fill, Color::WHITE);
    assert_eq!(quad.border_width, 0.0);
    assert_eq!(quad.border, Color::TRANSPARENT);
    assert_eq!(quad.radius, 2.0);
}

/// The recording painter keeps text, boxes, and layers in the order given,
/// and copies the borrowed text.
#[test]
fn the_recorder_keeps_everything_in_order() {
    let clip = Rect::new(Point::ORIGIN, Size::new(100.0, 50.0));
    let mut recorder = Recorder::default();
    recorder.begin_layer(clip);
    recorder.quad(Quad::filled(clip, Color::BLACK, 0.0));
    {
        let text = String::from("gain");
        recorder.text(&TextRun {
            text: &text,
            anchor: Point::new(5.0, 6.0),
            bounds: Size::new(40.0, 14.0),
            align: Align::Left,
            vertical: Vertical::Center,
            style: style(10.0),
            wrap: false,
            colour: Color::WHITE,
            clip,
        });
    }
    recorder.end_layer();
    assert_eq!(recorder.texts(), ["gain"]);
    assert!(matches!(
        recorder.painted.as_slice(),
        [Painted::Begin(_), Painted::Quad(_), Painted::Text { .. }, Painted::End]
    ));
    let Painted::Text { run, .. } = &recorder.painted[2] else {
        panic!("the third piece is text");
    };
    assert_eq!(run.anchor, Point::new(5.0, 6.0));
    assert_eq!(run.colour, Color::WHITE);
}

/// The recorder's fixed measure gives one line without a wrap width, and
/// breaks into lines of whole characters with one.
#[test]
fn the_fixed_measure_wraps_by_whole_characters() {
    let mut measure = Recorder::default();
    assert_eq!(measure.measure("abcd", style(10.0), None), Size::new(20.0, 12.0));
    assert_eq!(measure.measure("abcd", style(10.0), Some(100.0)), Size::new(20.0, 12.0));
    assert_eq!(
        measure.measure("abcdef", style(10.0), Some(10.0)),
        Size::new(10.0, 36.0)
    );
    assert_eq!(measure.measure("", style(10.0), None), Size::new(0.0, 12.0));
}

/// The paint shaders validate alone, as the paint renderer builds them, and
/// translate to Metal, as wgpu does on macOS. A host that appends its own
/// stages validates the two together in its own tests.
#[test]
fn the_paint_shader_is_valid_alone() {
    let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parses");
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .expect("WGSL validates");
    let options = naga::back::msl::Options::default();
    let pipeline = naga::back::msl::PipelineOptions::default();
    naga::back::msl::write_string(&module, &info, &options, &pipeline).expect("translates to Metal");
}

/// wgpu passes some WGSL names straight through to Metal and HLSL, where
/// they are reserved, so the paint shaders use none of them.
#[test]
fn the_paint_shader_uses_no_reserved_names() {
    const RESERVED: &[&str] = &[
        "half", "half2", "half3", "half4", "in", "out", "inout", "vector", "matrix", "sample", "texture", "sampler",
        "kernel", "vertex", "fragment", "device", "constant", "thread",
    ];
    for (number, line) in SHADER.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        let words = code
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '@'))
            .filter(|w| !w.starts_with('@'));
        for word in words {
            assert!(
                !RESERVED.contains(&word),
                "paint.wgsl line {}: `{word}` is reserved in Metal or HLSL",
                number + 1
            );
        }
    }
}
