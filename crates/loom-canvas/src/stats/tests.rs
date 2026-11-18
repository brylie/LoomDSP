use super::*;

#[test]
fn counts_for_the_thread_and_the_process() {
    let process = RenderCounts::process();
    let thread = RenderCounts::this_thread();
    count(Counter::ControlBuilds);
    assert_eq!(RenderCounts::this_thread().control_builds, thread.control_builds + 1);
    assert_eq!(RenderCounts::this_thread().scene_builds, thread.scene_builds);
    assert!(RenderCounts::process().control_builds > process.control_builds);
}

/// A frame requested by two kinds counts once under each kind.
#[test]
fn one_frame_can_have_two_causes() {
    let before = FrameCauses::process();
    request_frame(FrameCause::Tick);
    request_frame(FrameCause::Live);
    request_frame(FrameCause::Tick);
    frame();
    let delta = FrameCauses::process().since(before);
    assert_eq!(delta.0[FrameCause::Tick as usize], 1);
    assert_eq!(delta.0[FrameCause::Live as usize], 1);
}

/// One counter per cause, and the live causes are the readings and the
/// frames that change no view (frame-meter.md, Causes).
#[test]
fn lights_are_a_live_cause_of_their_own() {
    assert_eq!(FrameCause::Other as usize + 1, CAUSE_COUNT);
    assert_ne!(FrameCause::Lights as usize, FrameCause::Meters as usize);
    for cause in [
        FrameCause::Tick,
        FrameCause::Live,
        FrameCause::Meters,
        FrameCause::Lights,
        FrameCause::Forced,
    ] {
        assert!(cause.is_live(), "{cause:?} is live");
    }
    for cause in [
        FrameCause::Control,
        FrameCause::Input,
        FrameCause::Redraw,
        FrameCause::Audio,
        FrameCause::Lifecycle,
        FrameCause::Other,
    ] {
        assert!(!cause.is_live(), "{cause:?} is not live");
    }
}
