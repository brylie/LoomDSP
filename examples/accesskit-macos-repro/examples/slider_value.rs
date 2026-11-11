//! Gap 2: a slider can be stepped or say its unit, not both.
//!
//! Two sliders with the same range. "Number only" has a numeric value. "With
//! unit" has the same numeric value and the string value "800 Hz". The
//! macOS adapter reports the string when a node has one, and the number
//! otherwise, and has no spoken value description beside a number. Run
//! with VoiceOver on, move to each slider, and try to step it.
//!
//! ```sh
//! cargo run --example slider_value
//! ```

use accesskit_macos_repro::model::{Slider, Spoken};
use accesskit_macos_repro::window;

fn main() -> Result<(), winit::error::EventLoopError> {
    let slider = |label, spoken| Slider {
        label,
        value: 800.0,
        min: 20.0,
        max: 20_000.0,
        step: 10.0,
        spoken,
    };
    window::run(
        "Slider value",
        vec![
            slider("Number only", Spoken::Number),
            slider("With unit", Spoken::NumberAndText("Hz")),
        ],
    )
}
