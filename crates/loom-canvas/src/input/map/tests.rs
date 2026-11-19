use super::*;

/// Long solid lines take one mark and dashes take one mark each.
#[test]
fn lines_use_one_mark_per_solid_line_or_dash() {
    let mut marks = Vec::new();
    let start = Point::new(1.0, 2.0);
    let end = Point::new(7.0, 10.0);
    let line = MapLine {
        from: 1,
        to: 2,
        colour: Color::WHITE,
        width: 1.25,
        dash: 0.0,
    };
    line_marks(&mut marks, start, end, line);
    assert_eq!(
        marks,
        [Segment {
            from: start,
            to: end,
            width: 1.25,
            colour: Color::WHITE
        }]
    );

    marks.clear();
    line_marks(&mut marks, start, end, MapLine { dash: 4.0, ..line });
    assert_eq!(marks.len(), 2);
    assert_eq!(marks[0].from, start);
    assert_eq!(marks[1].to, end);
}
