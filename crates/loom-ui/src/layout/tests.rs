//! Tests for `layout.rs`: sizes as flexbox gives them, text measuring and
//! what it costs, rounding, and keyed identity across rebuilds.

use super::Layout;
use crate::element::{Align, Element, Length, Padding};
use loom_ui_core::{Point, Rect, Size, TextMeasure, TextStyle, UiId, Weight};

/// A text style the fake sets at 8 pixels a character and 16 a line.
pub(crate) const STYLE: TextStyle = TextStyle {
    size: 16.0,
    line_height: 1.0,
    weight: Weight::Regular,
};

/// A fake text measure: every character is half the font size wide, a
/// line is the line height, and lines break between words. It counts its
/// calls.
#[derive(Default)]
pub(crate) struct Mono {
    /// How many times `measure` was called.
    pub(crate) calls: usize,
}

impl TextMeasure for Mono {
    fn measure(&mut self, text: &str, style: TextStyle, wrap_width: Option<f32>) -> Size {
        self.calls += 1;
        let advance = style.size / 2.0;
        let line = style.size * style.line_height;
        let width_of = |chars: usize| chars as f32 * advance;
        let Some(limit) = wrap_width else {
            return Size::new(width_of(text.chars().count()), line);
        };
        let (mut widest, mut lines, mut current) = (0.0_f32, 1, 0_usize);
        for word in text.split(' ') {
            let length = word.chars().count();
            if current == 0 {
                current = length;
            } else if width_of(current + 1 + length) <= limit {
                current += 1 + length;
            } else {
                widest = widest.max(width_of(current));
                lines += 1;
                current = length;
            }
        }
        Size::new(widest.max(width_of(current)), lines as f32 * line)
    }
}

/// `root` laid out for the panel `p` in `width` by `height`.
pub(crate) fn lay_out(root: Element, width: f32, height: f32) -> Layout {
    let mut layout = Layout::new(UiId::panel("p"));
    layout.rebuild(root);
    layout.compute(Size::new(width, height), &mut Mono::default());
    layout
}

/// The UI ID `p/<path>`, with `path`'s keys split at `/`.
pub(crate) fn id(path: &str) -> UiId {
    path.split('/').fold(UiId::panel("p"), UiId::with)
}

/// The bounds of `p/<key>`.
fn at(layout: &Layout, key: &str) -> Rect {
    layout.bounds(&id(key)).unwrap_or_else(|| panic!("no element p/{key}"))
}

/// A rectangle from its four numbers.
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect::new(Point::new(x, y), Size::new(width, height))
}

/// A keyed space of a fixed size.
fn space(key: &str, width: f32, height: f32) -> Element {
    Element::space(Length::Fixed(width), Length::Fixed(height)).key(key)
}

/// A panel laid out before it was ever given a tree is empty.
#[test]
fn an_empty_layout_has_no_size_and_no_bounds() {
    let mut layout = Layout::new(UiId::panel("p"));
    layout.compute(Size::new(100.0, 100.0), &mut Mono::default());
    assert_eq!(layout.size(), Size::ZERO);
    assert_eq!(layout.bounds(&id("a")), None);
    assert_eq!(layout.ids().count(), 0);
    assert_eq!(layout.panel(), &UiId::panel("p"));
}

/// Fixed sizes, spacing, and padding place children side by side.
#[test]
fn fixed_children_sit_side_by_side_inside_the_padding() {
    let row = Element::row([space("a", 40.0, 20.0), space("b", 30.0, 20.0)])
        .key("row")
        .spacing(10.0)
        .padding(5.0);
    let layout = lay_out(row, 300.0, 100.0);
    assert_eq!(at(&layout, "row/a"), rect(5.0, 5.0, 40.0, 20.0));
    assert_eq!(at(&layout, "row/b"), rect(55.0, 5.0, 30.0, 20.0));
    // The row shrinks to its content: 5 + 40 + 10 + 30 + 5.
    assert_eq!(layout.size(), Size::new(90.0, 30.0));
}

/// Fill shares split what is left after the fixed children.
#[test]
fn fill_shares_split_the_space_left_over() {
    let row = Element::row([
        space("fixed", 60.0, 10.0),
        Element::space(Length::FILL, Length::Fixed(10.0)).key("one"),
        Element::space(Length::fill(2), Length::Fixed(10.0)).key("two"),
    ])
    .width(Length::FILL);
    let layout = lay_out(row, 300.0, 100.0);
    assert_eq!(layout.size().width, 300.0);
    assert_eq!(at(&layout, "one"), rect(60.0, 0.0, 80.0, 10.0));
    assert_eq!(at(&layout, "two"), rect(140.0, 0.0, 160.0, 10.0));
}

