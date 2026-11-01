use super::*;

#[test]
fn reaches_target() {
    let mut s = Smoothed::new(0.0, 48_000.0, 0.005);
    s.set_target(1.0);
    let mut value = 0.0;
    for _ in 0..2_000 {
        value = s.tick();
    }
    assert_eq!(value, 1.0);
}

#[test]
fn moves_gradually() {
    let mut s = Smoothed::new(0.0, 48_000.0, 0.005);
    s.set_target(1.0);
    let first = s.tick();
    assert!(first > 0.0 && first < 0.1);
}
