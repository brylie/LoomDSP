//! Gap 1: a slider's name is not read.
//!
//! One slider named "Cutoff" with a numeric value. AccessKit's macOS
//! adapter gives the name to macOS as the accessibility title and never as
//! the accessibility label. Run with VoiceOver on and move to the slider.
//!
//! ```sh
//! cargo run --example slider_name
//! ```

use accesskit_macos_repro::model::{Slider, Spoken};
use accesskit_macos_repro::window;

fn main() -> Result<(), winit::error::EventLoopError> {
    window::run(
        "Slider name",
        vec![Slider {
            label: "Cutoff",
            value: 50.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            spoken: Spoken::Number,
        }],
    )
}
