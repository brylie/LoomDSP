//! Tests for the plan of painted text, built on the CPU with no GPU.

use super::*;
use crate::painter::{Painter, Quad, TextRun, TextStyle, Vertical, Weight};
use crate::text::{CanvasText, ListPainter};
use crate::tile_text::Align;
use loom_ui_core::{Color, Size};
use std::cell::RefCell;

/// The canvas's place: not at the window's corner, so a missing offset
/// shows.
const BOUNDS: Rect = Rect {
    x: 100.0,
    y: 50.0,
    width: 400.0,
    height: 300.0,
};

/// A target for the canvas at `BOUNDS` in a 1000 by 800 window at `scale`.
fn target(scale: f32) -> Target {
    Target {
        size: BOUNDS.size(),
        scale,
        origin: BOUNDS.position(),
        physical: ((1000.0 * scale) as u32, (800.0 * scale) as u32),
    }
}

/// One line of `text` at `at`.
fn run(text: &str, at: Point) -> TextRun<'_> {
    TextRun {
        text,
        anchor: at,
        bounds: Size::new(200.0, 20.0),
        align: Align::Left,
        vertical: Vertical::Top,
        style: TextStyle {
            size: 12.0,
            line_height: 1.2,
            weight: Weight::Regular,
        },
        wrap: false,
        colour: Color::WHITE,
        clip: BOUNDS,
    }
}

/// A box at `at`.
fn quad(at: Point) -> Quad {
    Quad::filled(Rect::new(at, Size::new(20.0, 10.0)), Color::BLACK, 2.0)
}

/// Canvas text, or `None` on a machine with no font.
fn text() -> Option<RefCell<CanvasText>> {
    let mut text = CanvasText::new();
    let probe = run("x", Point::ORIGIN);
    (text.measure("x", probe.style, None).width > 0.0).then(|| RefCell::new(text))
}

/// A clip becomes a scissor in physical pixels, cut to the target, and a
/// clip outside the target has none.
#[test]
fn a_clip_becomes_a_scissor_in_the_target() {
    assert_eq!(scissor(BOUNDS, target(1.0)), Some([100, 50, 400, 300]));
    assert_eq!(scissor(BOUNDS, target(2.0)), Some([200, 100, 800, 600]));
    let half = Rect::new(Point::new(100.25, 50.0), Size::new(10.5, 10.0));
    assert_eq!(
        scissor(half, target(1.0)),
        Some([100, 50, 11, 10]),
        "outward to whole pixels"
    );
    let past = Rect::new(Point::new(900.0, 700.0), Size::new(500.0, 500.0));
    assert_eq!(scissor(past, target(1.0)), Some([900, 700, 100, 100]));
    let away = Rect::new(Point::new(2000.0, 0.0), Size::new(5.0, 5.0));
    assert_eq!(scissor(away, target(1.0)), None);
}

/// The plan draws layer by layer and batch by batch in the order painted,
/// with each layer's scissor, and boxes placed from the canvas's corner.
#[test]
fn the_plan_keeps_the_painted_order() {
    let Some(text) = text() else { return };
    let mut list = PaintList::new(BOUNDS);
    let popup = Rect::new(Point::new(150.0, 100.0), Size::new(100.0, 50.0));
    let mut painter = ListPainter::new(&text, &mut list);
    painter.text(&run("Cutoff", Point::new(110.0, 60.0)));
    painter.begin_layer(popup);
    painter.quad(quad(Point::new(150.0, 100.0)));
    painter.text(&run("50", Point::new(155.0, 105.0)));
    painter.quad(quad(Point::new(160.0, 100.0)));
    painter.end_layer();

    let mut atlas = Atlas::new(super::super::atlas::MAX_SIZE);
    let plan = TextPlan::build(&list, &mut atlas, target(1.0));
    let kinds: Vec<(&str, [u32; 4])> = plan
        .steps
        .iter()
        .map(|step| match step.draw {
            Draw::Boxes(_) => ("boxes", step.scissor),
            Draw::Glyphs(_) => ("glyphs", step.scissor),
        })
        .collect();
    let canvas = [100, 50, 400, 300];
    let layer = [150, 100, 100, 50];
    assert_eq!(
        kinds,
        [
            ("glyphs", canvas),
            ("boxes", layer),
            ("glyphs", layer),
            ("boxes", layer)
        ],
        "tile text, then the layer's box, text, and caret over it"
    );
    let first_box = plan.boxes[0];
    assert_eq!(first_box.rect(), [50.0, 50.0, 20.0, 10.0]);
    assert_eq!(plan.glyphs.len(), 8, "six letters and two digits");
}

