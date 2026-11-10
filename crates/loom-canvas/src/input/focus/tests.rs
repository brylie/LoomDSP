use super::*;

/// A list with its first row highlighted.
fn popup() -> SelectPopup {
    SelectPopup {
        highlight: 0,
        scroll: 0,
        wheel: 0.0,
    }
}

/// A field holding `text`.
fn field(text: &str) -> TypedField {
    TypedField::typing(text, |_| Ok(()))
}

/// The inside of a range, on the band.
fn band() -> RangePart {
    RangePart { part: 1, grab: None }
}

/// One focus in each mode, on module 1, widget 0, by Tab.
fn every_mode() -> Vec<FocusState> {
    vec![
        FocusState::at(1, 0, true, FocusMode::Browse),
        FocusState::at(1, 0, true, FocusMode::RangePart(band())),
        FocusState::at(1, 0, true, FocusMode::Popup(popup())),
        FocusState::at(1, 0, true, FocusMode::Typing(field("12"))),
    ]
}

/// Every transition from every state: what it leaves, and that no state
/// ends with a mode on another control than the focused one. The enum makes
/// that last part true by construction; the table shows each step keeps the
/// focus where it should.
#[test]
fn every_transition_from_every_state() {
    type Step = fn(&mut FocusState);
    let steps: [(&str, Step); 10] = [
        ("set", |f| f.set(2, 3, false)),
        ("clear", FocusState::clear),
        ("pressed", FocusState::pressed),
        ("enter range", |f| f.set_range_part(Some(band()))),
        ("leave range", |f| f.set_range_part(None)),
        ("open popup", |f| f.open_popup(1, 0, popup())),
        ("close popup", FocusState::close_popup),
        ("open typing", |f| f.open_typing(field("5"))),
        ("close typing", FocusState::close_typing),
        ("take typing", |f| {
            let _ = f.take_typing();
        }),
    ];
    let mut starts = every_mode();
    starts.push(FocusState::None);
    for start in starts {
        for (name, step) in steps {
            let mut focus = start.clone();
            step(&mut focus);
            let expected = expected_after(&start, name);
            assert_eq!(focus, expected, "{name} from {start:?}");
        }
    }
}

/// What each step should leave, written out case by case.
fn expected_after(start: &FocusState, step: &str) -> FocusState {
    let control = start.control();
    let mode = start.mode().cloned();
    let via_tab = start.tab_control().is_some();
    let on = |mode| match control {
        Some((m, w)) => FocusState::at(m, w, via_tab, mode),
        None => FocusState::None,
    };
    match step {
        "set" => FocusState::at(2, 3, false, FocusMode::Browse),
        "clear" => FocusState::None,
        // A press ends the inside and the Tab look. A list or field has
        // already taken or closed on the press, so the press leaves them be.
        "pressed" => match (control, mode) {
            (Some((m, w)), Some(FocusMode::RangePart(_))) => FocusState::at(m, w, false, FocusMode::Browse),
            (Some((m, w)), Some(mode)) => FocusState::at(m, w, false, mode),
            _ => FocusState::None,
        },
        // Going inside or out only works while browsing or already inside.
        "enter range" => match mode {
            Some(FocusMode::Browse | FocusMode::RangePart(_)) => on(FocusMode::RangePart(band())),
            _ => start.clone(),
        },
        "leave range" => match mode {
            Some(FocusMode::RangePart(_)) => on(FocusMode::Browse),
            _ => start.clone(),
        },
        // Opening a list focuses its control, keeping Tab only on the same one.
        "open popup" => FocusState::at(1, 0, via_tab, FocusMode::Popup(popup())),
        "close popup" => match mode {
            Some(FocusMode::Popup(_)) => on(FocusMode::Browse),
            _ => start.clone(),
        },
        "open typing" => on(FocusMode::Typing(field("5"))),
        "close typing" | "take typing" => match mode {
            Some(FocusMode::Typing(_)) => on(FocusMode::Browse),
            _ => start.clone(),
        },
        other => panic!("no expectation for {other}"),
    }
}

/// Opening a list on another control than the focused one moves the focus
/// there, without the Tab look.
#[test]
fn a_list_on_another_control_takes_the_focus() {
    let mut focus = FocusState::at(1, 1, true, FocusMode::Browse);
    focus.open_popup(1, 0, popup());
    assert_eq!(focus, FocusState::at(1, 0, false, FocusMode::Popup(popup())));
}

/// The accessors read only their own mode.
#[test]
fn accessors_read_their_own_mode() {
    for focus in every_mode() {
        let mode = focus.mode().cloned();
        assert_eq!(focus.control(), Some((1, 0)));
        assert_eq!(focus.tab_control(), Some((1, 0)));
        assert_eq!(
            focus.range_part().is_some(),
            matches!(mode, Some(FocusMode::RangePart(_)))
        );
        assert_eq!(focus.popup().is_some(), matches!(mode, Some(FocusMode::Popup(_))));
        assert_eq!(focus.typed().is_some(), matches!(mode, Some(FocusMode::Typing(_))));
        assert_eq!(
            focus.has_overlay(),
            matches!(mode, Some(FocusMode::Popup(_) | FocusMode::Typing(_)))
        );
    }
    let none = FocusState::None;
    assert_eq!(none.control(), None);
    assert!(none.mode().is_none() && !none.has_overlay());
}

/// A click's focus has no Tab look.
#[test]
fn a_click_focus_has_no_tab_look() {
    let focus = FocusState::at(1, 0, false, FocusMode::Browse);
    assert_eq!(focus.control(), Some((1, 0)));
    assert_eq!(focus.tab_control(), None);
}

/// Changing the open list in place keeps it on its control.
#[test]
fn a_list_changes_in_place() {
    let mut focus = FocusState::at(1, 0, true, FocusMode::Popup(popup()));
    focus.popup_mut().expect("open").highlight = 4;
    assert_eq!(focus.popup().map(|p| p.highlight), Some(4));
    assert_eq!(focus.tab_control(), Some((1, 0)));
    let mut browsing = FocusState::at(1, 0, true, FocusMode::Browse);
    assert!(browsing.popup_mut().is_none());
}
