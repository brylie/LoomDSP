//! Tests for `slider.rs`: the keys' steps, a drag's snapping and its fine
//! speed, a double-click's reset, and how keys group, without a runtime.
//! The runtime's own tests are in `tests/runtime.rs`.

use super::*;
use loom_ui_core::Size;

mod runtime;

/// A slider 400 px wide and 14 tall at the window's corner: its thumb's
/// centre travels from 7 to 393, 386 px.
const BOUNDS: Rect = Rect::new(Point::new(0.0, 0.0), Size::new(400.0, 14.0));

/// The point over `position` on [`BOUNDS`], on its middle line.
fn over(position: f32) -> Point {
    Point::new(7.0 + position * 386.0, 7.0)
}

/// True when `a` and `b` are within a millionth of each other.
fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-6
}

/// Asserts that `reports` are `expected`, positions to a millionth.
#[track_caller]
fn assert_reports(reports: &[Slid], expected: &[(f32, SlidePhase)]) {
    let got: Vec<(f32, SlidePhase)> = reports.iter().map(|(_, at, phase)| (*at, *phase)).collect();
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for ((at, phase), (want, want_phase)) in got.iter().zip(expected) {
        assert!(near(*at, *want) && phase == want_phase, "{got:?} is not {expected:?}");
    }
}

/// The slider's UI ID.
fn level() -> UiId {
    UiId::panel("list").with("level")
}

/// No modifiers.
const NONE: Modifiers = Modifiers::NONE;

/// Shift alone.
const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};

/// Section 6, rule 10: the arrows move 1% by default and 0.1% with Shift,
/// Page Up and Page Down 10%, and Home and End go to the ends.
#[test]
fn the_keys_step_as_the_slider_pattern_says() {
    let slider = Slider::new(0.5);
    let to = |key, modifiers| slider.after_key(0.5, key, modifiers).expect("a key it takes");
    assert!(near(to(Key::Right, NONE), 0.51));
    assert!(near(to(Key::Up, NONE), 0.51));
    assert!(near(to(Key::Left, NONE), 0.49));
    assert!(near(to(Key::Down, NONE), 0.49));
    assert!(near(to(Key::Right, SHIFT), 0.501));
    assert!(near(to(Key::Down, SHIFT), 0.499));
    assert!(near(to(Key::PageUp, NONE), 0.6));
    assert!(near(to(Key::PageDown, NONE), 0.4));
    assert_eq!(to(Key::Home, NONE), 0.0);
    assert_eq!(to(Key::End, NONE), 1.0);
}

/// The steps can be set, as for a whole-number parameter, and a key past
/// an end stops there.
#[test]
fn set_steps_are_used_and_the_ends_hold() {
    let slider = Slider::new(0.95).key_step(0.25).fine_key_step(0.05).page_step(0.5);
    assert_eq!(slider.after_key(0.95, Key::Right, NONE), Some(1.0));
    assert!(near(slider.after_key(0.5, Key::Left, NONE).expect("Left"), 0.25));
    assert!(near(slider.after_key(0.5, Key::Left, SHIFT).expect("Shift+Left"), 0.45));
    assert_eq!(slider.after_key(0.2, Key::PageDown, NONE), Some(0.0));
    assert_eq!(Slider::new(1.5).position(), 1.0, "a position is clamped");
    assert_eq!(Slider::new(f32::NAN).position(), 0.0, "and NaN is 0");
}

/// A key the pattern does not give it, or one with Ctrl, Alt, or Cmd, is
/// not taken: Space goes on to a container, and Shift goes only with an
/// arrow.
#[test]
fn other_keys_and_modifiers_are_not_taken() {
    let slider = Slider::new(0.5);
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::NONE
    };
    assert_eq!(slider.after_key(0.5, Key::Space, NONE), None);
    assert_eq!(slider.after_key(0.5, Key::Enter, NONE), None);
    assert_eq!(slider.after_key(0.5, Key::Right, ctrl), None);
    assert_eq!(slider.after_key(0.5, Key::Home, SHIFT), None);
    assert_eq!(slider.after_key(0.5, Key::PageUp, SHIFT), None);
}

