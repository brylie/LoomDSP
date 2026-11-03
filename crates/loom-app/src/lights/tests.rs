use super::*;

/// [`super::changed`], for entries that are not readouts.
fn changed(last: &[Option<i32>], readings: &[f32], ranges: &[Option<DisplayRange>]) -> Option<Vec<Option<i32>>> {
    super::changed(last, readings, ranges, &vec![None; readings.len()])
}

#[test]
fn only_visible_changes_are_sent() {
    let ranges = [None; 2];
    let first = changed(&[None, None], &[0.0, 1.0], &ranges).expect("the first reading is sent");
    assert_eq!(first, vec![Some(0), Some(255)]);
    // Less than one level of 256: not drawn, not sent.
    assert_eq!(changed(&first, &[0.001, 0.999], &ranges), None);
    assert_eq!(changed(&first, &[0.5, 1.0], &ranges), Some(vec![Some(128), Some(255)]));
    // Out of range values are clamped, as the light draws them.
    assert_eq!(changed(&first, &[-3.0, 7.0], &ranges), None);
}

/// A meter on a display value moves within its own range, so a value below 0
/// is a visible change. Clamped to 0..1, every such value would read the same.
#[test]
fn a_change_is_measured_in_the_range_of_the_value() {
    let ranges = [Some(DisplayRange {
        format: loom_core::DisplayFormat::Number { decimals: 1 },
        min: -36.0,
        max: 0.0,
        unit: "dB",
    })];
    let first = changed(&[None], &[0.0], &ranges).expect("the first reading is sent");
    assert_eq!(first, vec![Some(255)]);
    assert_eq!(changed(&first, &[-18.0], &ranges), Some(vec![Some(128)]));
    // Less than one level of 256 is about 0.14 dB here.
    assert_eq!(changed(&first, &[-0.01], &ranges), None);
    // Past the end of the range the bar stops, but the text changes.
    let far = changed(&first, &[-40.0], &ranges).expect("a change of text is sent");
    assert!(changed(&far, &[-60.0], &ranges).is_some());
    assert_eq!(changed(&far, &[-40.01], &ranges), None);
}

/// A meter measures a change in the display value's range. A light on the same
/// value measures it as brightness, because that is what it draws.
#[test]
fn a_light_compares_brightness_and_a_meter_its_range() {
    let d = &loom_modules::dynamics::DYNAMICS;
    let meter = range(d, &Widget::meter(0, 0, "reduction")).expect("a meter has a range");
    assert_eq!(meter.min, -36.0);
    assert_eq!(range(d, &Widget::light(0, 0, "reduction")), None);
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
        if let Some(levels) = changed(&last, &[fade.brightness(reading, now)], &[None]) {
            sent.push(tick);
            last = levels;
        }
    }
    // Full, three steps of the fade, then dark: 120 ms at 33 ms a tick.
    assert_eq!(sent, [0, 1, 2, 3, 4]);
    assert_eq!(last, vec![Some(0)]);
}

/// A range that writes whole numbers, for the readout tests.
fn whole_numbers() -> DisplayRange {
    DisplayRange {
        min: 0.0,
        max: 100.0,
        unit: "",
        format: loom_core::DisplayFormat::Number { decimals: 0 },
    }
}

/// A readout's text changes at most 10 times a second, and only when it looks
/// different, even while a light beside it fades at 30 Hz and sends a snapshot
/// each time (panel-widgets.md, Readout).
#[test]
fn a_readout_holds_its_value_under_a_30_hz_light() {
    let start = Instant::now();
    let readouts = [None, Some(whole_numbers())];
    let ranges = [None, None];
    let mut holds = vec![ReadoutHold::default(); 2];
    let mut last = vec![None; 2];
    let mut sends_with_new_text = 0;
    let mut shown = f32::NAN;
    // One second at 30 snapshots. The readout's value moves every time, and
    // the light's brightness does too, so every snapshot is sent.
    for frame in 0..30_u32 {
        let now = start + INTERVAL * frame;
        let mut snapshot = vec![((1, 0), frame as f32 / 30.0), ((1, 1), frame as f32 * 2.0)];
        hold_readouts(&mut snapshot, &readouts, &mut holds, now);
        let readings: Vec<f32> = snapshot.iter().map(|&(_, r)| r).collect();
        if let Some(levels) = super::changed(&last, &readings, &ranges, &readouts) {
            last = levels;
            if snapshot[1].1.to_bits() != shown.to_bits() {
                shown = snapshot[1].1;
                sends_with_new_text += 1;
            }
        }
    }
    assert!(
        (7..=10).contains(&sends_with_new_text),
        "at most 10 changes a second, got {sends_with_new_text}"
    );
}

/// A reading whose text does not change sends nothing, however long it runs. A
/// change that does not show in the text keeps the value already shown.
#[test]
fn a_readout_sends_nothing_while_its_text_is_the_same() {
    let start = Instant::now();
    let range = whole_numbers();
    let mut hold = ReadoutHold::default();
    assert_eq!(hold.held(7.2, &range, start), 7.2, "the first reading shows");
    let later = start + READOUT_INTERVAL * 5;
    assert_eq!(hold.held(7.4, &range, later), 7.2, "still reads 7");
    assert_eq!(
        hold.held(8.0, &range, later),
        8.0,
        "reads 8, and enough time has passed"
    );
    assert_eq!(hold.held(9.0, &range, later), 8.0, "too soon");
    let readouts = [Some(range)];
    let ranges = [None];
    let first = super::changed(&[None], &[8.0], &ranges, &readouts).unwrap();
    assert_eq!(super::changed(&first, &[8.0], &ranges, &readouts), None);
}

/// Only a readout gets a readout range.
#[test]
fn only_a_readout_writes_its_value_as_text() {
    let d = &loom_modules::test_modules::READOUT;
    assert_eq!(
        readout(d, &Widget::readout(0, 0, 3, "count")).map(|r| r.max),
        Some(100.0)
    );
    assert_eq!(readout(d, &Widget::meter(0, 0, "cents")), None);
    assert_eq!(readout(d, &Widget::light(0, 0, "cents")), None);
}
