//! The text gate (docs/dev/leaving-iced-plan.md, step 2.4b, item 3): a
//! tool run by hand when a view's text moves from Iced to our own, not a
//! test that runs in CI. It is `#[ignore]`d and lives with the tests, so
//! nothing of it reaches the app.
//!
//! For each example patch it
//!
//! 1. measures every tile label with Iced's paragraph (today's measure,
//!    still a dependency until phase 4) and with `CanvasText`, and reports
//!    the change in width per label: least, greatest, and median;
//! 2. reports every label whose new width leaves its tile text box, which
//!    `fit_label` sizes by an estimate;
//! 3. draws the whole window as `--screenshot-window` does and compares it
//!    with the image from before the change, ignoring only the pixels
//!    inside the moved text: the union of each label's old and new box,
//!    two physical pixels wider all round for ink past the advance.
//!
//! Run it with the images from the base commit in a folder:
//!
//! ```sh
//! LOOM_TEXT_GATE_BEFORE=/path/to/before cargo test --release -p loom-app \
//!     text_gate -- --ignored --nocapture
//! ```
//!
//! The folder holds `NAME.png` for each patch `NAME.ron`, made at the base
//! commit with `--screenshot-window NAME.png --scale 2`. Without the
//! variable it measures only, and writes this commit's images to the
//! system's temporary folder, so they can be the next baseline. It needs a
//! GPU adapter either way, since the labels are found in the window it
//! draws; without one it stops and says so.

use super::*;
use clap::Parser as _;
use iced::advanced::text::{self, Paragraph as _};
use loom_canvas::{Align, CanvasText, TextKind, TextStyle, Weight, for_each_tile_text};
use loom_ui_core::Rect;

/// Physical pixels per logical pixel for the gate's images.
const SCALE: f32 = 2.0;
/// Extra room around a text box for ink past its advance, in physical
/// pixels.
const INK: f32 = 2.0;

/// The style tile text is set in (`input/paint.rs`).
fn style(size: f32) -> TextStyle {
    TextStyle {
        size,
        line_height: 1.2,
        weight: Weight::Regular,
    }
}

/// Today's width of `text` at `size`: Iced's paragraph as the canvas set
/// tile text before step 2.4b, with simple shaping.
fn iced_width(text: &str, size: f32) -> f32 {
    <iced::Renderer as text::Renderer>::Paragraph::with_text(iced::advanced::Text {
        content: text,
        bounds: iced::Size::new(f32::INFINITY, f32::INFINITY),
        size: Pixels(size),
        line_height: text::LineHeight::Relative(1.2),
        font: Font::DEFAULT,
        align_x: text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: text::Shaping::Basic,
        wrapping: text::Wrapping::None,
    })
    .min_bounds()
    .width
}

/// The example patches, by name.
fn patches() -> Vec<std::path::PathBuf> {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    let mut found: Vec<_> = std::fs::read_dir(folder)
        .expect("the example patches")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "ron"))
        .collect();
    found.sort();
    found
}

/// One label measured both ways.
struct Label {
    patch: String,
    text: String,
    size: f32,
    old: f32,
    new: f32,
    /// The room its tile gives it.
    room: f32,
}

