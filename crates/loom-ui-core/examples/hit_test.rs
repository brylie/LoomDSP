//! Finds which button of a small toolbar is under the pointer.
//!
//! This uses only `loom-ui-core`: a row of rectangles, a point, and a colour
//! for the button that is hit. Run it with:
//!
//! ```sh
//! cargo run -p loom-ui-core --example hit_test
//! ```

use loom_ui_core::{Color, Point, Rect, Size, Vector};

/// The width and height of one button.
const BUTTON: Size = Size::new(80.0, 24.0);
/// The colour of a button the pointer is not over.
const IDLE: Color = Color::rgb8(60, 60, 70);
/// The colour of the button under the pointer.
const HOVERED: Color = Color::rgb8(90, 140, 220);

/// Lays out three buttons side by side and reports which one each pointer
/// position hits.
fn main() {
    let names = ["Open", "Save", "Undo"];
    let first = Rect::new(Point::new(10.0, 10.0), BUTTON);
    let step = Vector::new(BUTTON.width, 0.0);

    let buttons: Vec<Rect> = (0..names.len())
        .scan(first.position(), |corner, _| {
            let button = Rect::new(*corner, BUTTON);
            *corner += step;
            Some(button)
        })
        .collect();

    let toolbar = buttons.iter().fold(first, |all, button| all.union(button));
    println!("toolbar covers {toolbar:?}");

    // The second pointer is exactly on the edge "Open" and "Save" share.
    // The half-open rule gives it to "Save" alone.
    let pointers = [Point::new(30.0, 20.0), Point::new(90.0, 20.0), Point::new(400.0, 20.0)];
    for pointer in pointers {
        println!("pointer at ({}, {}):", pointer.x, pointer.y);
        for (name, button) in names.iter().zip(&buttons) {
            let colour = if button.contains(pointer) { HOVERED } else { IDLE };
            let mark = if colour == HOVERED { "hit" } else { "   " };
            println!("  {mark} {name:<5} drawn with {colour:?}");
        }
    }

    // A focus ring is the button grown by two units on every side.
    let ring = buttons[1].expand(2.0);
    println!("focus ring around Save: {ring:?}");
    println!("ring overlaps Open: {}", ring.intersects(&buttons[0]));
}
