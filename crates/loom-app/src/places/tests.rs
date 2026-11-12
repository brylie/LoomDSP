use super::*;
use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, RelativeOffset};
use loom_ui_core::{Point, Size};

/// A scrolling area's state, for the calls that need one. The collector
/// never scrolls it.
struct Still;

impl Scrollable for Still {
    fn snap_to(&mut self, _offset: RelativeOffset<Option<f32>>) {}
    fn scroll_to(&mut self, _offset: AbsoluteOffset<Option<f32>>) {}
    fn scroll_by(&mut self, _offset: AbsoluteOffset, _bounds: Rectangle, _content_bounds: Rectangle) {}
}

fn rectangle(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
    Rectangle { x, y, width, height }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(Point::new(x, y), Size::new(width, height))
}

/// Reports a scrolling area and walks `content` inside it, as the
/// toolkit's scrollable does.
fn scrolling(
    operation: &mut dyn Operation<Places>,
    bounds: Rectangle,
    offset: f32,
    content: &mut dyn FnMut(&mut dyn Operation<Places>),
) {
    operation.scrollable(None, bounds, bounds, iced::Vector::new(0.0, offset), &mut Still);
    operation.traverse(content);
}

/// Reports a named container with nothing inside.
fn part(operation: &mut dyn Operation<Places>, name: &'static str, bounds: Rectangle) {
    operation.container(Some(&Id::new(name)), bounds);
    operation.traverse(&mut |_| {});
}

/// A part outside any scrolling area is where it was laid out. A part
/// inside one is moved back by the area's offset and knows what cuts it
/// off. After the area, parts are unmoved again.
#[test]
fn a_part_in_a_scrolling_area_is_moved_by_its_offset() {
    let mut collect = Collect::default();
    part(&mut collect, "toolbar/undo", rectangle(10.0, 4.0, 50.0, 20.0));
    let area = rectangle(600.0, 40.0, 280.0, 400.0);
    scrolling(&mut collect, area, 150.0, &mut |inside| {
        part(inside, "inspector/4/param/cutoff", rectangle(600.0, 100.0, 280.0, 24.0));
        part(inside, "inspector/4/param/mix", rectangle(600.0, 700.0, 280.0, 24.0));
    });
    part(&mut collect, "stats/load", rectangle(10.0, 580.0, 90.0, 16.0));
    let Outcome::Some(places) = collect.finish() else {
        panic!("the collector always has a result");
    };
    assert_eq!(places.len(), 4);

    let id = |text: &str| text.parse::<UiId>().unwrap();
    let undo = places.of(&id("toolbar/undo")).unwrap();
    assert_eq!((undo.rect, undo.clip), (rect(10.0, 4.0, 50.0, 20.0), None));
    assert!(undo.is_shown());

    let clip = Some(rect(600.0, 40.0, 280.0, 400.0));
    let cutoff = places.of(&id("inspector/4/param/cutoff")).unwrap();
    assert_eq!((cutoff.rect, cutoff.clip), (rect(600.0, -50.0, 280.0, 24.0), clip));
    assert!(!cutoff.is_shown(), "scrolled out above the area");
    let mix = places.of(&id("inspector/4/param/mix")).unwrap();
    assert_eq!(mix.rect, rect(600.0, 550.0, 280.0, 24.0));
    assert!(!mix.is_shown(), "still below the area");

    let load = places.of(&id("stats/load")).unwrap();
    assert_eq!((load.rect, load.clip), (rect(10.0, 580.0, 90.0, 16.0), None));
    assert_eq!(places.of(&id("toolbar/redo")), None);
}

