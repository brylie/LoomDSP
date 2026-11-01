use super::*;

#[test]
fn only_visible_changes_are_sent() {
    let first = changed(&[None, None], &[0.0, 1.0]).expect("the first reading is sent");
    assert_eq!(first, vec![Some(0), Some(255)]);
    // Less than one level of 256: not drawn, not sent.
    assert_eq!(changed(&first, &[0.001, 0.999]), None);
    assert_eq!(changed(&first, &[0.5, 1.0]), Some(vec![Some(128), Some(255)]));
    // Out of range values are clamped, as the light draws them.
    assert_eq!(changed(&first, &[-3.0, 7.0]), None);
}

/// A flash starts at full and is dark 120 ms later. A new flash starts
/// over at full.
#[test]
fn a_trigger_light_fades_in_120_ms() {
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    let mut fade = Fade::default();
    assert_eq!(fade.brightness(0.0, at(0)), 0.0, "dark before any trigger");
    assert_eq!(fade.brightness(1.0, at(10)), 1.0);
    assert!((fade.brightness(0.0, at(70)) - 0.5).abs() < 1e-3);
    assert_eq!(fade.brightness(0.0, at(130)), 0.0, "dark 120 ms after the flash");
    assert_eq!(fade.brightness(0.0, at(1_000)), 0.0);
    assert_eq!(fade.brightness(1.0, at(1_010)), 1.0, "a new trigger flashes again");
}

/// The watcher sends while a trigger light fades, and nothing once it
/// is dark: an idle patch draws no frames (panels.md, Redraw).
#[test]
fn a_faded_light_asks_for_no_more_frames() {
    let start = Instant::now();
    let mut fade = Fade::default();
    let mut last = vec![Some(0)];
    let mut sent = Vec::new();
    for tick in 0..30u32 {
        let now = start + INTERVAL * tick;
        // One trigger, read at the first tick.
        let reading = if tick == 0 { 1.0 } else { 0.0 };
        if let Some(levels) = changed(&last, &[fade.brightness(reading, now)]) {
            sent.push(tick);
            last = levels;
        }
    }
    // Full, three steps of the fade, then dark: 120 ms at 33 ms a tick.
    assert_eq!(sent, [0, 1, 2, 3, 4]);
    assert_eq!(last, vec![Some(0)]);
}
