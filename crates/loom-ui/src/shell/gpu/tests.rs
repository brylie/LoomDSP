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
