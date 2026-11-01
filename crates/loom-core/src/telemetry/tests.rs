use super::*;

#[test]
fn measure_finds_peak_and_rms() {
    assert_eq!(measure(&[]), (0.0, 0.0));
    assert_eq!(measure(&[0.5, -1.0, 0.25]).0, 1.0);
    let (peak, rms) = measure(&[1.0, -1.0, 1.0, -1.0]);
    assert_eq!((peak, rms), (1.0, 1.0));
}

#[test]
fn probe_kinds() {
    assert!(ProbeKind::Meter.meter() && !ProbeKind::Meter.scope());
    assert!(!ProbeKind::Scope.meter() && ProbeKind::Scope.scope());
    assert!(ProbeKind::MeterAndScope.meter() && ProbeKind::MeterAndScope.scope());
}
