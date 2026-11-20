//! Tests that draw painted boxes and glyphs on the GPU and read the pixels
//! back. On a machine with no GPU adapter or no system font they skip, as
//! the screenshot tests do.

use super::*;
use crate::paint::{Align, ListPainter, Painter, Quad, TextRun, TextStyle, Typesetter, Vertical, Weight};
use loom_ui_core::{Color, Point, Size};
use std::cell::RefCell;

/// One line of `text` at `at`, in 12 px white type.
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
        clip: Rect::new(Point::ORIGIN, Size::new(WIDTH as f32, HEIGHT as f32)),
    }
}

/// The typesetter, or `None` on a machine with no font.
fn text() -> Option<RefCell<Typesetter>> {
    let mut text = Typesetter::new();
    let probe = run("x", Point::ORIGIN);
    (text.measure("x", probe.style, None).width > 0.0).then(|| RefCell::new(text))
}

/// The pixels the GPU tests draw into: 256 wide, so a row is already
/// aligned for copying out.
const WIDTH: u32 = 256;
/// The height of the GPU tests' target.
const HEIGHT: u32 = 96;

/// Runs `future` on this thread until it is done, so the tests need no
/// executor: `pollster` comes only with the shell.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    /// Wakes the waiting thread.
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Unpark(std::thread::current())));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::park();
    }
}

/// A device and queue, or `None` on a machine with no GPU adapter, where
/// the GPU tests skip as the screenshot tests do.
fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// Draws `list` over black with a [`PaintRenderer`] alone, and returns the
/// RGBA pixels, row by row.
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
    let mut renderer = PaintRenderer::new(device, format);
    renderer.prepare(device, queue, &list, target);
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

/// On the GPU, a floating box painted in a later layer covers the text
/// under it, though the text is drawn by another pipeline.
#[test]
fn a_floating_box_covers_the_text_under_it_on_the_gpu() {
    let (Some((device, queue)), Some(text)) = (device(), text()) else {
        assert!(
            !gpu_required(),
            "LOOM_REQUIRE_GPU is set, but there is no GPU adapter or no font"
        );
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
        assert!(
            !gpu_required(),
            "LOOM_REQUIRE_GPU is set, but there is no GPU adapter or no font"
        );
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

/// True when `LOOM_REQUIRE_GPU` is `1`: then a missing adapter or font is a
/// failure, not a skip, as in the app's screenshot tests.
fn gpu_required() -> bool {
    std::env::var("LOOM_REQUIRE_GPU").is_ok_and(|value| value == "1")
}

/// The RGBA pixel at `x`, `y`.
fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = ((y * WIDTH + x) * 4) as usize;
    [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
}

/// A panel laid out and painted through the runtime's own walk, drawn on
/// the GPU: a rounded box with a fill and a border, holding a line of text
/// and a clipped row whose text runs past the clip (ui-layer.md, section 5,
/// rule 9).
#[test]
fn a_panel_draws_its_box_border_text_and_clip_on_the_gpu() {
    let (Some((device, queue)), Some(text)) = (device(), text()) else {
        assert!(
            !gpu_required(),
            "LOOM_REQUIRE_GPU is set, but there is no GPU adapter or no font"
        );
        eprintln!("skipped: no GPU adapter or no system font");
        return;
    };
    let style = TextStyle {
        size: 24.0,
        line_height: 1.2,
        weight: Weight::Regular,
    };
    let fill = Color::rgb(1.0, 0.0, 0.0);
    let border = Color::rgb(0.0, 0.0, 1.0);
    let root = crate::Element::column([
        crate::Element::text("Hi", style),
        crate::Element::row([crate::Element::text("MMMMMMMM", style)])
            .width(crate::Length::Fixed(40.0))
            .min_width(0.0)
            .clip(),
    ])
    .background(fill)
    .border(2.0, border)
    .radius(8.0)
    .padding(4.0)
    .align_x(crate::Align::Start);
    let mut layout = crate::Layout::new(loom_ui_core::UiId::panel("test"));
    layout.rebuild(root);
    layout.compute(Size::new(WIDTH as f32, HEIGHT as f32), &mut *text.borrow_mut());
    let box_size = layout.size();
    let whole = Rect::new(Point::ORIGIN, Size::new(WIDTH as f32, HEIGHT as f32));
    let origin = Point::new(10.0, 10.0);
    let mut list = PaintList::new(whole);
    layout.paint(
        origin,
        whole,
        &mut ListPainter::new(&text, &mut list),
        &crate::paint::PanelStyle {
            text: Color::WHITE,
            caret: Color::WHITE,
            selection: Color::BLACK,
            ring: crate::focus::RingStyle {
                color: Color::WHITE,
                width: 2.0,
                offset: 2.0,
            },
        },
        None,
    );
    let pixels = draw(&device, &queue, list);

    let (left, top) = (10, 10);
    let (right, bottom) = (10 + box_size.width as u32, 10 + box_size.height as u32);
    let middle = (top + bottom) / 2;
    assert_eq!(
        pixel(&pixels, left, middle),
        [0, 0, 255, 255],
        "the border, at the box's edge"
    );
    assert_eq!(
        pixel(&pixels, left + 1, middle),
        [0, 0, 255, 255],
        "the border is 2 wide"
    );
    assert_eq!(
        pixel(&pixels, right - 5, bottom - 5),
        [255, 0, 0, 255],
        "the fill, in the padding"
    );
    assert_eq!(pixel(&pixels, left, top), [0, 0, 0, 255], "the corner, past its curve");
    let blended = (0..8).any(|k| {
        let [r, g, b, _] = pixel(&pixels, left + k, top + k);
        (r, g, b) != (0, 0, 0) && (r, g, b) != (0, 0, 255) && (r, g, b) != (255, 0, 0)
    });
    assert!(blended, "a pixel on the corner's curve blends");
    // The first line: "Hi", inside the border and padding.
    let line = (style.size * style.line_height) as u32;
    assert!(
        has_text(&pixels, left + 6..right, top + 6..top + 6 + line),
        "the text is drawn"
    );
    // The second line is cut at the row's clip, 40 wide from the padding.
    let (row_top, clip_right) = (top + 6 + line, left + 6 + 40);
    assert!(
        has_text(&pixels, left + 6..clip_right, row_top..row_top + line),
        "the clipped text inside the clip is drawn"
    );
    assert!(
        !has_text(&pixels, clip_right..WIDTH, row_top..row_top + line),
        "nothing of it past the clip"
    );
}
