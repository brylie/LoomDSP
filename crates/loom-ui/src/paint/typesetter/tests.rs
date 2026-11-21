//! Tests for the typesetter. They use the machine's own fonts, so they check
//! relations between sizes, never a number of pixels: the face differs
//! from macOS to Windows to Linux (spec section 8, rule 6).

use super::*;

/// A plain style at `size`.
fn style(size: f32) -> TextStyle {
    TextStyle {
        size,
        line_height: 1.2,
        weight: Weight::Regular,
    }
}

/// Canvas text, or `None` on a machine with no font to set text in, where
/// these tests skip as the screenshot tests do without a GPU.
fn text() -> Option<Typesetter> {
    let mut text = Typesetter::new();
    if text.measure("x", style(10.0), None).width > 0.0 {
        Some(text)
    } else {
        eprintln!("skipped: no system font");
        None
    }
}

/// Longer text is wider, larger text is wider and taller, and a line is
/// as high as its line height says.
#[test]
fn sizes_follow_the_text_and_the_style() {
    let Some(mut text) = text() else { return };
    let short = text.measure("Cut", style(10.0), None);
    let long = text.measure("Cutoff", style(10.0), None);
    let large = text.measure("Cutoff", style(20.0), None);
    assert!(long.width > short.width, "{long:?} {short:?}");
    assert!(large.width > long.width * 1.8, "{large:?} {long:?}");
    assert!(large.height > long.height, "{large:?} {long:?}");
    assert!((long.height - 12.0).abs() < 0.01, "{long:?}");
}

/// Bold letters are wider than regular ones.
#[test]
fn bold_is_wider() {
    let Some(mut text) = text() else { return };
    let regular = text.measure("Resonance", style(12.0), None);
    let bold = text.measure(
        "Resonance",
        TextStyle {
            weight: Weight::Bold,
            ..style(12.0)
        },
        None,
    );
    assert!(bold.width > regular.width, "{bold:?} {regular:?}");
}

/// Wrapping breaks between words: narrower than one line, and taller.
#[test]
fn wrapping_breaks_between_words() {
    let Some(mut text) = text() else { return };
    let sentence = "The cutoff frequency of the filter, in hertz.";
    let line = text.measure(sentence, style(12.0), None);
    let wrapped = text.measure(sentence, style(12.0), Some(line.width / 2.0));
    assert!(wrapped.width <= line.width / 2.0 + 0.01, "{wrapped:?} {line:?}");
    assert!(wrapped.height >= line.height * 2.0, "{wrapped:?} {line:?}");
}

/// A space at the end of the text widens it, so a caret after a typed
/// space moves past it. A space where wrapping breaks a line takes no
/// room in the box.
#[test]
fn a_trailing_space_counts_but_a_space_at_a_break_does_not() {
    let Some(mut text) = text() else { return };
    let word = text.measure("Gain", style(12.0), None);
    let spaced = text.measure("Gain ", style(12.0), None);
    assert!(spaced.width > word.width, "{spaced:?} {word:?}");
    let narrow = word.width + 1.0;
    let wrapped = text.measure("Gain Gain", style(12.0), Some(narrow));
    assert!(wrapped.height > word.height, "it wraps: {wrapped:?}");
    assert!((wrapped.width - word.width).abs() < 0.01, "{wrapped:?} {word:?}");
}

/// A symbol the interface font may lack still has a width: shaping falls
/// back to a font that has it.
#[test]
fn a_check_mark_has_a_width() {
    let Some(mut text) = text() else { return };
    let mark = text.measure("✓", style(12.0), None);
    assert!(mark.width > 0.0, "{mark:?}");
}

/// A layout is built once while frames use it, and dropped one frame
/// after the last frame that used it.
#[test]
fn layouts_are_kept_while_used() {
    let mut text = Typesetter::new();
    text.measure("Cutoff", style(10.0), None);
    text.measure("Cutoff", style(10.0), None);
    assert_eq!(text.take_built(), 1, "the same frame reuses it");

    text.begin_frame();
    text.measure("Cutoff", style(10.0), None);
    assert_eq!(text.take_built(), 0, "the next frame reuses it");

    text.begin_frame();
    text.begin_frame();
    text.measure("Cutoff", style(10.0), None);
    assert_eq!(text.take_built(), 1, "a frame without it dropped it");
}

/// Each part of the style keys its own layout.
#[test]
fn each_part_of_the_style_keys_a_layout() {
    let mut text = Typesetter::new();
    let base = style(10.0);
    text.measure("Mix", base, None);
    text.measure("Mix", style(11.0), None);
    text.measure(
        "Mix",
        TextStyle {
            line_height: 1.4,
            ..base
        },
        None,
    );
    text.measure(
        "Mix",
        TextStyle {
            weight: Weight::Bold,
            ..base
        },
        None,
    );
    text.measure("Mix", base, Some(50.0));
    text.measure("Max", base, None);
    assert_eq!(text.take_built(), 6);
}

/// A second canvas's text shares the process's font scan and sets text
/// the same way.
#[test]
fn a_second_canvas_sets_text_the_same() {
    let Some(mut first) = text() else { return };
    let mut second = Typesetter::new();
    assert_eq!(
        first.measure("Envelope", style(9.0), None),
        second.measure("Envelope", style(9.0), None)
    );
}

/// The debug view counts layouts kept, not their contents.
#[test]
fn debug_counts_the_layouts() {
    let mut text = Typesetter::default();
    text.measure("a", style(10.0), None);
    let shown = format!("{text:?}");
    assert!(shown.contains("current: 1"), "{shown}");
    assert!(shown.contains("previous: 0"), "{shown}");
}