/// The window drawn as `--screenshot-window` draws it, with where the
/// canvas is and its camera, or `None` without a GPU.
fn window(patch: &std::path::Path) -> Option<(crate::App, CanvasView, Size<u32>, Vec<u8>)> {
    let path = patch.to_string_lossy();
    let args = crate::Args::try_parse_from(["loom-app", &path, "--screenshot-window", "gate.png", "--scale", "2"])
        .expect("the gate's arguments");
    let mut app = crate::snapshot::headless(&args).expect("the patch loads");
    let options = &args.view;
    let logical = crate::WINDOW;
    let physical = physical_size(logical, options.scale).expect("a size");
    let mut renderer = iced::futures::executor::block_on(<iced::Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))?;
    let mut ui = UserInterface::build(app.view(), logical, Cache::default(), &mut renderer);
    let canvas = canvas_view(&mut ui, &mut renderer).expect("a canvas");
    drop(ui);
    let (scene, shown) = shown(
        options,
        &mut app.session.controller,
        &app.session.hierarchy,
        &app.session.level,
        &app.session.positions,
        &app.session.sizes,
        canvas.bounds.size(),
    )
    .expect("the view");
    app.session.scene = scene;
    app.refresh_values();
    app.still = Some(shown.still);
    let pixels = draw_window(&app, &mut renderer, logical, options.scale);
    // The camera the still frame shows, as the canvas reports it.
    let mut ui = UserInterface::build(app.view(), logical, Cache::default(), &mut renderer);
    let canvas = canvas_view(&mut ui, &mut renderer).expect("a canvas");
    drop(ui);
    Some((app, canvas, physical, pixels))
}

/// Reads a PNG as (width, height, RGBA pixels).
fn read_png(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).expect("the image")));
    let mut reader = decoder.read_info().expect("a PNG");
    let mut pixels = vec![0; reader.output_buffer_size().expect("a sane size")];
    let info = reader.next_frame(&mut pixels).expect("PNG data");
    pixels.truncate(info.buffer_size());
    (info.width, info.height, pixels)
}

/// The least, greatest, and median of `values`, which must not be empty.
fn spread(mut values: Vec<f32>) -> (f32, f32, f32) {
    values.sort_by(f32::total_cmp);
    (values[0], values[values.len() - 1], values[values.len() / 2])
}