/// Flexbox grows each fill item from its content's size, so two fill items
/// with different content end up of different widths: the space left over
/// is split equally, not the whole row.
#[test]
fn fill_items_grow_from_their_content_so_their_widths_differ() {
    let row = Element::row([
        Element::text("a", STYLE).width(Length::FILL).key("short"),
        Element::text("abcd", STYLE).width(Length::FILL).key("long"),
    ])
    .width(Length::FILL);
    let layout = lay_out(row, 100.0, 100.0);
    // 8 and 32 pixels of text; the 60 left over goes 30 to each.
    assert_eq!(at(&layout, "short").width, 38.0);
    assert_eq!(at(&layout, "long").width, 62.0);
}

/// A fixed size is what an item takes when there is room. With flexbox's
/// default, it also shrinks when the row is too short: two 80-pixel items
/// in a 100-pixel row get 50 each.
#[test]
fn a_fixed_size_shrinks_when_the_row_is_too_short() {
    let row = Element::row([space("a", 80.0, 10.0), space("b", 80.0, 10.0)]).width(Length::Fixed(100.0));
    let layout = lay_out(row, 300.0, 100.0);
    assert_eq!(at(&layout, "a"), rect(0.0, 0.0, 50.0, 10.0));
    assert_eq!(at(&layout, "b"), rect(50.0, 0.0, 50.0, 10.0));
}

/// No item shrinks below its narrowest content: a line of text that does
/// not wrap keeps its width and runs past a row too short for it.
#[test]
fn text_on_one_line_never_shrinks_below_its_width() {
    let row = Element::row([Element::text("abcdefgh", STYLE).key("label")]).width(Length::Fixed(50.0));
    let layout = lay_out(row, 300.0, 100.0);
    assert_eq!(at(&layout, "label").width, 64.0);
    assert_eq!(layout.size().width, 50.0);
}

/// A shrink size is the content's size when there is room.
#[test]
fn a_shrink_size_is_the_content_size() {
    let row = Element::row([Element::text("abc", STYLE).key("label")]).width(Length::FILL);
    let layout = lay_out(row, 300.0, 100.0);
    assert_eq!(at(&layout, "label"), rect(0.0, 0.0, 24.0, 16.0));
}

/// Across a row, an item with no alignment set stretches to the row's
/// height, as flexbox does; with an alignment, it keeps its own height,
/// unless its height is a fill size.
#[test]
fn across_a_row_items_stretch_unless_aligned() {
    let children = || {
        [
            Element::text("a", STYLE).key("text"),
            Element::space(Length::Fixed(10.0), Length::FILL).key("fill"),
        ]
    };
    let plain = lay_out(Element::row(children()).height(Length::Fixed(40.0)), 300.0, 100.0);
    assert_eq!(at(&plain, "text").height, 40.0);

    let centred = lay_out(
        Element::row(children())
            .height(Length::Fixed(40.0))
            .align_y(Align::Center),
        300.0,
        100.0,
    );
    assert_eq!(at(&centred, "text"), rect(0.0, 12.0, 8.0, 16.0));
    assert_eq!(at(&centred, "fill"), rect(8.0, 0.0, 10.0, 40.0));
}

/// Along a row, alignment places the children as a group.
#[test]
fn along_a_row_alignment_moves_the_group() {
    let row = |align| {
        Element::row([space("a", 20.0, 10.0), space("b", 20.0, 10.0)])
            .width(Length::Fixed(100.0))
            .align_x(align)
    };
    assert_eq!(at(&lay_out(row(Align::Start), 300.0, 100.0), "a").x, 0.0);
    assert_eq!(at(&lay_out(row(Align::Center), 300.0, 100.0), "a").x, 30.0);
    assert_eq!(at(&lay_out(row(Align::End), 300.0, 100.0), "b").x, 80.0);
}

