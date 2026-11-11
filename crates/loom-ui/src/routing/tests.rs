use super::*;

/// `NoTarget` routes nothing, reports no focus, and never asks to draw.
#[test]
fn no_target_routes_nothing() {
    let mut target = NoTarget;
    let mut out: Vec<u32> = Vec::new();
    Target::<u32>::input(
        &mut target,
        &Event::WindowUnfocused,
        None,
        std::time::Instant::now(),
        &mut out,
    );
    assert!(out.is_empty());
    assert_eq!(Target::<u32>::focused(&target), None);
    assert!(!Target::<u32>::needs_redraw(&mut target, 3));
}

/// Section 2, rule 3: a focus effect with no elements panics, naming it.
#[test]
#[should_panic(expected = "no target: there are no elements to focus")]
fn no_target_panics_on_focus() {
    Target::<u32>::focus(&mut NoTarget, &FocusTarget::Next, Selection::Keep);
}

/// Section 2, rule 3: a scroll effect with no elements panics, naming it.
#[test]
#[should_panic(expected = "no target: there are no elements to scroll into view")]
fn no_target_panics_on_scroll() {
    Target::<u32>::scroll_into_view(&mut NoTarget, &UiId::panel("list"));
}
