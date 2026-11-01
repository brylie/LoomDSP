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

/// A full live-value table is reported on entry only, and again after the
/// patch went back below the limit.
#[test]
fn a_full_live_value_table_is_reported_on_entry() {
    let mut alerts = Alerts::default();
    assert!(!alerts.live_values_full(false));
    assert!(alerts.live_values_full(true), "entering the full state");
    assert!(!alerts.live_values_full(true), "still full: no second report");
    assert!(!alerts.live_values_full(false));
    assert!(alerts.live_values_full(true), "full again after it had room");
}

/// A stopped recording is reported once.
#[test]
fn a_stopped_recording_is_reported_once() {
    let mut alerts = Alerts::default();
    assert!(alerts.recording_stopped());
    assert!(!alerts.recording_stopped());
}

/// The status line can be replaced, added to, and taken.
#[test]
fn the_status_line_is_replaced_added_to_and_taken() {
    let mut alerts = Alerts::new("opened drone.ron".to_string());
    alerts.append("; 2 notes");
    assert_eq!(alerts.status(), "opened drone.ron; 2 notes");
    alerts.say("saved");
    assert_eq!(alerts.take_status(), "saved");
    assert_eq!(alerts.status(), "");
}