/// A font keeps one identity while its canvas lives, even after every
/// layout that used it is dropped, so the glyph atlas can reuse its images.
#[test]
fn a_font_stays_loaded_while_its_canvas_lives() {
    let Some(mut text) = text() else { return };
    let id = |text: &mut Typesetter| {
        let layout = text.layout("Cutoff", style(10.0), None);
        let line = layout.lines().next().expect("a line");
        match line.items().next() {
            Some(PositionedLayoutItem::GlyphRun(run)) => run.run().font().data.id(),
            _ => panic!("a glyph run"),
        }
    };
    let before = id(&mut text);
    text.begin_frame();
    text.begin_frame();
    assert_eq!(id(&mut text), before);
}

/// With `complex-scripts`, words in Thai break by dictionary: "สวัสดีครับ"
/// has no space, and one word step from its start stops after "สวัสดี",
/// not at the end of the text. This shows the feature reaches Parley.
/// Word boundaries come from the text alone, so no Thai font is needed,
/// but movement walks the shaped layout, so it skips with no font at all.
#[cfg(feature = "complex-scripts")]
#[test]
fn thai_word_movement_stops_at_a_dictionary_word() {
    let Some(mut text) = text() else { return };
    let mut editor = parley::PlainEditor::<u16>::new(16.0);
    editor.set_text("สวัสดีครับ");
    let mut driver = editor.driver(&mut text.fonts, &mut text.layouts);
    driver.move_to_text_start();
    driver.move_word_right();
    assert_eq!(editor.raw_selection().focus().index(), "สวัสดี".len());
}

/// Spans `lens` long, in order, the middle one bold where there are three.
fn spans(lens: &[usize]) -> Vec<Span> {
    lens.iter()
        .enumerate()
        .map(|(index, &len)| Span {
            len,
            weight: if index == 1 { Weight::Bold } else { Weight::Regular },
            ..Span::default()
        })
        .collect()
}

/// Each span's index is its glyphs' brush, so a painter can find a glyph
/// run's span: the runs of three spans carry brushes 0, 1, and 2, in order.
#[test]
fn each_span_brushes_its_glyph_runs() {
    let Some(mut text) = text() else { return };
    let layout = text.layout_spans("See Saw now", &spans(&[4, 3, 4]), style(12.0), None);
    let mut brushes: Vec<u16> = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            if let PositionedLayoutItem::GlyphRun(run) = item
                && brushes.last() != Some(&run.style().brush)
            {
                brushes.push(run.style().brush);
            }
        }
    }
    assert_eq!(brushes, [0, 1, 2]);
}

/// A monospace span is set in another face: narrow letters take more room.
#[test]
fn a_mono_span_takes_the_monospace_face() {
    let Some(mut text) = text() else { return };
    let plain = text.measure_spans("iiiiii", &[Span::plain(6)], style(12.0), None);
    let mono = text.measure_spans(
        "iiiiii",
        &[Span {
            len: 6,
            face: Face::Mono,
            ..Span::default()
        }],
        style(12.0),
        None,
    );
    assert!(mono.width > plain.width, "{mono:?} {plain:?}");
}

/// A span's colour and underline are painted, not set, so spans that
/// differ only in them share one layout.
#[test]
fn colour_and_underline_key_no_layout() {
    let Some(mut text) = text() else { return };
    let red = Span {
        colour: Some(loom_ui_core::Color::rgb(1.0, 0.0, 0.0)),
        underline: true,
        ..Span::plain(3)
    };
    text.take_built();
    text.measure_spans("Saw", &[Span::plain(3)], style(12.0), None);
    text.measure_spans("Saw", &[red], style(12.0), None);
    assert_eq!(text.take_built(), 1);
}

/// A range on one line has one rectangle, inside the text and as tall as
/// the line, to the pixel the line box is rounded to; a range that a wrap breaks has one per line, the second
/// below the first.
#[test]
fn span_rects_give_one_rectangle_per_line() {
    let Some(mut text) = text() else { return };
    let line = "alpha beta gamma delta";
    let whole = text.measure(line, style(12.0), None);
    let one = text.span_rects(line, &[], style(12.0), None, 6..10);
    assert_eq!(one.len(), 1, "{one:?}");
    assert!(one[0].x > 0.0 && one[0].x + one[0].width < whole.width, "{one:?}");
    assert!((one[0].height - whole.height).abs() <= 1.0, "{one:?} {whole:?}");
    // Wide enough for "alpha beta" only, so "beta gamma" breaks.
    let wrap = text.measure("alpha beta", style(12.0), None).width + 1.0;
    let two = text.span_rects(line, &[], style(12.0), Some(wrap), 6..16);
    assert_eq!(two.len(), 2, "{two:?}");
    assert!(two[1].y >= two[0].y + two[0].height - 0.01, "{two:?}");
}

/// No rectangle for an empty range, one past the end, or one that cuts a
/// character.
#[test]
fn span_rects_are_empty_for_no_range() {
    let Some(mut text) = text() else { return };
    assert!(text.span_rects("abc", &[], style(12.0), None, 1..1).is_empty());
    assert!(text.span_rects("abc", &[], style(12.0), None, 1..9).is_empty());
    assert!(text.span_rects("é", &[], style(12.0), None, 0..1).is_empty());
}

/// A span's brush is its index, up to the last a `u16` holds.
#[test]
fn a_brush_is_the_span_index() {
    assert_eq!(brush(0), 0);
    assert_eq!(brush(usize::from(u16::MAX)), u16::MAX);
}

/// More spans than a brush counts fail loudly in a debug build.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "more than 65535 spans")]
fn too_many_spans_panic_in_a_debug_build() {
    brush(usize::from(u16::MAX) + 1);
}
