use super::*;

const PORT: HoverTarget = HoverTarget::Port(PortRef {
    module: 1,
    output: false,
    index: 0,
});
const KNOB: HoverTarget = HoverTarget::Control(1, 0);

#[test]
fn shows_after_resting_for_the_delay() {
    let start = Instant::now();
    let mut timer = HoverTimer::default();
    assert!(!timer.rest(Some(PORT), start));
    assert_eq!(timer.due(), Some(start + HOVER_DELAY));
    assert!(!timer.tick(start + HOVER_DELAY - Duration::from_millis(1)));
    assert_eq!(timer.shown(), None);
    assert!(timer.tick(start + HOVER_DELAY));
    assert_eq!(timer.shown(), Some(PORT));
}

#[test]
fn a_shown_text_asks_for_no_more_frames() {
    let start = Instant::now();
    let mut timer = HoverTimer::default();
    timer.rest(Some(PORT), start);
    timer.tick(start + HOVER_DELAY);
    assert_eq!(timer.due(), None);
    // Resting on the same target, or redrawing, changes nothing.
    assert!(!timer.rest(Some(PORT), start + HOVER_DELAY * 4));
    assert!(!timer.tick(start + HOVER_DELAY * 4));
    assert_eq!(timer.shown(), Some(PORT));
}

#[test]
fn leaving_hides_at_once_and_a_new_target_waits_again() {
    let start = Instant::now();
    let mut timer = HoverTimer::default();
    timer.rest(Some(PORT), start);
    timer.tick(start + HOVER_DELAY);
    let later = start + HOVER_DELAY * 2;
    assert!(timer.rest(Some(KNOB), later), "hides the port's text");
    assert_eq!(timer.shown(), None);
    assert_eq!(timer.due(), Some(later + HOVER_DELAY));
    assert!(!timer.rest(None, later), "nothing was shown");
    assert_eq!(timer.due(), None);
}

#[test]
fn moving_within_a_target_does_not_restart_the_wait() {
    let start = Instant::now();
    let mut timer = HoverTimer::default();
    timer.rest(Some(PORT), start);
    timer.rest(Some(PORT), start + Duration::from_millis(300));
    assert!(timer.tick(start + HOVER_DELAY));
}

#[test]
fn pan_and_zoom_hide_and_wait_again() {
    let start = Instant::now();
    let mut timer = HoverTimer::default();
    timer.rest(Some(KNOB), start);
    timer.tick(start + HOVER_DELAY);
    let later = start + HOVER_DELAY * 3;
    assert!(timer.restart(later));
    assert_eq!(timer.shown(), None);
    assert_eq!(timer.due(), Some(later + HOVER_DELAY));
}

#[test]
fn hover_text_stays_inside_the_canvas_at_every_edge() {
    let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(800.0, 600.0));
    let size = Size::new(HOVER_MAX_WIDTH, 60.0);
    let inside = |at: Point| {
        at.x >= bounds.x
            && at.y >= bounds.y
            && at.x + size.width <= bounds.x + bounds.width
            && at.y + size.height <= bounds.y + bounds.height
    };
    let port = |x: f32, y: f32| Rectangle::new(Point::new(x - 4.0, y - 4.0), Size::new(8.0, 8.0));
    for anchor in [
        port(12.0, 300.0),  // left edge
        port(808.0, 300.0), // right edge
        port(400.0, 22.0),  // top edge
        port(400.0, 618.0), // bottom edge
        port(12.0, 22.0),   // corner
    ] {
        let at = place(size, anchor, bounds);
        assert!(inside(at), "{anchor:?} -> {at:?}");
    }
}

#[test]
fn a_narrow_canvas_narrows_the_text_and_the_box_fits() {
    assert_eq!(text_width(1_000.0), HOVER_MAX_WIDTH - 2.0 * HOVER_PADDING);
    let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(150.0, 400.0));
    let width = text_width(bounds.width) + 2.0 * HOVER_PADDING;
    assert!(width <= bounds.width - 2.0 * HOVER_GAP, "{width}");
    let size = Size::new(width, 80.0);
    let anchor = Rectangle::new(Point::new(140.0, 200.0), Size::new(8.0, 8.0));
    let at = place(size, anchor, bounds);
    assert!(at.x >= 0.0 && at.x + size.width <= bounds.width, "{at:?}");
    // Even a canvas too small for padding leaves a positive width.
    assert!(text_width(4.0) > 0.0);
}

#[test]
fn hover_text_sits_above_and_goes_below_near_the_top() {
    let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 600.0));
    let size = Size::new(100.0, 40.0);
    let middle = Rectangle::new(Point::new(395.0, 295.0), Size::new(10.0, 10.0));
    let at = place(size, middle, bounds);
    assert_eq!(at, Point::new(350.0, 295.0 - HOVER_GAP - 40.0));
    let top = Rectangle::new(Point::new(395.0, 10.0), Size::new(10.0, 10.0));
    assert_eq!(place(size, top, bounds).y, 20.0 + HOVER_GAP);
}
