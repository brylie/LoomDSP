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