/// A column stacks its children downwards, and aligns them across.
#[test]
fn a_column_places_children_downwards() {
    let column = Element::column([space("a", 20.0, 10.0), space("b", 40.0, 10.0)])
        .width(Length::Fixed(100.0))
        .spacing(4.0)
        .align_x(Align::End);
    let layout = lay_out(column, 300.0, 300.0);
    assert_eq!(at(&layout, "a"), rect(80.0, 0.0, 20.0, 10.0));
    assert_eq!(at(&layout, "b"), rect(60.0, 14.0, 40.0, 10.0));

    // Along a column, a fill height takes the space left over.
    let column = Element::column([
        space("top", 20.0, 10.0),
        Element::space(Length::Fixed(20.0), Length::FILL).key("rest"),
    ])
    .height(Length::FILL);
    let layout = lay_out(column, 300.0, 100.0);
    assert_eq!(at(&layout, "rest"), rect(0.0, 10.0, 20.0, 90.0));

    // Across a column, a fill width stretches to the column's width.
    let column = Element::column([
        space("wide", 60.0, 10.0),
        Element::space(Length::FILL, Length::Fixed(10.0)).key("fill"),
    ])
    .align_x(Align::Start);
    let layout = lay_out(column, 300.0, 100.0);
    assert_eq!(at(&layout, "fill").width, 60.0);
}

/// A stack is as large as its largest child, and aligns each child in it.
#[test]
fn a_stack_is_its_largest_child_and_aligns_the_rest() {
    let stack = Element::stack([space("big", 100.0, 50.0), space("small", 20.0, 10.0)])
        .align_x(Align::Center)
        .align_y(Align::Center);
    let layout = lay_out(stack, 300.0, 300.0);
    assert_eq!(layout.size(), Size::new(100.0, 50.0));
    assert_eq!(at(&layout, "small"), rect(40.0, 20.0, 20.0, 10.0));

    // A fill child stretches over the whole stack, aligned or not.
    let stack = Element::stack([
        space("big", 100.0, 50.0),
        Element::space(Length::FILL, Length::FILL).key("cover"),
    ])
    .align_x(Align::Center)
    .align_y(Align::Center);
    let layout = lay_out(stack, 300.0, 300.0);
    assert_eq!(at(&layout, "cover"), rect(0.0, 0.0, 100.0, 50.0));
}

/// A minimum and a maximum hold a size between them.
#[test]
fn minimum_and_maximum_sizes_hold() {
    let row = Element::row([
        Element::space(Length::FILL, Length::Fixed(10.0))
            .max_width(50.0)
            .key("capped"),
        Element::space(Length::Shrink, Length::Shrink)
            .min_width(30.0)
            .min_height(12.0)
            .key("floored"),
    ])
    .width(Length::FILL);
    let layout = lay_out(row, 300.0, 100.0);
    assert_eq!(at(&layout, "capped").width, 50.0);
    assert_eq!(at(&layout, "floored").width, 30.0);

    let column = Element::column([Element::space(Length::Fixed(10.0), Length::FILL)
        .max_height(40.0)
        .key("short")])
    .height(Length::FILL);
    assert_eq!(at(&lay_out(column, 300.0, 100.0), "short").height, 40.0);
}

/// A wrapping row starts a new line when its children do not fit, with
/// its spacing between the lines too.
#[test]
fn a_wrapping_row_breaks_into_lines() {
    let chips = ["a", "b", "c", "d", "e"].map(|key| space(key, 30.0, 10.0));
    let row = Element::row(chips).width(Length::Fixed(100.0)).spacing(4.0).wrap();
    let layout = lay_out(row, 300.0, 300.0);
    assert_eq!(at(&layout, "c"), rect(68.0, 0.0, 30.0, 10.0));
    assert_eq!(at(&layout, "d"), rect(0.0, 14.0, 30.0, 10.0));
    assert_eq!(layout.size().height, 24.0);
}

/// Wrapping text is as tall as the lines it needs at the width it gets.
#[test]
fn wrapping_text_is_as_tall_as_its_lines_at_its_width() {
    let column =
        Element::column([Element::text("aaaa bbbb cccc", STYLE).wrap().key("text")]).width(Length::Fixed(80.0));
    let layout = lay_out(column, 300.0, 300.0);
    // "aaaa bbbb" fits 80 pixels; "cccc" goes on a second line.
    assert_eq!(at(&layout, "text"), rect(0.0, 0.0, 80.0, 32.0));

    // Wider, it is one line.
    let column =
        Element::column([Element::text("aaaa bbbb cccc", STYLE).wrap().key("text")]).width(Length::Fixed(200.0));
    assert_eq!(at(&lay_out(column, 300.0, 300.0), "text").height, 16.0);
}

