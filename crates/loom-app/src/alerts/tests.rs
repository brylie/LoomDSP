use super::*;

#[test]
fn load_thresholds() {
    assert_eq!(load_severity(0.0), Severity::Normal);
    assert_eq!(load_severity(0.69), Severity::Normal);
    assert_eq!(load_severity(0.7), Severity::Warning);
    assert_eq!(load_severity(0.89), Severity::Warning);
    assert_eq!(load_severity(0.9), Severity::Alarm);
    assert_eq!(load_severity(1.5), Severity::Alarm);
}

#[test]
fn dropouts_are_red_for_ten_seconds_after_they_grow() {
    let start = Instant::now();
    let mut dropouts = Dropouts::default();
    dropouts.update(0, start);
    assert_eq!(dropouts.severity(start), Severity::Normal);
    dropouts.update(2, start);
    assert_eq!(dropouts.severity(start + Duration::from_secs(9)), Severity::Alarm);
    assert_eq!(dropouts.severity(start + Duration::from_secs(10)), Severity::Normal);
    // The same count again is not a new dropout.
    dropouts.update(2, start + Duration::from_secs(11));
    assert_eq!(dropouts.severity(start + Duration::from_secs(11)), Severity::Normal);
    assert_eq!(dropouts.count(), 2);
}
