//! Tests for the glyph atlas, with the machine's own fonts and no GPU.

use super::*;
use crate::painter::{Painter, TextMeasure, TextRun, TextStyle, Vertical, Weight};
use crate::text::{CanvasText, ListPainter, PaintList};
use crate::tile_text::Align;
use loom_ui_core::{Color, Size};
use std::cell::RefCell;

/// Everywhere: no cut.
const ALL: Rect = Rect {
    x: -1e6,
    y: -1e6,
    width: 2e6,
    height: 2e6,
};

/// `text` at 12 px with its top-left at `at`, as a paint list, or `None`
/// on a machine with no font, where these tests skip.
fn painted(text: &str, at: Point) -> Option<PaintList> {
    let canvas = RefCell::new(CanvasText::new());
    let style = TextStyle {
        size: 12.0,
        line_height: 1.2,
        weight: Weight::Regular,
    };
    let mut list = PaintList::new(ALL);
    let mut painter = ListPainter::new(&canvas, &mut list);
    if painter.measure("x", style, None).width <= 0.0 {
        eprintln!("skipped: no system font");
        return None;
    }
    painter.text(&TextRun {
        text,
        anchor: at,
        bounds: Size::new(500.0, 20.0),
        align: Align::Left,
        vertical: Vertical::Top,
        style,
        wrap: false,
        colour: Color::WHITE,
        clip: ALL,
    });
    Some(list)
}

/// Places every glyph of `list` at scale `scale`, keeping the quads.
fn place_all(atlas: &mut Atlas, list: &PaintList, scale: f32) -> Result<Vec<GlyphQuad>, Full> {
    let mut quads = Vec::new();
    for glyph in &list.glyphs {
        if let Some(quad) = atlas.place(glyph, &list.faces[glyph.face as usize], ALL, Point::ORIGIN, scale)? {
            quads.push(quad);
        }
    }
    Ok(quads)
}

