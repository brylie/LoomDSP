//! Drawing the app's views offscreen, with wgpu: for `--screenshot-window`
//! and for the headless harness's captures.
//!
//! The app's renderer type is Iced's fallback over wgpu and
//! [`Measure`](super::Measure). Only the wgpu half draws, so an offscreen
//! renderer is always that half, and taking its pixels asks it directly.

use iced::advanced::renderer::Headless;
use iced::{Color, Font, Pixels, Size};

/// A wgpu renderer that draws into an offscreen texture, with the window's
/// defaults (Iced's default font at 16), or `None` when no GPU adapter is
/// found.
pub(crate) fn renderer() -> Option<crate::Renderer> {
    iced::futures::executor::block_on(<iced_wgpu::Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))
    .map(crate::Renderer::Primary)
}

/// The RGBA pixels of what `renderer` drew, `size` physical pixels at
/// `scale`, on `background`.
///
/// # Panics
///
/// If `renderer` is the measure renderer, which draws nothing: only
/// [`renderer`] makes renderers to draw offscreen with.
pub(crate) fn pixels(renderer: &mut crate::Renderer, size: Size<u32>, scale: f32, background: Color) -> Vec<u8> {
    match renderer {
        crate::Renderer::Primary(wgpu) => Headless::screenshot(wgpu, size, scale, background),
        crate::Renderer::Secondary(_) => unreachable!("only a wgpu renderer draws offscreen"),
    }
}
