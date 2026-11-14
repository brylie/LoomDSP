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
    assert!(!pacing.frame_ended(Drawn::Shown), "and no frame follows one");
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
    assert!(!pacing.frame_ended(Drawn::Shown));
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
    assert!(pacing.frame_ended(Drawn::Shown), "the next frame is asked for");
    assert_eq!(pacing.frame_begins(), FrameKind::Animation);
    assert!(!pacing.turn(&idle()), "the turn that stops asks nothing more");
    assert!(!pacing.frame_ended(Drawn::Shown));
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
    assert!(!pacing.frame_ended(Drawn::Shown), "and only once");
    assert_eq!(pacing.frame_begins(), FrameKind::Animation);
    pacing.frame_turn(&animating);
    assert!(pacing.frame_ended(Drawn::Shown), "with no Presented turn, the end asks");
}

/// A window with no size draws nothing, and while the runtime animates,
/// its frames ask for no more: a minimised window does not spin. The next
/// event's turn asks again, so the animation resumes.
#[test]
fn a_skipped_frame_asks_for_no_other() {
    let mut pacing = Pacing::default();
    let animating = Next { frames: true, ..idle() };
    assert!(pacing.turn(&animating));
    for _ in 0..3 {
        pacing.frame_begins();
        pacing.frame_turn(&animating);
        assert!(!pacing.frame_ended(Drawn::Skipped));
    }
    assert!(pacing.turn(&animating), "a resize's turn asks again");
}

/// A lost surface is set up again and tried once more. If the retry is
/// lost too, no more frames are asked for, animation or not, until an
/// event's turn asks. A frame that is shown allows one retry again.
#[test]
fn a_lost_surface_is_tried_again_once() {
    let mut pacing = Pacing::default();
    let animating = Next { frames: true, ..idle() };
    assert!(pacing.turn(&animating));
    pacing.frame_begins();
    assert!(pacing.frame_ended(Drawn::Retry), "the first loss retries");
    pacing.frame_begins();
    assert!(!pacing.frame_ended(Drawn::Retry), "the second waits");
    assert!(pacing.turn(&animating), "an event's turn asks again");
    pacing.frame_begins();
    assert!(pacing.frame_ended(Drawn::Shown), "a shown frame animates on");
    pacing.frame_begins();
    assert!(pacing.frame_ended(Drawn::Retry), "and a later loss retries once again");
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
    assert!(!pacing.frame_ended(Drawn::Shown), "no animation, no next frame");
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
