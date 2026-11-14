use super::*;
use std::time::Duration;

/// A turn's result with nothing to do after it.
fn idle() -> Next {
    Next {
        draw: false,
        again: false,
        frames: false,
        deadline: None,
    }
}

/// A turn that says to draw.
fn draws() -> Next {
    Next { draw: true, ..idle() }
}

/// The case the idle criterion rests on: a turn that changed nothing asks
/// for no frame, and the loop sleeps with no time to wake at.
#[test]
fn nothing_asked_nothing_drawn() {
    let mut pacing = Pacing::default();
    assert!(!pacing.turn(&idle()));
    assert!(!pacing.layers(Redraw::No));
    assert_eq!(pacing.wait(), Wait::Sleep);
    assert!(!pacing.frame_ended(), "and no frame follows one");
}

/// Several turns that say to draw before the frame comes ask once, and
/// the frame that comes is the one asked for.
#[test]
fn many_asks_make_one_frame() {
    let mut pacing = Pacing::default();
    assert!(pacing.turn(&draws()), "the first ask goes to the window");
    assert!(!pacing.turn(&draws()), "the second waits for the same frame");
    assert!(!pacing.layers(Redraw::NextFrame), "and so does the layers' ask");
    assert_eq!(pacing.frame_begins(), FrameKind::Asked);
    assert!(!pacing.frame_ended());
    assert!(pacing.turn(&draws()), "after the frame, a new ask goes again");
}

/// A frame nobody asked for is the system's, so the runtime hears
/// `RedrawRequested`; one the shell asked for is not.
#[test]
fn a_frame_nobody_asked_for_is_the_systems() {
    let mut pacing = Pacing::default();
    assert_eq!(pacing.frame_begins(), FrameKind::System);
    assert!(pacing.layers(Redraw::NextFrame));
    assert_eq!(pacing.frame_begins(), FrameKind::Asked);
    assert_eq!(pacing.frame_begins(), FrameKind::System, "an ask is used once");
}

/// While the runtime animates, each frame is an animation frame and asks
/// for the next; once it stops, frames stop.
#[test]
fn animation_asks_a_frame_after_each_frame() {
    let mut pacing = Pacing::default();
    let animating = Next { frames: true, ..idle() };
    assert!(pacing.turn(&animating));
    assert_eq!(pacing.frame_begins(), FrameKind::Animation);
    assert!(pacing.frame_ended(), "the next frame is asked for");
    assert_eq!(pacing.frame_begins(), FrameKind::Animation);
    assert!(!pacing.turn(&idle()), "the turn that stops asks nothing more");
    assert!(!pacing.frame_ended());
    assert_eq!(pacing.wait(), Wait::Sleep);
}

/// The turn that begins an animation frame asks nothing itself, so the
/// next frame is still asked for, once, as the shell draws: after the
/// `Presented` turn, or at the frame's end when nothing was shown. Taking
/// it as an ordinary turn left an ask standing, and the animation
/// stopped after one frame.
#[test]
fn the_frame_turn_leaves_the_next_frame_to_follow() {
    let mut pacing = Pacing::default();
    let animating = Next { frames: true, ..idle() };
    assert!(pacing.turn(&animating));
    assert_eq!(pacing.frame_begins(), FrameKind::Animation);
    pacing.frame_turn(&Next {
        draw: true,
        ..animating
    });
    assert!(pacing.turn(&animating), "the Presented turn asks for the next");
    assert!(!pacing.frame_ended(), "and only once");
    assert_eq!(pacing.frame_begins(), FrameKind::Animation);
    pacing.frame_turn(&animating);
    assert!(pacing.frame_ended(), "with no frame shown, the end asks");
}

/// The frame turn takes the runtime's state: a deadline and waiting
/// messages it reports are kept, and an animation that stops stops.
#[test]
fn the_frame_turn_takes_the_runtimes_state() {
    let mut pacing = Pacing::default();
    let at = Instant::now() + Duration::from_millis(5);
    pacing.frame_turn(&Next {
        deadline: Some(at),
        ..idle()
    });
    assert_eq!(pacing.wait(), Wait::Until(at));
    assert!(!pacing.frame_ended(), "no animation, no next frame");
    pacing.frame_turn(&Next { again: true, ..idle() });
    assert_eq!(pacing.wait(), Wait::Poll);
}

/// The loop wakes at the earlier of the runtime's deadline and the layers'
/// time.
#[test]
fn the_earliest_time_wins() {
    let now = Instant::now();
    let soon = now + Duration::from_millis(10);
    let later = now + Duration::from_millis(500);
    let mut pacing = Pacing::default();
    pacing.turn(&Next {
        deadline: Some(later),
        ..idle()
    });
    assert_eq!(pacing.wait(), Wait::Until(later));
    pacing.layers(Redraw::At(soon));
    assert_eq!(pacing.wait(), Wait::Until(soon));
    pacing.layers(Redraw::At(later));
    assert_eq!(pacing.wait(), Wait::Until(soon), "a later time keeps the earlier");
}

/// At the layers' time the shell asks for a frame, once; at the runtime's
/// deadline it runs a turn.
#[test]
fn what_comes_due_is_done_once() {
    let now = Instant::now();
    let at = now + Duration::from_millis(10);
    let mut pacing = Pacing::default();
    pacing.layers(Redraw::At(at));
    pacing.turn(&Next {
        deadline: Some(at),
        ..idle()
    });
    assert_eq!(pacing.due(now), Due::default(), "nothing is due before its time");
    assert_eq!(
        pacing.due(at),
        Due {
            deadline: true,
            redraw: true
        }
    );
    assert!(!pacing.due(at).redraw, "the layers' time is used once");
    assert_eq!(pacing.frame_begins(), FrameKind::Asked);
}

/// Messages that wait make the loop run another turn at once.
#[test]
fn waiting_messages_poll() {
    let mut pacing = Pacing::default();
    pacing.turn(&Next { again: true, ..idle() });
    assert_eq!(pacing.wait(), Wait::Poll);
    pacing.turn(&idle());
    assert_eq!(pacing.wait(), Wait::Sleep);
}
