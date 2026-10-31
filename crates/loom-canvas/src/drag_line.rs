//! A pointer-transparent window layer for a cable dragged to the inspector.

use crate::gpu::{Cable, DragPrimitive};
use iced::widget::shader::{self, Program};
use iced::{Color, Length, Point, Rectangle, mouse};

/// Draws one focused cable in window coordinates with the canvas shader.
/// Its GPU buffers are separate from the patch canvas buffers.
pub struct DragLine {
    start: Point,
    at: Point,
    colour: Color,
    zoom: f32,
}

impl DragLine {
    /// Creates the line from the output to the current pointer position.
    pub fn new<Message>(start: Point, at: Point, colour: Color, zoom: f32) -> shader::Shader<Message, Self> {
        shader::Shader::new(Self {
            start,
            at,
            colour,
            zoom,
        })
        .width(Length::Fill)
        .height(Length::Fill)
    }
}

impl<Message> Program<Message> for DragLine {
    type State = ();
    type Primitive = DragPrimitive;

    fn draw(&self, _state: &Self::State, _cursor: mouse::Cursor, bounds: Rectangle) -> Self::Primitive {
        let relative = |point: Point| Point::new((point.x - bounds.x) / self.zoom, (point.y - bounds.y) / self.zoom);
        DragPrimitive {
            cable: Cable::new(relative(self.start), relative(self.at), self.colour, self.colour, false).focused(),
            zoom: self.zoom,
        }
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        mouse::Interaction::None
    }
}
