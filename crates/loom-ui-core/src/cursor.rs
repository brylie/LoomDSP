//! The pointer's shape, by what it tells the user.

/// The pointer shape a part of the window asks for, by what it tells the
/// user.
///
/// It holds only the shapes the app shows today: the canvas's, and those
/// of the toolkit's widgets that the app uses. A shape is added when a view
/// needs it. The window shell turns each into what its window system calls
/// it, and nothing else names the window system's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Cursor {
    /// The plain arrow.
    #[default]
    Default,
    /// An open hand: this can be picked up and moved.
    Grab,
    /// A closed hand: something is being moved.
    Grabbing,
    /// A pointing hand: a press here does something.
    Pointer,
    /// A cross: a press here picks an exact place.
    Crosshair,
    /// A text beam: a press here places the caret in text.
    Text,
    /// A press here does nothing, because the part is turned off.
    NotAllowed,
    /// A two-way arrow, left and right: a drag sideways changes a value.
    ResizeHorizontal,
    /// A two-way arrow, up and down: a drag up or down changes a value.
    ResizeVertical,
    /// A slanted two-way arrow, from the top left to the bottom right: a
    /// drag resizes from a corner.
    ResizeDiagonal,
}

impl Cursor {
    /// Every shape, once each, in the order they are declared.
    pub const ALL: [Self; 10] = [
        Self::Default,
        Self::Grab,
        Self::Grabbing,
        Self::Pointer,
        Self::Crosshair,
        Self::Text,
        Self::NotAllowed,
        Self::ResizeHorizontal,
        Self::ResizeVertical,
        Self::ResizeDiagonal,
    ];
}

#[cfg(test)]
mod tests;