/// The root takes the whole space along a fill axis, and its content's
/// size along a shrink axis.
#[test]
fn the_root_fills_or_shrinks_in_the_space_it_has() {
    let root = || Element::row([space("a", 20.0, 10.0)]);
    assert_eq!(
        lay_out(root().width(Length::FILL), 300.0, 100.0).size(),
        Size::new(300.0, 10.0)
    );
    assert_eq!(
        lay_out(root().height(Length::FILL), 300.0, 100.0).size(),
        Size::new(20.0, 100.0)
    );
    assert_eq!(
        lay_out(root().width(Length::Fixed(50.0)), 300.0, 100.0).size(),
        Size::new(50.0, 10.0)
    );
}

/// A resize lays the same tree out again, with no rebuild.
#[test]
fn a_resize_moves_the_parts_without_a_rebuild() {
    let row = Element::row([
        Element::space(Length::FILL, Length::Fixed(10.0)),
        space("end", 20.0, 10.0),
    ])
    .width(Length::FILL);
    let mut layout = lay_out(row, 300.0, 100.0);
    assert_eq!(at(&layout, "end").x, 280.0);
    layout.compute(Size::new(200.0, 100.0), &mut Mono::default());
    assert_eq!(at(&layout, "end").x, 180.0);
}

/// Edges are rounded from their place in the panel, never widths, so
/// seven fill items across 100 pixels meet with no gap and no overlap,
/// and the last ends exactly at the row's end.
#[test]
fn rounded_edges_tile_with_no_gap_or_overlap() {
    let keys = ["a", "b", "c", "d", "e", "f", "g"];
    let row = Element::row(keys.map(|key| Element::space(Length::FILL, Length::Fixed(10.0)).key(key)))
        .width(Length::FILL)
        .padding(Padding::symmetric(0.0, 0.5));
    let layout = lay_out(row, 101.0, 100.0);
    let mut edge = at(&layout, "a").x;
    for key in keys {
        let bounds = at(&layout, key);
        assert_eq!(bounds.x, edge, "{key} starts where the one before it ends");
        assert_eq!(
            bounds.width,
            bounds.width.round(),
            "{key} is a whole number of pixels wide"
        );
        edge = bounds.x + bounds.width;
    }
    assert_eq!(edge.round(), edge);
    assert!((100.0..=101.0).contains(&edge));
}

/// A line of text is measured once until the panel rebuilds, however many
/// times the layout asks and whatever width it offers: 50 labels cost 50
/// measures, a resize costs none, and a rebuild measures them again.
#[test]
fn one_line_text_is_measured_once_per_rebuild() {
    let labels = || {
        Element::row((0..50_u64).map(|index| Element::text(format!("label {index}"), STYLE).key(index)))
            .width(Length::FILL)
            .wrap()
    };
    let mut mono = Mono::default();
    let mut layout = Layout::new(UiId::panel("p"));
    layout.rebuild(labels());
    layout.compute(Size::new(400.0, 300.0), &mut mono);
    assert_eq!(mono.calls, 50);
    layout.compute(Size::new(300.0, 300.0), &mut mono);
    assert_eq!(mono.calls, 50, "a resize measures nothing again");
    layout.rebuild(labels());
    layout.compute(Size::new(300.0, 300.0), &mut mono);
    assert_eq!(mono.calls, 100, "a rebuild measures each label once");
}

/// Wrapping text is measured at most three times in one layout: at its
/// narrowest, on one line, and at the width it gets.
#[test]
fn wrapping_text_is_measured_at_most_three_times_a_layout() {
    let row = Element::row([
        Element::text("some words that wrap", STYLE).wrap().key("text"),
        space("beside", 40.0, 10.0),
    ])
    .width(Length::Fixed(120.0));
    let mut mono = Mono::default();
    let mut layout = Layout::new(UiId::panel("p"));
    layout.rebuild(row);
    layout.compute(Size::new(300.0, 300.0), &mut mono);
    assert!(mono.calls <= 3, "measured {} times", mono.calls);
    // Both shrink by their share of 160 and 40 pixels: the text to 96,
    // the fixed space to 24.
    assert_eq!(at(&layout, "text").width, 96.0);
    assert_eq!(at(&layout, "beside").width, 24.0);
}

