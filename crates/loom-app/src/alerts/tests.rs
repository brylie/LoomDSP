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
    let mut dropouts = GrowthAlarm::default();
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

/// The status line can be replaced and added to, and each write sets how it
/// reads.
#[test]
fn the_status_line_is_replaced_and_added_to() {
    let mut alerts = Alerts::with_notes("opened drone.ron".to_string());
    assert_eq!(alerts.severity(), Severity::Warning);
    alerts.say("opened drone.ron");
    assert_eq!(alerts.severity(), Severity::Normal);
    alerts.append("; 2 notes");
    assert_eq!(alerts.status(), "opened drone.ron; 2 notes");
    assert_eq!(alerts.severity(), Severity::Normal);
    alerts.append_warning("; table full");
    assert_eq!(alerts.severity(), Severity::Warning);
    assert_eq!(Alerts::with_notes(String::new()).severity(), Severity::Normal);
}

/// An error stays kept after a later info or warning message, and the bar's
/// own field shows it only once the status line says something else.
#[test]
fn an_error_stays_after_later_messages_and_never_shows_twice() {
    let mut alerts = Alerts::default();
    alerts.error(Cause::Other, "the recording stopped");
    assert_eq!(alerts.status(), "the recording stopped", "a command's reply reads this");
    assert_eq!(alerts.severity(), Severity::Alarm);
    assert_eq!(alerts.kept_field(), None, "the status line says it");
    assert!(alerts.newest_kept().is_some());

    alerts.say("saved");
    assert_eq!(alerts.kept_field().unwrap().text, "the recording stopped");
    alerts.warn("the recording stopped");
    assert!(alerts.kept_field().is_some(), "a warning is not the kept error");
}

/// There is one kept error per cause, the newest of a cause replaces it, and
/// the bar shows the newest of all.
#[test]
fn one_error_is_kept_per_cause() {
    let mut alerts = Alerts::default();
    alerts.error(Cause::AudioOutput, "lost");
    alerts.error(Cause::File, "save failed");
    alerts.error(Cause::File, "save failed again");
    let kept: Vec<_> = alerts.kept().iter().map(|kept| kept.text.as_str()).collect();
    assert_eq!(kept, ["lost", "save failed again"]);
    assert_eq!(alerts.newest_kept().unwrap().cause, Cause::File);

    assert!(alerts.clear(Cause::File));
    assert!(!alerts.clear(Cause::File));
    assert_eq!(alerts.newest_kept().unwrap().cause, Cause::AudioOutput);
}

/// Clearing an error that is also the latest message empties the line, so it
/// does not go on saying `Error:`. Clearing leaves a newer message alone.
#[test]
fn clearing_empties_the_line_only_if_it_says_that_error() {
    let mut alerts = Alerts::default();
    alerts.error(Cause::AudioInput, "input lost");
    alerts.clear(Cause::AudioInput);
    assert_eq!(alerts.status(), "");
    assert_eq!(alerts.severity(), Severity::Normal);

    alerts.error(Cause::AudioInput, "input lost");
    alerts.say("saved");
    alerts.clear(Cause::AudioInput);
    assert_eq!(alerts.status(), "saved");
}

/// A lost output has no ×, so dismissing it does nothing. Other errors go.
#[test]
fn a_lost_output_cannot_be_dismissed() {
    let mut alerts = Alerts::default();
    alerts.error(Cause::AudioOutput, "lost");
    alerts.error(Cause::Other, "history cleared");
    assert!(!alerts.kept()[0].dismissible);
    assert!(alerts.kept()[1].dismissible);
    alerts.dismiss(Cause::AudioOutput);
    alerts.dismiss(Cause::Other);
    let kept: Vec<_> = alerts.kept().iter().map(|kept| kept.cause).collect();
    assert_eq!(kept, [Cause::AudioOutput]);
}

/// Text added to a kept error's line grows the kept text too, so the bar
/// still sees them as one message and does not show it twice.
#[test]
fn appending_to_a_kept_error_keeps_them_matched() {
    let mut alerts = Alerts::default();
    alerts.error(Cause::File, "could not load");
    alerts.append_warning(" 2 notes");
    assert_eq!(alerts.severity(), Severity::Alarm, "a warning does not lower an error");
    assert_eq!(alerts.kept_field(), None);
    assert_eq!(alerts.kept()[0].text, "could not load 2 notes");
}

/// A new app for another patch takes over the kept errors.
#[test]
fn kept_errors_move_to_the_next_app() {
    let mut old = Alerts::default();
    old.error(Cause::AudioOutput, "lost");
    let mut fresh = Alerts::default();
    fresh.carry_kept_from(&mut old);
    assert_eq!(fresh.kept().len(), 1);
    assert!(old.kept().is_empty());
}

/// The bar's words for each severity, as well as its colours (NFR-UX-03).
#[test]
fn messages_are_labelled_in_words() {
    assert_eq!(labelled(Severity::Normal, "saved"), "saved");
    assert_eq!(labelled(Severity::Warning, "refused"), "Warning: refused");
    assert_eq!(labelled(Severity::Alarm, "lost"), "Error: lost");
}