/// A glyph's quad sits on whole physical pixels, its texels match its size
/// one to one, and its image is uploaded once.
#[test]
fn a_glyph_is_placed_on_whole_pixels_and_uploaded_once() {
    let Some(list) = painted("H", Point::new(10.3, 20.6)) else {
        return;
    };
    let mut atlas = Atlas::new(MAX_SIZE);
    atlas.begin_frame();
    for scale in [1.0, 2.0] {
        let quads = place_all(&mut atlas, &list, scale).unwrap();
        let [x, y, width, height] = quads[0].rect;
        for value in [x, y, width, height] {
            let physical = value * scale;
            assert!((physical - physical.round()).abs() < 1e-3, "{value} at {scale}");
        }
        let [u0, v0, u1, v1] = quads[0].uv;
        assert!((u1 - u0 - width * scale).abs() < 1e-3);
        assert!((v1 - v0 - height * scale).abs() < 1e-3);
        assert_eq!(quads[0].colour, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(quads[0].params[0], 0.0, "a letter is a coverage glyph");
    }
    assert_eq!(atlas.uploads.len(), 2, "one image a scale");
    place_all(&mut atlas, &list, 1.0).unwrap();
    assert_eq!(atlas.uploads.len(), 2, "placed again from the atlas");
    assert_eq!(atlas.uploads[0].page, Page::Mask);
    let upload = &atlas.uploads[0];
    assert_eq!(upload.data.len() as u32, upload.width * upload.height);
}

/// A glyph is made once for each quarter pixel across, and a whole pixel
/// further reuses the same image.
#[test]
fn quarter_pixels_make_their_own_images() {
    let mut atlas = Atlas::new(MAX_SIZE);
    atlas.begin_frame();
    // The lists are kept: a font file no list holds may load again as
    // another font (see `crate::text`).
    let mut lists = Vec::new();
    for x in [0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 7.5] {
        let Some(list) = painted("o", Point::new(x, 0.0)) else {
            return;
        };
        place_all(&mut atlas, &list, 1.0).unwrap();
        lists.push(list);
    }
    assert_eq!(atlas.len(), 4);
}

/// A space has no image: no quad and no upload.
#[test]
fn a_space_draws_nothing() {
    let Some(list) = painted(" ", Point::ORIGIN) else {
        return;
    };
    let mut atlas = Atlas::new(MAX_SIZE);
    atlas.begin_frame();
    assert!(place_all(&mut atlas, &list, 1.0).unwrap().is_empty());
    assert!(atlas.uploads.is_empty());
}

/// A cut glyph's quad and texels shrink together; a glyph wholly outside
/// its clip draws nothing.
#[test]
fn a_clip_cuts_the_quad_and_its_texels() {
    let Some(list) = painted("H", Point::new(10.0, 10.0)) else {
        return;
    };
    let mut atlas = Atlas::new(MAX_SIZE);
    atlas.begin_frame();
    let glyph = &list.glyphs[0];
    let face = &list.faces[0];
    let whole = atlas.place(glyph, face, ALL, Point::ORIGIN, 2.0).unwrap().unwrap();
    let [x, y, width, height] = whole.rect;
    let half = Rect::new(Point::new(x, y), Size::new(width / 2.0, height));
    let cut = atlas.place(glyph, face, half, Point::ORIGIN, 2.0).unwrap().unwrap();
    assert_eq!(cut.rect, [x, y, width / 2.0, height]);
    assert!(
        (cut.uv[2] - cut.uv[0] - width).abs() < 1e-3,
        "half the width in texels at scale 2"
    );
    assert_eq!(cut.uv[0], whole.uv[0]);
    let away = Rect::new(Point::new(1000.0, 1000.0), Size::new(5.0, 5.0));
    assert_eq!(atlas.place(glyph, face, away, Point::ORIGIN, 2.0), Ok(None));
}

/// The canvas's corner moves every quad by the same amount.
#[test]
fn the_canvas_corner_moves_the_quads() {
    let Some(list) = painted("H", Point::new(110.0, 60.0)) else {
        return;
    };
    let mut atlas = Atlas::new(MAX_SIZE);
    atlas.begin_frame();
    let mut at = |origin| {
        atlas
            .place(&list.glyphs[0], &list.faces[0], ALL, origin, 1.0)
            .unwrap()
            .unwrap()
            .rect
    };
    let [x0, y0, ..] = at(Point::ORIGIN);
    let [x1, y1, ..] = at(Point::new(100.0, 50.0));
    assert_eq!((x0 - x1, y0 - y1), (100.0, 50.0));
}

/// When a page is full, glyphs the frame has not used are freed for new
/// ones. Glyphs the frame uses are kept, and when they alone fill it, the
/// atlas is full.
#[test]
fn a_full_page_frees_what_the_frame_did_not_use() {
    // The same letters a quarter physical pixel apart: other images of
    // the same sizes, so the freed room fits them whatever the font.
    let letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let (Some(first), Some(second)) = (
        painted(letters, Point::ORIGIN),
        painted(letters, Point::new(0.125, 0.0)),
    ) else {
        return;
    };
    // The smallest page that holds one alphabet at scale 2. Its sides are
    // found, not fixed, because glyph sizes differ from font to font.
    let side = (16..=1024)
        .step_by(8)
        .find(|&side| {
            let mut atlas = Atlas::sized(side, side);
            atlas.begin_frame();
            place_all(&mut atlas, &first, 2.0).is_ok()
        })
        .expect("a page that holds one alphabet");
    let mut atlas = Atlas::sized(side, side);
    atlas.begin_frame();
    place_all(&mut atlas, &first, 2.0).unwrap();
    let mut both = Atlas::sized(side, side);
    both.begin_frame();
    place_all(&mut both, &first, 2.0).unwrap();
    assert_eq!(
        place_all(&mut both, &second, 2.0),
        Err(Full),
        "one frame cannot hold both"
    );
    atlas.begin_frame();
    place_all(&mut atlas, &second, 2.0).expect("the first alphabet's glyphs are freed");
    assert!(atlas.len() < 52);

    let mut tiny = Atlas::sized(side - 8, side - 8);
    tiny.begin_frame();
    assert_eq!(place_all(&mut tiny, &first, 2.0), Err(Full));
}

/// The atlas doubles until it reaches its largest size, and empties as it
/// does, so the frame is placed again in full.
#[test]
fn the_atlas_grows_to_its_largest_size() {
    let mut atlas = Atlas::sized(64, 256);
    atlas.resized = false;
    atlas.uploads.push(Upload {
        page: Page::Mask,
        x: 0,
        y: 0,
        width: 0,
        height: 0,
        data: Vec::new(),
    });
    assert!(atlas.grow());
    assert_eq!(atlas.size(), 128);
    assert!(atlas.resized && atlas.uploads.is_empty());
    assert!(atlas.grow());
    assert_eq!(atlas.size(), 256);
    assert!(!atlas.grow(), "at its largest");
    assert_eq!(atlas.size(), 256);
    assert_eq!(Atlas::new(1 << 20).max, MAX_SIZE, "never past the largest size");
    let shown = format!("{atlas:?}");
    assert!(shown.contains("size: 256"), "{shown}");
}

/// An emoji keeps its own colours, in the colour page, premultiplied, at
/// the size asked for, not at its font's nearest strike.
#[test]
fn an_emoji_goes_to_the_colour_page() {
    let Some(list) = painted("😀", Point::ORIGIN) else {
        return;
    };
    let mut atlas = Atlas::new(MAX_SIZE);
    atlas.begin_frame();
    let quads = place_all(&mut atlas, &list, 2.0).unwrap();
    let Some(upload) = atlas.uploads.first() else {
        eprintln!("skipped: no emoji font");
        return;
    };
    if upload.page != Page::Colour {
        eprintln!("skipped: the emoji font has no colour images");
        return;
    }
    assert_eq!(quads[0].params[0], 1.0);
    // 12 px at scale 2: about 24 physical pixels, where a strike would be
    // 40 to 160.
    let size = upload.width.max(upload.height);
    assert!((16..=32).contains(&size), "{size} pixels across");
    assert_eq!(upload.data.len() as u32, upload.width * upload.height * 4);
    assert!(
        upload
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .all(|t| t[0] <= t[3] && t[1] <= t[3] && t[2] <= t[3]),
        "no channel brighter than its alpha"
    );
}

/// Straight colour becomes premultiplied: each channel scaled by alpha.
#[test]
fn premultiplying_scales_by_alpha() {
    let mut data = vec![255, 128, 0, 128, 200, 200, 200, 255, 10, 20, 30, 0];
    premultiply(&mut data);
    assert_eq!(data, [128, 64, 0, 128, 200, 200, 200, 255, 0, 0, 0, 0]);
    assert_eq!(Page::Mask.bytes(), 1);
    assert_eq!(Page::Colour.bytes(), 4);
}