/// Section 7, rule 5: Increment and Decrement move one key step.
#[test]
fn a_screen_readers_step_is_one_key_step() {
    let slider = Slider::new(0.5).key_step(0.1);
    assert!(near(slider.after_step(true), 0.6));
    assert!(near(slider.after_step(false), 0.4));
    let mut sliding = Sliding::default();
    let reports = sliding.step(&level(), 0.5, 0.6);
    assert_reports(
        &reports,
        &[
            (0.5, SlidePhase::Start),
            (0.6, SlidePhase::Move),
            (0.6, SlidePhase::End),
        ],
    );
    assert!(sliding.step(&level(), 1.0, 1.0).is_empty(), "nothing at an end");
}

/// Section 5, rule 16: a press on the track jumps there and drags from
/// there; a press on the thumb drags with no jump; the release ends it.
#[test]
fn a_press_on_the_track_jumps_then_drags() {
    let mut sliding = Sliding::default();
    let slider = Slider::new(0.5);
    let reports = sliding.press(&level(), slider, BOUNDS, over(0.25), false, None);
    assert_reports(&reports, &[(0.5, SlidePhase::Start), (0.25, SlidePhase::Move)]);
    assert!(sliding.drags());
    let moved = sliding.moved(over(0.35), false).expect("a move");
    assert!(near(moved.1, 0.35) && moved.2 == SlidePhase::Move);
    assert!(
        sliding
            .moved(Point::new(900.0, 300.0), false)
            .is_some_and(|m| m.1 == 1.0)
    );
    let end = sliding.released().expect("an end");
    assert_eq!((end.1, end.2), (1.0, SlidePhase::End));
    assert!(!sliding.drags());

    // 5.8 px from the thumb's centre is on the thumb.
    let reports = sliding.press(&level(), slider, BOUNDS, over(0.515), false, None);
    assert_reports(&reports, &[(0.5, SlidePhase::Start)]);
}

/// Shift while dragging is ten times finer, from where it went down, and
/// letting it go goes on at full speed from there, with no jump.
#[test]
fn shift_drags_ten_times_finer_from_where_it_went_down() {
    let mut sliding = Sliding::default();
    let _ = sliding.press(&level(), Slider::new(0.5), BOUNDS, over(0.5), false, None);
    let moved = sliding.moved(over(0.6), false).expect("a move");
    assert!(near(moved.1, 0.6));
    // Shift goes down here: 38.6 px further is 1%, not 10%.
    let moved = sliding.moved(over(0.7), true).expect("a fine move");
    assert!(near(moved.1, 0.61), "{}", moved.1);
    let moved = sliding.moved(over(0.8), false).expect("full speed again");
    assert!(near(moved.1, 0.71), "{}", moved.1);
}

/// A drag with a step lands on whole steps.
#[test]
fn a_stepped_drag_lands_on_its_steps() {
    let mut sliding = Sliding::default();
    let slider = Slider::new(0.0).drag(Drag::Step(0.25));
    let reports = sliding.press(&level(), slider, BOUNDS, over(0.3), false, None);
    assert_reports(&reports, &[(0.0, SlidePhase::Start), (0.25, SlidePhase::Move)]);
    assert!(sliding.moved(over(0.32), false).is_none(), "still on 0.25");
    assert!(sliding.moved(over(0.4), false).is_some_and(|m| m.1 == 0.5));
}

/// A second press soon after the first, near it, resets to the default as
/// one Start, Move, and End. Too late, too far, or with no default, it
/// drags as a press does.
#[test]
fn a_double_click_resets_to_the_default() {
    let start = Instant::now();
    let slider = Slider::new(0.5).default(0.2);
    let mut sliding = Sliding::default();
    let _ = sliding.press(&level(), slider, BOUNDS, over(0.5), false, Some(start));
    let _ = sliding.released();
    let soon = start + Duration::from_millis(300);
    let reports = sliding.press(&level(), slider, BOUNDS, over(0.5), false, Some(soon));
    assert_reports(
        &reports,
        &[
            (0.5, SlidePhase::Start),
            (0.2, SlidePhase::Move),
            (0.2, SlidePhase::End),
        ],
    );
    assert!(!sliding.drags(), "a reset holds no drag");

    let late = soon + Duration::from_millis(400);
    let _ = sliding.released();
    let _ = sliding.press(&level(), slider, BOUNDS, over(0.5), false, Some(late));
    let _ = sliding.released();
    let far = Point::new(over(0.5).x + 5.0, 7.0);
    let reports = sliding.press(&level(), slider, BOUNDS, far, false, Some(late));
    assert!(sliding.drags(), "5 px away is a new press");
    assert_eq!(reports[0].2, SlidePhase::Start);
    assert_eq!(reports.len(), 1, "on the thumb: no jump");

    let mut plain = Sliding::default();
    let no_default = Slider::new(0.5);
    let _ = plain.press(&level(), no_default, BOUNDS, over(0.5), false, Some(start));
    let _ = plain.released();
    let _ = plain.press(&level(), no_default, BOUNDS, over(0.5), false, Some(soon));
    assert!(plain.drags(), "with no default, the second press drags");
}

