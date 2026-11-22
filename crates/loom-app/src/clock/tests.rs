use super::*;

/// The system clock moves by itself, and `advance` leaves it alone.
#[test]
fn the_system_clock_follows_the_system() {
    let clock = Clock::default();
    let before = Instant::now();
    clock.advance(Duration::from_secs(3_600));
    let now = clock.now();
    assert!(now >= before);
    assert!(now < before + Duration::from_secs(60), "advance did not move it");
}

/// A stepped clock stands still until moved, and its copies share it.
#[test]
fn a_stepped_clock_moves_only_when_told() {
    let start = Instant::now();
    let clock = Clock::stepped(start);
    let copy = clock.clone();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(clock.now(), start, "real time does not move it");
    clock.advance(Duration::from_millis(450));
    assert_eq!(copy.now(), start + Duration::from_millis(450), "a copy sees the move");
}

/// `advance_to` moves forward only, so the clock never goes back.
#[test]
fn advance_to_never_goes_back() {
    let start = Instant::now();
    let clock = Clock::stepped(start);
    clock.advance_to(start + Duration::from_secs(2));
    clock.advance_to(start + Duration::from_secs(1));
    assert_eq!(clock.now(), start + Duration::from_secs(2));
}
