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
