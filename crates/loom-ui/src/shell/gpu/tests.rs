use super::*;

/// The format is the first one that is not sRGB, as Iced chooses with its
/// `web-colors` feature, so the colours stay what they are today.
#[test]
fn the_format_is_the_first_that_is_not_srgb() {
    use wgpu::TextureFormat as F;
    assert_eq!(
        pick_format(&[F::Bgra8UnormSrgb, F::Bgra8Unorm, F::Rgba8Unorm]),
        Some(F::Bgra8Unorm)
    );
    assert_eq!(
        pick_format(&[F::Bgra8UnormSrgb]),
        Some(F::Bgra8UnormSrgb),
        "else the first"
    );
    assert_eq!(pick_format(&[]), None);
}

/// Post-multiplied alpha first, then pre-multiplied, then the system's.
#[test]
fn the_alpha_mode_prefers_post_multiplied() {
    use wgpu::CompositeAlphaMode as A;
    assert_eq!(
        pick_alpha(&[A::Opaque, A::PreMultiplied, A::PostMultiplied]),
        A::PostMultiplied
    );
    assert_eq!(pick_alpha(&[A::Opaque, A::PreMultiplied]), A::PreMultiplied);
    assert_eq!(pick_alpha(&[A::Opaque]), A::Auto);
}

/// A device with no window, for a capture in a test. `None` when the
/// machine has no adapter.
fn headless(format: wgpu::TextureFormat) -> Option<Device> {
    let adapter = pollster::block_on(instance().request_adapter(&adapter_options(None))).ok()?;
    let (device, queue) = request_device(&adapter).ok()?;
    Some(Device { device, queue, format })
}

/// The capture reads four 8-bit channels in either order, and refuses
/// wider or packed formats rather than read their bytes as colours.
#[test]
fn the_capture_reads_only_eight_bit_formats() {
    use wgpu::TextureFormat as F;
    assert_eq!(eight_bit_order(F::Bgra8Unorm), Some(true));
    assert_eq!(eight_bit_order(F::Bgra8UnormSrgb), Some(true));
    assert_eq!(eight_bit_order(F::Rgba8Unorm), Some(false));
    assert_eq!(eight_bit_order(F::Rgba8UnormSrgb), Some(false));
    assert_eq!(eight_bit_order(F::Rgba16Float), None);
    assert_eq!(eight_bit_order(F::Rgb10a2Unorm), None);
}

/// A capture in a format it cannot read gives no image, and draws nothing.
/// Skipped, with `NO_GPU`, on a machine with no adapter.
#[test]
fn a_capture_in_a_wide_format_gives_none() {
    let Some(device) = headless(wgpu::TextureFormat::Rgba16Float) else {
        eprintln!("NO_GPU: no adapter, the capture is not checked");
        return;
    };
    let mut drawn = false;
    let image = capture(&device, PhysicalSize::new(2, 2), 1.0, |_| drawn = true);
    assert!(image.is_none());
    assert!(!drawn);
}

/// Clears `frame` to `colour`, as a layer would.
fn clear(frame: &Frame<'_>, colour: wgpu::Color) {
    let mut encoder = frame
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: frame.view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(colour),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    }));
    frame.queue.submit([encoder.finish()]);
}

/// A capture draws through the layers into a texture of the size asked,
/// and gives the pixels in RGBA order, from a BGRA format too. Skipped,
/// with `NO_GPU`, on a machine with no adapter.
#[test]
fn a_capture_gives_what_the_layers_drew_in_rgba_order() {
    for format in [wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Rgba8Unorm] {
        let Some(device) = headless(format) else {
            eprintln!("NO_GPU: no adapter, the capture is not checked");
            return;
        };
        let mut seen = None;
        let image = capture(&device, PhysicalSize::new(6, 4), 2.0, |frame| {
            seen = Some((frame.size, frame.scale, frame.format));
            clear(
                frame,
                wgpu::Color {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            );
        })
        .expect("the read-back gives an image");
        assert_eq!(seen, Some((Size::new(3.0, 2.0), 2.0, format)), "{format:?}");
        assert_eq!((image.width(), image.height()), (6, 4));
        assert!(
            image
                .rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [255, 0, 0, 255]),
            "red in RGBA order from {format:?}"
        );
    }
}
