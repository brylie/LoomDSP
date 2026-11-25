//! Reading a pipe with a deadline the system holds (ui-layer.md, section 8,
//! rule 2), for the Wayland clipboard, where the app that copied writes the
//! text into a pipe and may never finish.
//!
//! The deadline is a timer file descriptor, armed once, which `poll` waits
//! on beside the pipe. So it bounds the whole read, even one whose writer
//! sends a byte at a time, and the crate never reads the clock. When the
//! read ends, the caller drops the pipe, which closes our end: the writer's
//! next write fails, and nothing on our side is left waiting.

use rustix::event::{PollFd, PollFlags, poll};
use rustix::io::Errno;
use rustix::time::{
    Itimerspec, TimerfdClockId, TimerfdFlags, TimerfdTimerFlags, Timespec, timerfd_create, timerfd_settime,
};
use std::io::{ErrorKind, Read};
use std::os::fd::{AsFd, OwnedFd};
use std::time::Duration;

/// Why a read ended without the writer's bytes. Each is answered
/// `Refused`; they are told apart only so the tests can check each path.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Unread {
    /// The deadline came first.
    Late,
    /// The writer sent more than the read takes. *Why:* within the deadline
    /// an owner could still send far more than any text, and the read must
    /// not grow without bound.
    TooLong,
    /// The timer, `poll`, or the read failed. The read stops at once: it
    /// never goes on with no deadline.
    Failed,
}

/// How much one `read` takes from the pipe at a time.
const CHUNK: usize = 64 * 1024;

/// Reads `pipe` to its end, or until `limit` has passed since the call, or
/// until it has sent more than `max` bytes.
pub(super) fn read_within(pipe: &mut (impl Read + AsFd), limit: Duration, max: usize) -> Result<Vec<u8>, Unread> {
    read_with(pipe, max, || timer(limit))
}

/// Reads `pipe` to its end, or until the timer `make` returns fires, or
/// until it has sent more than `max` bytes. A timer that cannot be made
/// fails the read. *Why a parameter:* a test gives a timer that never
/// fires or one that fails.
pub(super) fn read_with<T: AsFd>(
    pipe: &mut (impl Read + AsFd),
    max: usize,
    make: impl FnOnce() -> std::io::Result<T>,
) -> Result<Vec<u8>, Unread> {
    let timer = make().map_err(|_| Unread::Failed)?;
    let mut bytes = Vec::new();
    let mut chunk = vec![0; CHUNK];
    loop {
        let (pipe_ready, timer_ready) = wait(pipe, &timer)?;
        // The deadline wins a tie, so a read never ends later than it.
        if timer_ready {
            return Err(Unread::Late);
        }
        if !pipe_ready {
            continue;
        }
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(read) if bytes.len() + read > max => return Err(Unread::TooLong),
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err(Unread::Failed),
        }
    }
}

/// Waits until the pipe has bytes or has closed, or the timer fires, and
/// says which. A signal that cuts the wait short waits again: the timer
/// keeps its own deadline. An error on either is a failure.
fn wait(pipe: &impl AsFd, timer: &impl AsFd) -> Result<(bool, bool), Unread> {
    loop {
        let mut fds = [PollFd::new(pipe, PollFlags::IN), PollFd::new(timer, PollFlags::IN)];
        match poll(&mut fds, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(_) => return Err(Unread::Failed),
        }
        let [pipe, timer] = fds.map(|fd| fd.revents());
        let broken = PollFlags::ERR | PollFlags::NVAL;
        if pipe.intersects(broken) || timer.intersects(broken) {
            return Err(Unread::Failed);
        }
        // A closed writer is `HUP`: the read that follows gives the bytes
        // left, then the end.
        return Ok((
            pipe.intersects(PollFlags::IN | PollFlags::HUP),
            timer.contains(PollFlags::IN),
        ));
    }
}

/// A timer that fires once, `limit` from now, on the system's monotonic
/// clock, which no change of the wall clock moves.
fn timer(limit: Duration) -> std::io::Result<OwnedFd> {
    let timer = timerfd_create(TimerfdClockId::Monotonic, TimerfdFlags::CLOEXEC)?;
    // A zero time disarms a timer, so the shortest limit is a nanosecond.
    let value = Timespec::try_from(limit.max(Duration::from_nanos(1))).map_err(|_| ErrorKind::InvalidInput)?;
    let once = Itimerspec {
        it_interval: Timespec { tv_sec: 0, tv_nsec: 0 },
        it_value: value,
    };
    timerfd_settime(&timer, TimerfdTimerFlags::empty(), &once)?;
    Ok(timer)
}

#[cfg(test)]
mod tests;
