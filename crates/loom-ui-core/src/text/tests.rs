use super::*;

/// Every character is half the size wide, on one line.
struct Half;

impl TextMeasure for Half {
    fn measure(&mut self, text: &str, style: TextStyle, _wrap_width: Option<f32>) -> Size {
        Size::new(
            text.chars().count() as f32 * style.size * 0.5,
            style.size * style.line_height,
        )
    }
}

const STYLE: TextStyle = TextStyle {
    size: 10.0,
    line_height: 1.5,
    weight: Weight::Regular,
};

/// Spans cover the text from its start, and a span past its end is cut.
#[test]
fn span_ranges_follow_each_other_and_stop_at_the_end() {
    let spans = [Span::plain(3), Span::plain(4), Span::plain(10)];
    let ranges: Vec<_> = Span::ranges(&spans, 9).collect();
    assert_eq!(ranges, vec![0..3, 3..7, 7..9]);
}

/// A span too long to add up saturates, so the spans after it are empty
/// at the end of the text rather than overflowing.
#[test]
fn span_ranges_saturate() {
    let spans = [Span::plain(usize::MAX), Span::plain(1)];
    let ranges: Vec<_> = Span::ranges(&spans, 3).collect();
    assert_eq!(ranges, vec![0..3, 3..3]);
}

/// The default rectangles are the range's place on one line.
#[test]
fn default_span_rects_measure_on_one_line() {
    let rects = Half.span_rects("see the page", &[], STYLE, None, 4..7);
    assert_eq!(rects, vec![Rect::new(Point::new(20.0, 0.0), Size::new(15.0, 15.0))]);
}

/// An empty range, or one that cuts a character, has no rectangle.
#[test]
fn default_span_rects_are_empty_for_no_range() {
    assert!(Half.span_rects("abc", &[], STYLE, None, 1..1).is_empty());
    assert!(Half.span_rects("é", &[], STYLE, None, 0..1).is_empty());
}

/// The default size of spanned text is the plain text's.
#[test]
fn default_measure_spans_ignores_the_spans() {
    let spans = [Span {
        len: 3,
        weight: Weight::Bold,
        ..Span::default()
    }];
    assert_eq!(
        Half.measure_spans("abc", &spans, STYLE, None),
        Half.measure("abc", STYLE, None)
    );
}
