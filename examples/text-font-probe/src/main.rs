//! Prints, for each text crate candidate, which font face it draws the
//! system sans-serif with and how wide it sets a few tile labels.
//!
//! Four paths, so the candidates can be compared with what the app does
//! today:
//!
//! 1. `today`: Iced 0.14's paragraph, called as `loom-canvas` measures tile
//!    text (`Font::DEFAULT`, simple shaping, top-left aligned, no wrap).
//! 2. `cosmic-text 0.15`: the same crate Iced uses, called directly.
//! 3. `cosmic-text 0.19`: the newest release.
//! 4. `parley sans-serif`: Parley 0.12 with the generic `sans-serif`
//!    family, its default.
//! 5. `parley system-ui`: Parley 0.12 with the generic `system-ui` family,
//!    which CSS defines as the platform's interface font.
//!
//! Run it with `cargo run --release` in this folder. It prints the machine's
//! OS, then one line per path, size, and label.

use iced_core::text::Paragraph as _;
use std::collections::BTreeSet;

/// Labels from the example patches, and the sizes the canvas uses: tile
/// labels at 9 px, titles at 11 px, and 13 px for panel text.
const LABELS: [&str; 6] = [
    "Cutoff",
    "Resonance",
    "Oscillator",
    "800 Hz",
    "Envelope",
    "Attack 12 ms",
];
const SIZES: [f32; 3] = [9.0, 11.0, 13.0];

/// One measurement: the width of `label` at `size`, and the faces its
/// glyphs came from.
struct Measured {
    width: f32,
    faces: BTreeSet<String>,
}

/// Today's measure: Iced's paragraph through its global font system.
fn today(label: &str, size: f32) -> Measured {
    let paragraph = iced_graphics::text::Paragraph::with_text(iced_core::Text {
        content: label,
        bounds: iced_core::Size::new(f32::INFINITY, f32::INFINITY),
        size: iced_core::Pixels(size),
        line_height: iced_core::text::LineHeight::Relative(1.3),
        font: iced_core::Font::DEFAULT,
        align_x: iced_core::text::Alignment::Left,
        align_y: iced_core::alignment::Vertical::Top,
        shaping: iced_core::text::Shaping::Basic,
        wrapping: iced_core::text::Wrapping::None,
    });
    let width = paragraph.min_bounds().width;
    let mut system = iced_graphics::text::font_system()
        .write()
        .expect("the font system lock");
    let db = system.raw().db();
    let faces = paragraph
        .buffer()
        .layout_runs()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.font_id))
        .filter_map(|id| db.face(id).map(|face| face.post_script_name.clone()))
        .collect();
    Measured { width, faces }
}

/// cosmic-text 0.15, as Iced sets it up but without its icon font.
fn cosmic_15(system: &mut cosmic_text_15::FontSystem, label: &str, size: f32) -> Measured {
    use cosmic_text_15::{Attrs, Buffer, Family, Metrics, Shaping};
    let mut buffer = Buffer::new(system, Metrics::new(size, size * 1.3));
    buffer.set_size(system, None, None);
    buffer.set_text(
        system,
        label,
        &Attrs::new().family(Family::SansSerif),
        Shaping::Basic,
        None,
    );
    buffer.shape_until_scroll(system, false);
    let width = buffer.layout_runs().map(|run| run.line_w).fold(0.0, f32::max);
    let faces = buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.font_id))
        .filter_map(|id| system.db().face(id).map(|face| face.post_script_name.clone()))
        .collect();
    Measured { width, faces }
}

/// cosmic-text 0.19, set up the same way.
fn cosmic_19(system: &mut cosmic_text_19::FontSystem, label: &str, size: f32) -> Measured {
    use cosmic_text_19::{Attrs, Buffer, Family, Metrics, Shaping};
    let mut buffer = Buffer::new(system, Metrics::new(size, size * 1.3));
    buffer.set_size(None, None);
    buffer.set_text(label, &Attrs::new().family(Family::SansSerif), Shaping::Basic, None);
    buffer.shape_until_scroll(system, false);
    let width = buffer.layout_runs().map(|run| run.line_w).fold(0.0, f32::max);
    let faces = buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.font_id))
        .filter_map(|id| system.db().face(id).map(|face| face.post_script_name.clone()))
        .collect();
    Measured { width, faces }
}

/// The PostScript name of a face Parley hands back, read with skrifa.
fn parley_face(font: &parley::FontData) -> String {
    use skrifa::MetadataProvider as _;
    skrifa::FontRef::from_index(font.data.data(), font.index)
        .ok()
        .and_then(|face| {
            face.localized_strings(skrifa::string::StringId::POSTSCRIPT_NAME)
                .english_or_first()
                .map(|name| name.to_string())
        })
        .unwrap_or_else(|| "(unnamed)".to_string())
}

/// Parley 0.12 with `family`: the generic sans-serif (its default), the
/// generic system UI family, or a family by name.
fn parley(
    fonts: &mut parley::FontContext,
    layouts: &mut parley::LayoutContext<[u8; 4]>,
    family: parley::FontFamily<'static>,
    label: &str,
    size: f32,
) -> Measured {
    let mut builder = layouts.ranged_builder(fonts, label, 1.0, true);
    // The brush (a colour) does not change the layout.
    builder.push_default(parley::StyleProperty::FontSize(size));
    builder.push_default(parley::StyleProperty::FontFamily(family));
    let mut layout: parley::Layout<[u8; 4]> = builder.build(label);
    layout.break_all_lines(None);
    let mut faces = BTreeSet::new();
    for line in layout.lines() {
        for item in line.items() {
            if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                faces.insert(parley_face(run.run().font()));
            }
        }
    }
    Measured {
        width: layout.width(),
        faces,
    }
}

fn main() {
    println!("os: {} {}", std::env::consts::OS, std::env::consts::ARCH);
    let mut ct15 = cosmic_text_15::FontSystem::new();
    let mut ct19 = cosmic_text_19::FontSystem::new();
    let mut fonts = parley::FontContext::new();
    let mut layouts = parley::LayoutContext::<[u8; 4]>::new();
    println!("{:<18} {:>4}  {:<14} {:>8}  faces", "path", "px", "label", "width");
    for size in SIZES {
        for label in LABELS {
            let rows = [
                ("today (Iced 0.14)", today(label, size)),
                ("cosmic-text 0.15", cosmic_15(&mut ct15, label, size)),
                ("cosmic-text 0.19", cosmic_19(&mut ct19, label, size)),
                (
                    "parley sans-serif",
                    parley(
                        &mut fonts,
                        &mut layouts,
                        parley::GenericFamily::SansSerif.into(),
                        label,
                        size,
                    ),
                ),
                (
                    "parley system-ui",
                    parley(
                        &mut fonts,
                        &mut layouts,
                        parley::GenericFamily::SystemUi.into(),
                        label,
                        size,
                    ),
                ),
            ];
            for (path, measured) in rows {
                let faces: Vec<_> = measured.faces.into_iter().collect();
                println!(
                    "{path:<18} {size:>4}  {label:<14} {:>8.3}  {}",
                    measured.width,
                    faces.join(", ")
                );
            }
        }
    }
}