/// The gate. See the module's doc.
#[test]
#[ignore = "a tool for the text gate, run by hand"]
fn text_gate() {
    let started = std::time::Instant::now();
    let mut text = CanvasText::new();
    text.measure("Cutoff", style(9.0), None);
    println!(
        "first layout, with the font scan: {:.1} ms",
        started.elapsed().as_secs_f64() * 1e3
    );

    let before = std::env::var_os("LOOM_TEXT_GATE_BEFORE").map(std::path::PathBuf::from);
    let out = std::env::temp_dir().join("loom-text-gate");
    std::fs::create_dir_all(&out).expect("the output folder");
    let mut labels = Vec::new();
    let mut failed = false;
    for patch in patches() {
        let name = patch.file_stem().unwrap().to_string_lossy().to_string();
        let Some((app, canvas, physical, pixels)) = window(&patch) else {
            // The labels' places come from the window's scene and camera,
            // which need the renderer too, so there is nothing to measure.
            println!("{NO_GPU}: the gate needs one, nothing was measured");
            return;
        };
        let image = out.join(format!("{name}.png"));
        write_png(&image, physical, &pixels).expect("the image is written");

        // The text boxes, old and new, in the window's physical pixels.
        let camera = canvas.camera;
        let zoom = camera.zoom;
        let quantise = |size: f32| (size * 2.0).round() / 2.0;
        let mut mask: Vec<Rect> = Vec::new();
        let screen = |world| {
            let at = camera.to_screen(world);
            loom_ui_core::Point::new(canvas.bounds.x + at.x, canvas.bounds.y + at.y)
        };
        for module in app.session.scene.modules() {
            for (index, _) in module.outputs.iter().enumerate() {
                if let Some(badge) = app.session.scene.badge_bounds(module.id, index) {
                    let corner = screen(badge.position());
                    mask.push(Rect::new(corner, badge.size() * zoom));
                }
            }
            for_each_tile_text(module, |item| {
                let px = quantise(item.size * zoom);
                let shown = item.content.to_string();
                // Values and options change with the engine; their box is
                // masked whole.
                let room = item.width * zoom;
                let (old, new) = match item.kind {
                    TextKind::Value(_) => (room, room),
                    _ => (iced_width(&shown, px), text.measure(&shown, style(px), None).width),
                };
                if !matches!(item.kind, TextKind::Value(_)) && !shown.is_empty() {
                    labels.push(Label {
                        patch: name.clone(),
                        text: shown.clone(),
                        size: item.size,
                        old: iced_width(&shown, item.size),
                        new: text.measure(&shown, style(item.size), None).width,
                        room: item.width,
                    });
                }
                let anchor = screen(item.anchor);
                let height = px * 1.4;
                for width in [old.max(room.min(old.max(new))), new] {
                    let left = match item.align {
                        Align::Left => anchor.x,
                        Align::Center => anchor.x - width / 2.0,
                        Align::Right => anchor.x - width,
                    };
                    mask.push(Rect::new(
                        loom_ui_core::Point::new(left, anchor.y - height / 2.0),
                        loom_ui_core::Size::new(width, height),
                    ));
                }
            });
        }

        let Some(folder) = &before else { continue };
        let (width, height, old) = read_png(&folder.join(format!("{name}.png")));
        assert_eq!(
            (width, height),
            (physical.width, physical.height),
            "{name}: the image sizes"
        );
        let inside = |x: u32, y: u32| {
            let (x, y) = (x as f32 / SCALE, y as f32 / SCALE);
            let ink = INK / SCALE;
            mask.iter()
                .any(|r| x >= r.x - ink && x < r.x + r.width + ink && y >= r.y - ink && y < r.y + r.height + ink)
        };
        let (mut moved, mut outside) = (0usize, Vec::new());
        for y in 0..height {
            for x in 0..width {
                let at = ((y * width + x) * 4) as usize;
                if old[at..at + 4] != pixels[at..at + 4] {
                    if inside(x, y) {
                        moved += 1;
                    } else {
                        outside.push((x, y));
                    }
                }
            }
        }
        let canvas_px = Rect::new(
            loom_ui_core::Point::new(canvas.bounds.x * SCALE, canvas.bounds.y * SCALE),
            canvas.bounds.size() * SCALE,
        );
        let outside_canvas = outside
            .iter()
            .filter(|(x, y)| !canvas_px.contains(loom_ui_core::Point::new(*x as f32, *y as f32)))
            .count();
        println!(
            "{name}: {moved} pixels differ inside the text masks, {} outside ({} of them outside the canvas)",
            outside.len(),
            outside_canvas
        );
        if let (Some(first), Some(last)) = (outside.first(), outside.last()) {
            let xs = outside.iter().map(|p| p.0);
            let (x0, x1) = (xs.clone().min().unwrap(), xs.max().unwrap());
            println!("  outside, from {first:?} to {last:?}, x {x0}..={x1}");
            failed = true;
        }
    }

    println!("\nwidth change per label, new / old - 1, at the label's own size:");
    for (title, size) in [("titles", Some(11.0)), ("labels", Some(9.0)), ("all", None)] {
        let changes: Vec<f32> = labels
            .iter()
            .filter(|l| size.is_none_or(|s| l.size == s) && l.old > 0.0)
            .map(|l| (l.new / l.old - 1.0) * 100.0)
            .collect();
        if changes.is_empty() {
            continue;
        }
        let (least, greatest, median) = spread(changes.clone());
        println!(
            "  {title:<6} {:>4} labels: least {least:+.1}%, greatest {greatest:+.1}%, median {median:+.1}%",
            changes.len()
        );
    }
    println!("\nlabels whose width leaves their tile text box (old -> new, room), world units:");
    let mut any = false;
    for label in &labels {
        if label.new > label.room + 0.01 {
            any = true;
            println!(
                "  {}: {:?} at {} -> {:.2} -> {:.2}, room {:.2}{}",
                label.patch,
                label.text,
                label.size,
                label.old,
                label.new,
                label.room,
                if label.old > label.room + 0.01 {
                    " (already over)"
                } else {
                    ""
                }
            );
        }
    }
    if !any {
        println!("  none");
    }
    println!("\nimages written to {}", out.display());
    assert!(!failed, "pixels differ outside the moved text");
}
