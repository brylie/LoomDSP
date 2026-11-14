use super::*;

/// A view with the given panels, all in the panels layer.
fn view(panels: &[(&str, u32)]) -> View {
    let mut view = View::new();
    for (key, model) in panels {
        view.panel(UiId::panel(*key), Layer::Panels, model);
    }
    view
}

/// Section 3, rule 2: an equal model is compared and not changed; a
/// different one is changed.
#[test]
fn an_equal_model_is_not_a_change_and_a_different_one_is() {
    let last = view(&[("toolbar", 1), ("inspector", 2)]);
    assert_eq!(view(&[("toolbar", 1), ("inspector", 2)]).changed_since(&last), 0);
    assert_eq!(view(&[("toolbar", 1), ("inspector", 3)]).changed_since(&last), 1);
}

/// A panel added, removed, or moved to another layer is a change, and the
/// order panels are added in is not.
#[test]
fn adding_removing_or_moving_a_panel_is_a_change() {
    let last = view(&[("toolbar", 1), ("inspector", 2)]);
    assert_eq!(view(&[("inspector", 2), ("toolbar", 1)]).changed_since(&last), 0);
    assert_eq!(view(&[("toolbar", 1)]).changed_since(&last), 1);
    assert_eq!(
        view(&[("toolbar", 1), ("inspector", 2), ("browser", 0)]).changed_since(&last),
        1
    );

    let mut moved = View::new();
    moved.panel(UiId::panel("toolbar"), Layer::Overlays, &1u32);
    moved.panel(UiId::panel("inspector"), Layer::Panels, &2u32);
    assert_eq!(moved.changed_since(&last), 1);
}

/// A model of another type under the same key is a change.
#[test]
fn a_model_of_another_type_is_a_change() {
    let last = view(&[("toolbar", 1)]);
    let mut other = View::new();
    other.panel(UiId::panel("toolbar"), Layer::Panels, &"1");
    assert_eq!(other.changed_since(&last), 1);
}

/// A view lists its panels in order and reports its size.
#[test]
fn a_view_lists_its_panels() {
    let built = view(&[("toolbar", 1), ("inspector", 2)]);
    let keys: Vec<_> = built.panels().map(|(key, layer)| (key.to_string(), layer)).collect();
    assert_eq!(
        keys,
        [
            ("toolbar".to_string(), Layer::Panels),
            ("inspector".to_string(), Layer::Panels)
        ]
    );
    assert_eq!(built.len(), 2);
    assert!(!built.is_empty());
    assert!(View::new().is_empty());
    assert!(format!("{built:?}").contains("inspector"));
}

/// Two panels with one key in one view is a mistake, caught in debug
/// builds.
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "panel key toolbar is used twice in one view")]
fn a_key_used_twice_panics() {
    view(&[("toolbar", 1), ("toolbar", 2)]);
}

/// A panel with a builder is built when it is new or its model or layer
/// changed, and not when it is the same; a panel with no builder is never
/// built.
#[test]
fn a_panel_is_built_only_when_it_changed() {
    /// One space per unit of the model.
    fn build(count: &u32) -> crate::Element {
        crate::Element::row((0..*count).map(|_| crate::Element::space(crate::Length::Shrink, crate::Length::Shrink)))
    }
    let view = |count: u32, layer: Layer| {
        let mut view = View::new();
        view.panel_with(UiId::panel("list"), layer, &count, build);
        view.panel(UiId::panel("drawn elsewhere"), Layer::Panels, &count);
        view
    };
    let built = |now: &View, last: Option<&View>| -> Vec<bool> {
        now.built(last)
            .into_iter()
            .map(|(_, _, element)| element.is_some())
            .collect()
    };
    let first = view(2, Layer::Panels);
    assert_eq!(built(&first, None), [true]);
    assert_eq!(built(&view(2, Layer::Panels), Some(&first)), [false]);
    assert_eq!(built(&view(3, Layer::Panels), Some(&first)), [true]);
    assert_eq!(built(&view(2, Layer::Overlays), Some(&first)), [true]);
    let element = first.built(None).remove(0).2.expect("built");
    assert_eq!(element.children.len(), 2);
}
