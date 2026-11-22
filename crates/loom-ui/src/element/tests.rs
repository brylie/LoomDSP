//! Tests for `element.rs`: the builder's defaults and what each setter
//! sets.

use super::{Element, Kind, Length, Padding};
use loom_ui_core::{IdKey, TextStyle, Weight};

/// A text style for the tests.
const STYLE: TextStyle = TextStyle {
    size: 12.0,
    line_height: 1.25,
    weight: Weight::Regular,
};

/// A new element shrinks to its content, has no key, and is not opaque.
#[test]
fn a_new_element_has_the_defaults() {
    let element = Element::row([Element::text("a", STYLE)]);
    assert_eq!(element.width, Length::Shrink);
    assert_eq!(element.height, Length::Shrink);
    assert_eq!(element.key, None);
    assert!(!element.opaque && !element.wrap);
    assert_eq!(element.children.len(), 1);
}

/// `wrap` wraps a text's lines, and a row's children.
#[test]
fn wrap_means_lines_for_text_and_children_for_a_row() {
    let text = Element::text("a b", STYLE).wrap();
    assert!(matches!(text.kind, Kind::Text { wrap: true, .. }));
    assert!(!text.wrap);
    assert!(Element::row([]).wrap().wrap);
}

/// Each setter sets what it names.
#[test]
fn the_setters_set_what_they_name() {
    let element = Element::column([])
        .key("list")
        .width(Length::FILL)
        .height(Length::Fixed(20.0))
        .min_width(1.0)
        .min_height(2.0)
        .max_width(3.0)
        .max_height(4.0)
        .padding(Padding::symmetric(5.0, 6.0))
        .spacing(7.0)
        .opaque();
    assert_eq!(element.key, Some(IdKey::from("list")));
    assert_eq!(element.width, Length::fill(1));
    assert_eq!(element.height, Length::Fixed(20.0));
    assert_eq!(element.min, (Some(1.0), Some(2.0)));
    assert_eq!(element.max, (Some(3.0), Some(4.0)));
    assert_eq!(
        element.padding,
        Padding {
            top: 5.0,
            right: 6.0,
            bottom: 5.0,
            left: 6.0
        }
    );
    assert_eq!(element.spacing, 7.0);
    assert!(element.opaque);
    assert_eq!(Padding::from(3.0), Padding::all(3.0));
}

/// Space is a sized element with no children.
#[test]
fn space_has_its_sizes_and_no_children() {
    let space = Element::space(Length::FILL, Length::Fixed(4.0));
    assert_eq!(space.kind, Kind::Space);
    assert_eq!((space.width, space.height), (Length::FILL, Length::Fixed(4.0)));
    assert!(space.children.is_empty());
    assert_eq!(Element::stack([]).kind, Kind::Stack);
}

/// Only text has a colour: a debug build panics on `.colour` for any other
/// element, so a mistake shows at once instead of drawing nothing.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "only text has a colour")]
fn colour_is_for_text_only() {
    let _ = Element::row([]).colour(loom_ui_core::Color::WHITE);
}

/// The look's setters set what they name, and a text keeps its colour.
#[test]
fn the_look_setters_set_what_they_name() {
    let white = loom_ui_core::Color::WHITE;
    let element = Element::column([])
        .background(white)
        .border(2.0, white)
        .radius(4.0)
        .clip();
    assert_eq!(element.look.background, Some(white));
    assert_eq!(element.look.border, Some((2.0, white)));
    assert_eq!(element.look.radius, 4.0);
    assert!(element.look.clip && element.look.draws_box());
    assert_eq!(element.look.border_width(), 2.0);
    let text = Element::text("a", STYLE).colour(white);
    assert!(matches!(text.kind, Kind::Text { colour: Some(c), .. } if c == white));
}

/// `rich` joins its pieces into one wrapping text, with a span for each
/// piece over its bytes, and a link for each piece that is one.
#[test]
fn rich_text_keeps_its_pieces_as_spans_and_links() {
    use super::Inline;
    use crate::access::{Access, Action, Role};
    use loom_ui_core::{Color, Face};
    let blue = Color::rgb(0.0, 0.0, 1.0);
    let element = Element::rich(
        [
            Inline::new("Use "),
            Inline::new("delay1").mono(),
            Inline::new(", see "),
            Inline::new("Saw")
                .colour(blue)
                .underline()
                .link("saw", Access::new(Role::Link, "Saw").action(Action::Click)),
        ],
        STYLE,
    );
    let Kind::Text {
        text,
        spans,
        links,
        wrap,
        ..
    } = element.kind
    else {
        panic!("a text");
    };
    assert_eq!(text, "Use delay1, see Saw");
    assert!(wrap, "a paragraph wraps");
    assert_eq!(spans.iter().map(|span| span.len).collect::<Vec<_>>(), [4, 6, 6, 3]);
    assert_eq!(spans[1].face, Face::Mono);
    assert!(spans[3].underline && spans[3].colour == Some(blue));
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].range, 16..19);
    assert_eq!(links[0].key, IdKey::from("saw"));
}
