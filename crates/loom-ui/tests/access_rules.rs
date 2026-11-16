//! Section 7, rule 2 of docs/design/ui-layer.md, checked in every build:
//! every element the crate's examples give a screen reader has a role that
//! can be operated, a non-empty name, and the value its role needs. A debug
//! build also panics at the first one that breaks it, when it is laid out;
//! this test checks the same in a release build.

#[allow(dead_code, reason = "the example's own program is not run here")]
#[path = "../examples/focus.rs"]
mod focus;

use loom_ui::{Element, Layout};
use loom_ui_core::UiId;

/// Every panel the focus example builds, with each model it can show.
fn panels() -> Vec<(UiId, Element)> {
    let mut panels = Vec::new();
    for volume in [0, 70, 100] {
        panels.push((UiId::panel("toolbar"), focus::toolbar(&volume)));
    }
    for muted in [false, true] {
        panels.push((UiId::panel("status"), focus::status_bar(&muted)));
    }
    panels
}

/// No element of an example breaks rule 2.
#[test]
fn every_accessed_element_of_the_examples_keeps_rule_2() {
    for (key, element) in panels() {
        let mut layout = Layout::new(key.clone());
        layout.rebuild(element);
        assert_eq!(layout.access_problems(), [], "panel {key}");
    }
}
