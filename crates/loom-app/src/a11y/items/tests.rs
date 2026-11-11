use super::*;
use accesskit::{Action, ActionData};

/// A number a screen reader sets becomes the message the panel's own
/// control sends, in that control's terms and range.
#[test]
fn a_number_is_set_in_the_controls_own_terms() {
    let depth = SetNumber::Depth(loom_core::AssignmentId(3));
    assert!(matches!(
        depth.messages(50.0).as_slice(),
        [Message::Modulation(crate::modulation::Message::Depth { depth, .. })] if (*depth - 0.5).abs() < 1e-6
    ));
    assert!(matches!(
        depth.messages(-400.0).as_slice(),
        [Message::Modulation(crate::modulation::Message::Depth { depth, .. })] if *depth == -1.0
    ));
    let spread = SetNumber::Spread {
        tile: 2,
        name: "cutoff".into(),
    };
    assert!(matches!(
        spread.messages(25.0).as_slice(),
        [Message::Mode(crate::modes::Message::SetSpread { amount, .. })] if (*amount - 0.25).abs() < 1e-6
    ));
    assert!(depth.messages(f64::NAN).is_empty(), "not a number sets nothing");
}

/// Text a screen reader sets is typed and then entered, as a person would.
#[test]
fn text_is_typed_then_entered() {
    use crate::inspector::Message as Inspector;
    let value = SetText::Value { module: 4, index: 0 }.messages("800");
    assert!(matches!(
        value.as_slice(),
        [
            Message::Inspector(Inspector::ValueText { text, .. }),
            Message::Inspector(Inspector::ValueSubmit { .. })
        ] if text == "800"
    ));
    let low = SetText::MidiLow {
        module: 4,
        index: 0,
        high: "1".into(),
    }
    .messages("0.2");
    assert!(matches!(
        low.as_slice(),
        [Message::Learn(crate::learn::Message::MidiRangeText { low, high, .. }), Message::Learn(_)]
            if low == "0.2" && high == "1"
    ));
    for set in [
        SetText::MidiHigh {
            module: 4,
            index: 0,
            low: "0".into(),
        },
        SetText::SubPatchName(2),
        SetText::UnknownLabel(9),
        SetText::Tag,
    ] {
        assert_eq!(set.messages("x").len(), 2, "{set:?}: typed, then entered");
    }
}

/// A part does what its item says for each action, and nothing while it
/// is disabled. A part is found at any depth.
#[test]
fn actions_follow_the_item() {
    let id = UiId::panel("inspector").with(4_u32);
    let mut button = Item::button(id.clone().with("help"), "Help", Some(Message::ShowModuleHelp(4)));
    assert_eq!(messages(&button, Action::Click, None).len(), 1);
    assert!(messages(&button, Action::Increment, None).is_empty());
    button.enabled = false;
    assert!(
        messages(&button, Action::Click, None).is_empty(),
        "a disabled part does nothing"
    );
    let off = Item::button(id.clone().with("off"), "Off", None);
    assert!(!off.enabled, "a button with nothing to send is disabled");

    let slider = Item {
        step: (Some(Message::Undo), None),
        set_number: Some(SetNumber::Depth(loom_core::AssignmentId(1))),
        ..Item::new(id.clone().with("depth"), Kind::Slider)
    };
    assert_eq!(messages(&slider, Action::Increment, None).len(), 1);
    assert!(messages(&slider, Action::Decrement, None).is_empty(), "at its end");
    assert_eq!(
        messages(&slider, Action::SetValue, Some(&ActionData::NumericValue(10.0))).len(),
        1
    );
    assert!(messages(&slider, Action::SetValue, Some(&ActionData::Value("10".into()))).is_empty());

    let panel = Item::group(
        id.clone(),
        "Filter",
        vec![Item::group(id.clone().with("row"), "Row", vec![slider])],
    );
    assert_eq!(panel.walk().len(), 3);
    assert!(find(std::slice::from_ref(&panel), &id.clone().with("depth")).is_some());
    assert!(find(std::slice::from_ref(&panel), &id.with("nothing")).is_none());
    let text = Item::text(UiId::panel("x"), "Words");
    assert_eq!((text.label, text.value.as_deref()), (None, Some("Words")));
}