/// A scrolling area inside another: the inner content moves by both
/// offsets, the inner area's own bounds by the outer one, and the part is
/// cut by the nearest area.
#[test]
fn nested_scrolling_areas_add_their_offsets() {
    let mut collect = Collect::default();
    scrolling(&mut collect, rectangle(0.0, 0.0, 300.0, 300.0), 100.0, &mut |outer| {
        scrolling(outer, rectangle(0.0, 200.0, 300.0, 100.0), 30.0, &mut |inner| {
            part(inner, "matrix/7", rectangle(0.0, 240.0, 300.0, 20.0));
        });
        part(outer, "matrix/8", rectangle(0.0, 320.0, 300.0, 20.0));
    });
    let Outcome::Some(places) = collect.finish() else {
        panic!("the collector always has a result");
    };
    let id = |text: &str| text.parse::<UiId>().unwrap();
    let inner = places.of(&id("matrix/7")).unwrap();
    assert_eq!(inner.rect, rect(0.0, 110.0, 300.0, 20.0));
    assert_eq!(inner.clip, Some(rect(0.0, 100.0, 300.0, 100.0)));
    assert!(inner.is_shown());
    let outer = places.of(&id("matrix/8")).unwrap();
    assert_eq!(outer.rect, rect(0.0, 220.0, 300.0, 20.0));
    assert_eq!(outer.clip, Some(rect(0.0, 0.0, 300.0, 300.0)));
    // Each part's nearest area, numbered in the order the walk met them,
    // with the area's own bounds in the window, to scroll the part into
    // view.
    assert_eq!(
        inner.area,
        Some(Area {
            index: 1,
            bounds: rect(0.0, 100.0, 300.0, 100.0)
        })
    );
    assert_eq!(
        outer.area,
        Some(Area {
            index: 0,
            bounds: rect(0.0, 0.0, 300.0, 300.0)
        })
    );
}

/// The canvas is found by its view, and a container with no ID is not a
/// named part.
#[test]
fn the_canvas_is_found_and_unnamed_containers_are_not_parts() {
    let mut collect = Collect::default();
    let mut view = CanvasView {
        bounds: rect(0.0, 30.0, 600.0, 500.0),
        camera: loom_canvas::Camera::default(),
    };
    collect.custom(None, rectangle(0.0, 30.0, 600.0, 500.0), &mut view);
    collect.custom(None, rectangle(0.0, 0.0, 1.0, 1.0), &mut 5_u32);
    collect.container(None, rectangle(0.0, 0.0, 10.0, 10.0));
    let Outcome::Some(places) = collect.finish() else {
        panic!("the collector always has a result");
    };
    assert_eq!(places.canvas.map(|canvas| canvas.bounds), Some(view.bounds));
    assert_eq!(places.len(), 0);
    assert_eq!(Places::of_canvas(None).len(), 0);
}

/// An area scrolled wholly out of the area around it shows nothing: its
/// clip is empty, so a part inside it is not shown, though the part lies
/// inside the inner area's own bounds. Where the two overlap, the clip is
/// the overlap.
#[test]
fn an_area_scrolled_out_of_the_outer_one_shows_nothing() {
    let mut collect = Collect::default();
    scrolling(&mut collect, rectangle(0.0, 0.0, 300.0, 300.0), 0.0, &mut |outer| {
        // Below the outer area: none of it can be seen.
        scrolling(outer, rectangle(0.0, 400.0, 300.0, 100.0), 0.0, &mut |inner| {
            part(inner, "matrix/7", rectangle(0.0, 420.0, 300.0, 20.0));
        });
        // Half inside it: only the half that shows can be seen.
        scrolling(outer, rectangle(0.0, 250.0, 300.0, 100.0), 0.0, &mut |inner| {
            part(inner, "matrix/8", rectangle(0.0, 260.0, 300.0, 20.0));
        });
    });
    let Outcome::Some(places) = collect.finish() else {
        panic!("the collector always has a result");
    };
    let id = |text: &str| text.parse::<UiId>().unwrap();
    let hidden = places.of(&id("matrix/7")).unwrap();
    assert!(!hidden.is_shown(), "{hidden:?}");
    let half = places.of(&id("matrix/8")).unwrap();
    assert_eq!(half.clip, Some(rect(0.0, 250.0, 300.0, 50.0)));
    assert!(half.is_shown());
}
