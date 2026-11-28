//! Tests for the offscreen GPU target (`offscreen.rs`).

use super::physical;
use loom_ui_core::Size;

/// A logical size at a scale is whole physical pixels, rounded, and never
/// less than one on a side, so a window shrunk to nothing still gives a
/// texture wgpu can make.
#[test]
fn physical_pixels_round_and_keep_one_on_each_side() {
    assert_eq!(physical(Size::new(640.0, 400.0), 2.0), (1280, 800));
    assert_eq!(physical(Size::new(10.4, 10.6), 1.0), (10, 11));
    assert_eq!(physical(Size::new(0.0, 0.2), 1.0), (1, 1));
}
