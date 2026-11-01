use super::*;

/// `--seconds` must be a positive number a `Duration` can hold.
#[test]
fn length_takes_positive_numbers_only() {
    assert_eq!(length(1.5), Ok(Duration::from_millis(1500)));
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(length(bad).unwrap_err().contains("positive"), "{bad}");
    }
    assert!(length(1e300).unwrap_err().contains("too large"));
}

/// The wait lasts at least the length asked for.
#[test]
fn wait_lasts_the_length() {
    let started = Instant::now();
    wait(Duration::from_millis(20));
    assert!(started.elapsed() >= Duration::from_millis(20));
}

/// A clean capture passes; dropped frames and stream errors each fail
/// it, with the count in the message.
#[test]
fn check_fails_dropped_frames_and_stream_errors() {
    let clean = CaptureReport {
        frames: 100,
        dropped: 0,
        stream_errors: 0,
    };
    assert_eq!(check(&clean), Ok(()));
    let dropped = CaptureReport { dropped: 3, ..clean };
    assert!(check(&dropped).unwrap_err().contains("3 frames were dropped"));
    let errors = CaptureReport {
        stream_errors: 2,
        ..clean
    };
    assert!(check(&errors).unwrap_err().contains("reported 2 errors"));
}
