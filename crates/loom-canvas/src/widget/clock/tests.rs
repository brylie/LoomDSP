use super::*;
use std::time::Duration;

/// The system clock reads the system's time, and it moves on by itself.
#[test]
fn the_system_clock_reads_the_system_time() {
    let before = Instant::now();
    let read = SystemClock.now();
    assert!(read >= before, "not before the call");
    std::thread::sleep(Duration::from_millis(2));
    assert!(SystemClock.now() > read, "it moves on by itself");
}