/// Keys group: Start and Move on the first, Move on each within 500 ms,
/// and End once the group's time is up, or focus leaves, or a new group
/// starts on another slider.
#[test]
fn keys_group_until_500_ms_after_the_last() {
    let start = Instant::now();
    let mut sliding = Sliding::default();
    let first = sliding.key(&level(), 0.5, 0.51, start);
    assert_reports(&first, &[(0.5, SlidePhase::Start), (0.51, SlidePhase::Move)]);
    assert_eq!(sliding.deadline(), Some(start + KEY_GROUP));
    let at = start + Duration::from_millis(400);
    assert!(
        near(sliding.key_base(&level(), 0.5), 0.51),
        "keys step from the group's position"
    );
    let second = sliding.key(&level(), 0.51, 0.52, at);
    assert_reports(&second, &[(0.52, SlidePhase::Move)]);
    assert!(
        sliding
            .close_keys_if(at + Duration::from_millis(499), Some(&level()))
            .is_empty()
    );
    let closed = sliding.close_keys_if(at + KEY_GROUP, Some(&level()));
    assert_reports(&closed, &[(0.52, SlidePhase::End)]);
    assert_eq!(sliding.deadline(), None);

    let _ = sliding.key(&level(), 0.5, 0.51, start);
    let closed = sliding.close_keys_if(start, None);
    assert_reports(&closed, &[(0.51, SlidePhase::End)]);

    let other = UiId::panel("list").with("other");
    let _ = sliding.key(&level(), 0.5, 0.51, start);
    let switched = sliding.key(&other, 0.2, 0.21, start);
    assert_reports(
        &switched,
        &[
            (0.51, SlidePhase::End),
            (0.2, SlidePhase::Start),
            (0.21, SlidePhase::Move),
        ],
    );
}

/// A key at an end opens no group, and one that joins a group but moves
/// nothing reports nothing, though it keeps the group open.
#[test]
fn a_key_that_moves_nothing_reports_nothing() {
    let start = Instant::now();
    let mut sliding = Sliding::default();
    assert!(sliding.key(&level(), 1.0, 1.0, start).is_empty());
    assert_eq!(sliding.deadline(), None);
    let _ = sliding.key(&level(), 0.99, 1.0, start);
    let later = start + Duration::from_millis(300);
    assert!(sliding.key(&level(), 1.0, 1.0, later).is_empty());
    assert_eq!(sliding.deadline(), Some(later + KEY_GROUP), "the group stays open");
}

/// A press closes an open group of keys before it starts its drag.
#[test]
fn a_press_closes_the_keys_group() {
    let mut sliding = Sliding::default();
    let _ = sliding.key(&level(), 0.5, 0.51, Instant::now());
    let reports = sliding.press(&level(), Slider::new(0.51), BOUNDS, over(0.51), false, None);
    assert_reports(&reports, &[(0.51, SlidePhase::End), (0.51, SlidePhase::Start)]);
}

/// The thumb stays inside the bounds at both ends.
#[test]
fn the_thumb_stays_inside_at_both_ends() {
    assert_eq!(thumb_centre(BOUNDS, 0.0), Point::new(7.0, 7.0));
    assert_eq!(thumb_centre(BOUNDS, 1.0), Point::new(393.0, 7.0));
    assert_eq!(position_at(BOUNDS, -50.0), 0.0);
    assert_eq!(position_at(BOUNDS, 500.0), 1.0);
    let narrow = Rect::new(Point::ORIGIN, Size::new(10.0, 14.0));
    assert_eq!(position_at(narrow, 5.0), 0.0, "no travel");
}
