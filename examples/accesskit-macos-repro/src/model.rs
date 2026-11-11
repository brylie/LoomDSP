//! The sliders, the accessibility tree built from them, and what an
//! accessibility action does to them. No window and no platform code.

use accesskit::{
    Action, ActionData, ActionRequest, Node, NodeId, Orientation, Rect, Role, TreeId, TreeInfo, TreeUpdate,
};

/// The node ID of the window, the root of the tree.
pub const WINDOW: NodeId = NodeId(0);

/// How a slider tells assistive technology its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spoken {
    /// Only the number, as the node's numeric value.
    Number,
    /// The number as the numeric value, and the number with this unit as
    /// the node's string value: "800 Hz".
    NumberAndText(&'static str),
}

/// One slider in the window.
#[derive(Debug, Clone, PartialEq)]
pub struct Slider {
    /// The slider's name, given to AccessKit as the node's label.
    pub label: &'static str,
    /// The current value.
    pub value: f64,
    /// The lowest value.
    pub min: f64,
    /// The highest value.
    pub max: f64,
    /// How far one increment or decrement moves the value.
    pub step: f64,
    /// How the value is told to assistive technology.
    pub spoken: Spoken,
}

impl Slider {
    /// The value with its unit, when the slider has one.
    pub fn text(&self) -> Option<String> {
        match self.spoken {
            Spoken::Number => None,
            Spoken::NumberAndText(unit) => Some(format!("{} {unit}", self.value)),
        }
    }

    /// Sets the value, kept inside the slider's range.
    fn set(&mut self, value: f64) {
        self.value = value.clamp(self.min, self.max);
    }
}

/// The node ID of the slider at `index`.
pub fn slider_id(index: usize) -> NodeId {
    NodeId(index as u64 + 1)
}

/// The index of the slider with this node ID, if it is one.
fn slider_index(sliders: &[Slider], id: NodeId) -> Option<usize> {
    let index = (id.0 as usize).checked_sub(1)?;
    (index < sliders.len()).then_some(index)
}

/// Where the slider at `index` is in the window: a row 40 logical pixels
/// high, one under the other. AccessKit takes bounds in physical pixels,
/// so they are multiplied by `scale`, the window's scale factor.
pub fn slider_bounds(index: usize, scale: f64) -> Rect {
    let top = 20.0 + 60.0 * index as f64;
    Rect {
        x0: 20.0 * scale,
        y0: top * scale,
        x1: 380.0 * scale,
        y1: (top + 40.0) * scale,
    }
}

/// The accessibility node of one slider.
fn slider_node(slider: &Slider, index: usize, scale: f64) -> Node {
    let mut node = Node::new(Role::Slider);
    node.set_label(slider.label);
    node.set_numeric_value(slider.value);
    node.set_min_numeric_value(slider.min);
    node.set_max_numeric_value(slider.max);
    node.set_numeric_value_step(slider.step);
    if let Some(text) = slider.text() {
        node.set_value(text);
    }
    node.set_orientation(Orientation::Horizontal);
    node.set_bounds(slider_bounds(index, scale));
    node.add_action(Action::Focus);
    node.add_action(Action::Increment);
    node.add_action(Action::Decrement);
    node.add_action(Action::SetValue);
    node
}

/// The whole tree: the window, and one slider node for each slider.
/// `focus` is the index of the slider with keyboard focus, and `scale` is
/// the window's scale factor.
pub fn tree(title: &str, sliders: &[Slider], focus: usize, scale: f64) -> TreeUpdate {
    let mut window = Node::new(Role::Window);
    window.set_label(title);
    window.set_children((0..sliders.len()).map(slider_id).collect::<Vec<_>>());
    let mut nodes = vec![(WINDOW, window)];
    for (index, slider) in sliders.iter().enumerate() {
        nodes.push((slider_id(index), slider_node(slider, index, scale)));
    }
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(WINDOW)),
        tree_id: TreeId::ROOT,
        focus: slider_id(focus.min(sliders.len().saturating_sub(1))),
    }
}

/// The number at the start of `text`, so "810 Hz" gives 810.
fn leading_number(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

/// Applies an accessibility action. Returns true when a slider or the
/// focus changed, so the caller sends a new tree.
pub fn apply(sliders: &mut [Slider], focus: &mut usize, request: &ActionRequest) -> bool {
    let Some(index) = slider_index(sliders, request.target_node) else {
        return false;
    };
    let slider = &mut sliders[index];
    let before = (slider.value, *focus);
    match (request.action, &request.data) {
        (Action::Focus, _) => *focus = index,
        (Action::Increment, _) => slider.set(slider.value + slider.step),
        (Action::Decrement, _) => slider.set(slider.value - slider.step),
        (Action::SetValue, Some(ActionData::NumericValue(value))) => slider.set(*value),
        (Action::SetValue, Some(ActionData::Value(text))) => {
            if let Some(value) = leading_number(text) {
                slider.set(value);
            }
        }
        _ => {}
    }
    before != (sliders[index].value, *focus)
}

/// Moves the focused slider by `steps` of its own step, for the arrow keys.
pub fn nudge(sliders: &mut [Slider], focus: usize, steps: f64) {
    if let Some(slider) = sliders.get_mut(focus) {
        slider.set(slider.value + steps * slider.step);
    }
}

#[cfg(test)]
mod tests;
