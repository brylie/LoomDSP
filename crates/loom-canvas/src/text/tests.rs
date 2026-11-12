//! Tests for canvas text. They use the machine's own fonts, so they check
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
fn text() -> Option<CanvasText> {
    let mut text = CanvasText::new();
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
    let mut text = CanvasText::new();
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
    let mut text = CanvasText::new();
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
    let mut second = CanvasText::new();
    assert_eq!(
        first.measure("Envelope", style(9.0), None),
        second.measure("Envelope", style(9.0), None)
    );
}

/// The debug view counts layouts kept, not their contents.
#[test]
fn debug_counts_the_layouts() {
    let mut text = CanvasText::default();
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
    let id = |text: &mut CanvasText| {
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