/// A layer wholly outside the target draws nothing, and an empty batch
/// is no step.
#[test]
fn a_layer_outside_the_target_draws_nothing() {
    let text = RefCell::new(CanvasText::new());
    let mut list = PaintList::new(BOUNDS);
    let away = Rect::new(Point::new(5000.0, 0.0), Size::new(10.0, 10.0));
    let mut painter = ListPainter::new(&text, &mut list);
    painter.begin_layer(away);
    painter.quad(quad(Point::new(5000.0, 0.0)));
    painter.end_layer();
    painter.text(&run(" ", Point::new(110.0, 60.0)));
    let mut atlas = Atlas::new(super::super::atlas::MAX_SIZE);
    let plan = TextPlan::build(&list, &mut atlas, target(1.0));
    assert!(plan.steps.is_empty(), "{:?}", plan.steps);
}

/// At its largest size, an atlas too small for the frame leaves glyphs
/// out and counts them, never a panic. A smaller atlas grows first.
#[test]
fn a_full_atlas_skips_glyphs_and_counts_them() {
    let Some(text) = text() else { return };
    let mut list = PaintList::new(BOUNDS);
    let line = "The quick brown fox jumps over the lazy dog";
    ListPainter::new(&text, &mut list).text(&run(line, Point::new(110.0, 60.0)));

    let before = crate::stats::RenderCounts::this_thread().glyph_skips;
    let mut tiny = Atlas::sized(16, 16);
    let plan = TextPlan::build(&list, &mut tiny, target(2.0));
    assert!(tiny.skipped > 0);
    assert!(plan.glyphs.len() < list.glyphs.len());
    let after = crate::stats::RenderCounts::this_thread().glyph_skips;
    assert_eq!(after - before, tiny.skipped);

    let mut growing = Atlas::sized(16, 1024);
    let plan = TextPlan::build(&list, &mut growing, target(2.0));
    assert_eq!(growing.skipped, 0);
    assert!(growing.size() > 16, "it grew");
    let inked = list.glyphs.len() - line.matches(' ').count();
    assert_eq!(plan.glyphs.len(), inked, "every glyph with ink is placed");
}

/// The pixels the GPU tests draw into: 256 wide, so a row is already
/// aligned for copying out.
const WIDTH: u32 = 256;
/// The height of the GPU tests' target.
const HEIGHT: u32 = 96;