/// An element's UI ID is the panel's key, then the keys around it, then
/// its own; an element with no key adds nothing.
#[test]
fn a_ui_id_is_the_path_of_keys() {
    let list = Element::column([Element::row([Element::text("Name", STYLE).key("name")]).key(7_u64)]).key("list");
    let wrapped = Element::column([Element::row([list])]);
    let layout = lay_out(wrapped, 300.0, 300.0);
    let ids: Vec<String> = layout.ids().map(ToString::to_string).collect();
    assert_eq!(ids, ["p/list", "p/list/7", "p/list/7/name"]);
}

/// Keys come from what a row shows, so removing a row leaves the others'
/// IDs alone, and only their places move.
#[test]
fn removing_a_row_keeps_the_other_rows_ids() {
    let rows = |keys: &[u64]| Element::column(keys.iter().map(|key| space("", 50.0, 20.0).key(*key)));
    let mut layout = lay_out(rows(&[3, 5, 9]), 300.0, 300.0);
    let nine = UiId::panel("p").with(9_u64);
    assert_eq!(layout.bounds(&nine).map(|bounds| bounds.y), Some(40.0));

    layout.rebuild(rows(&[3, 9]));
    layout.compute(Size::new(300.0, 300.0), &mut Mono::default());
    let ids: Vec<String> = layout.ids().map(ToString::to_string).collect();
    assert_eq!(ids, ["p/3", "p/9"]);
    assert_eq!(layout.bounds(&nine).map(|bounds| bounds.y), Some(20.0));
}

/// Wrapping a part in another element with no key changes no UI ID.
#[test]
fn an_unkeyed_wrapper_changes_no_id() {
    let bare = lay_out(Element::column([space("a", 10.0, 10.0)]), 300.0, 300.0);
    let wrapped = lay_out(
        Element::column([Element::row([Element::stack([space("a", 10.0, 10.0)])])]),
        300.0,
        300.0,
    );
    assert_eq!(bare.ids().collect::<Vec<_>>(), wrapped.ids().collect::<Vec<_>>());
}

/// In a debug build, two elements with one UI ID panic, naming it.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "two elements have the UI ID p/a")]
fn a_duplicate_key_panics_in_a_debug_build() {
    lay_out(
        Element::row([space("a", 10.0, 10.0), space("a", 10.0, 10.0)]),
        300.0,
        300.0,
    );
}

/// In a release build, the first element in tree order keeps a repeated
/// UI ID and the later ones have none, so the result is the same every
/// time.
#[test]
fn a_duplicate_key_in_a_release_build_keeps_the_first() {
    let mut layout = Layout::new(UiId::panel("p"));
    layout.allow_duplicates();
    layout.rebuild(Element::row([space("a", 10.0, 10.0), space("a", 30.0, 10.0)]));
    layout.compute(Size::new(300.0, 300.0), &mut Mono::default());
    assert_eq!(layout.duplicate_keys(), 1);
    assert_eq!(layout.ids().count(), 1);
    assert_eq!(at(&layout, "a"), rect(0.0, 0.0, 10.0, 10.0));
    // A rebuild starts the count again.
    layout.rebuild(Element::row([space("a", 10.0, 10.0)]));
    assert_eq!(layout.duplicate_keys(), 0);
}

/// Nothing inside a lost duplicate gets a UI ID: a key there would name a
/// place inside the first element, which may not have it.
#[test]
fn nothing_inside_a_duplicate_gets_an_id() {
    let mut layout = Layout::new(UiId::panel("p"));
    layout.allow_duplicates();
    layout.rebuild(Element::column([
        Element::row([space("value", 10.0, 10.0)]).key("row"),
        Element::row([
            space("name", 30.0, 10.0),
            Element::row([space("deeper", 5.0, 5.0)]).key("inner"),
        ])
        .key("row")
        .opaque(),
    ]));
    layout.compute(Size::new(300.0, 300.0), &mut Mono::default());
    // The second row, its name, its inner row, and what is in that.
    assert_eq!(layout.duplicate_keys(), 4);
    let ids: Vec<String> = layout.ids().map(ToString::to_string).collect();
    assert_eq!(ids, ["p/row", "p/row/value"]);
    assert_eq!(layout.bounds(&id("row/name")), None);
    assert_eq!(layout.bounds(&id("row/inner/deeper")), None);
}

/// A fill length has at least one share.
#[test]
#[should_panic(expected = "a fill length has at least one share")]
fn a_fill_of_no_shares_cannot_be_built() {
    let shares = std::hint::black_box(0);
    let _ = Length::fill(shares);
}