/// A device and queue, or `None` on a machine with no GPU adapter, where
/// the GPU tests skip as the screenshot tests do.
fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        iced::futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    iced::futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// Draws an empty scene with `list` painted over it, and returns the RGBA
/// pixels, row by row.
fn draw(device: &wgpu::Device, queue: &wgpu::Queue, list: PaintList) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let size = wgpu::Extent3d {
        width: WIDTH,
        height: HEIGHT,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gpu test target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let target = Target {
        size: Size::new(WIDTH as f32, HEIGHT as f32),
        scale: 1.0,
        origin: Point::ORIGIN,
        physical: (WIDTH, HEIGHT),
    };
    let frame = super::super::CanvasFrame {
        geometry: Default::default(),
        controls: None,
        traces: None,
        overlay: Default::default(),
        bars: 0..0,
        value_bars: 0..0,
        camera: crate::Camera::default(),
        cable_dim: 1.0,
        paint: list,
    };
    let mut renderer = super::super::CanvasRenderer::new(device, format);
    renderer.prepare(device, queue, &frame, target);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("gpu test pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        renderer.draw(&mut pass);
    }
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu test read-back"),
        size: u64::from(WIDTH * HEIGHT * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.expect("the read-back maps"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the GPU finishes");
    buffer.slice(..).get_mapped_range().to_vec()
}

/// True when some pixel in `x` by `y` is near white: drawn text.
fn has_text(pixels: &[u8], x: std::ops::Range<u32>, y: std::ops::Range<u32>) -> bool {
    y.flat_map(|row| x.clone().map(move |column| ((row * WIDTH + column) * 4) as usize))
        .any(|at| pixels[at] > 200 && pixels[at + 1] > 200 && pixels[at + 2] > 200)
}

/// Large white text with its top-left at `at`.
fn big(text: &str, at: Point) -> TextRun<'_> {
    TextRun {
        style: TextStyle {
            size: 24.0,
            line_height: 1.2,
            weight: Weight::Regular,
        },
        bounds: Size::new(300.0, 40.0),
        ..run(text, at)
    }
}

/// On the GPU, a floating box painted in a later layer covers the tile
/// text under it, though the text is drawn by another pipeline.
#[test]
fn a_floating_box_covers_the_text_under_it_on_the_gpu() {
    let (Some((device, queue)), Some(text)) = (device(), text()) else {
        eprintln!("skipped: no GPU adapter or no system font");
        return;
    };
    let whole = Rect::new(Point::ORIGIN, Size::new(WIDTH as f32, HEIGHT as f32));
    let paint = |covered: bool| {
        let mut list = PaintList::new(whole);
        let mut painter = ListPainter::new(&text, &mut list);
        painter.text(&TextRun {
            clip: whole,
            ..big("MMMMMM", Point::new(20.0, 20.0))
        });
        if covered {
            painter.begin_layer(whole);
            painter.quad(Quad::filled(
                Rect::new(Point::new(10.0, 10.0), Size::new(200.0, 60.0)),
                Color::rgb(1.0, 0.0, 0.0),
                0.0,
            ));
            painter.end_layer();
        }
        list
    };
    let open = draw(&device, &queue, paint(false));
    assert!(has_text(&open, 12..208, 12..68), "the text is drawn");
    let covered = draw(&device, &queue, paint(true));
    for row in 12..68 {
        for column in 12..208 {
            let at = ((row * WIDTH + column) * 4) as usize;
            assert_eq!(covered[at..at + 4], [255, 0, 0, 255], "at {column}, {row}");
        }
    }
}

/// On the GPU, a layer's clip cuts its text and its boxes: nothing of
/// them is drawn past the clip, though unclipped they reach further. Boxes
/// are cut only by the scissor, so the box proves the scissor itself.
#[test]
fn a_layer_clip_cuts_its_text_on_the_gpu() {
    let (Some((device, queue)), Some(text)) = (device(), text()) else {
        eprintln!("skipped: no GPU adapter or no system font");
        return;
    };
    let whole = Rect::new(Point::ORIGIN, Size::new(WIDTH as f32, HEIGHT as f32));
    let paint = |clip: Rect| {
        let mut list = PaintList::new(whole);
        let mut painter = ListPainter::new(&text, &mut list);
        painter.begin_layer(clip);
        painter.quad(Quad::filled(
            Rect::new(Point::new(0.0, 70.0), Size::new(WIDTH as f32, 20.0)),
            Color::rgb(1.0, 0.0, 0.0),
            0.0,
        ));
        painter.text(&TextRun {
            clip: whole,
            ..big("MMMMMMMM", Point::new(20.0, 20.0))
        });
        painter.end_layer();
        list
    };
    let red = |pixels: &[u8], x: u32, y: u32| {
        let at = ((y * WIDTH + x) * 4) as usize;
        pixels[at..at + 4] == [255, 0, 0, 255]
    };
    let wide = draw(&device, &queue, paint(whole));
    assert!(red(&wide, 100, 80), "unclipped, the box is drawn");
    assert!(
        has_text(&wide, 70..WIDTH, 0..HEIGHT),
        "unclipped, the text reaches past x 70"
    );
    let cut = draw(
        &device,
        &queue,
        paint(Rect::new(Point::new(20.0, 20.0), Size::new(40.0, 60.0))),
    );
    assert!(red(&cut, 30, 75), "the box inside the clip is drawn");
    assert!(!red(&cut, 100, 75), "the scissor cuts the box past the clip");
    assert!(has_text(&cut, 20..60, 20..60), "the text inside the clip is drawn");
    assert!(
        !has_text(&cut, 60..WIDTH, 0..HEIGHT),
        "nothing past the clip's right edge"
    );
    assert!(!has_text(&cut, 0..WIDTH, 80..HEIGHT), "nothing below the clip");
}
