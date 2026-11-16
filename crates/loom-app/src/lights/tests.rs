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
    let (_app, lights) = lights_on_screen();
    let list = [
        lights.levels[0],
        Entry {
            readout: Some(whole_numbers()),
            ..lights.levels[1]
        },
    ];
    let mut tracker = Tracker::default();
    let mut sends_with_new_text = 0;
    let mut shown = f32::NAN;
    // One second at 30 snapshots. The readout's value moves every time, and
    // the light's brightness does too, so every snapshot is sent.
    for frame in 0..30_u32 {
        let now = start + INTERVAL * frame;
        let readings = [frame as f32 / 30.0, frame as f32 * 2.0];
        if let Some(change) = tracker.read(&list, &readings, now) {
            tracker.sent(&change);
            if change.snapshot[1].1.to_bits() != shown.to_bits() {
                shown = change.snapshot[1].1;
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

/// Lights from a real engine, as `App::watched_lights` lists them: Clock
/// Divide's trigger light, and the level lights of two envelopes.
struct TestLights {
    trigger: Entry,
    levels: [Entry; 2],
}

/// An app with a Clock Divide and two envelopes on screen, and their lights.
fn lights_on_screen() -> (crate::App, TestLights) {
    let mut app = crate::tests::app();
    let divide = app.session.controller.add_module("loom.logic.divide").unwrap();
    let first = app.session.controller.add_module("loom.env.envelope").unwrap();
    let second = app.session.controller.add_module("loom.env.envelope").unwrap();
    app.commit();
    app.set_visible_displays(vec![divide.0, first.0, second.0]);
    let on = app.watched_lights().expect("lights on screen");
    let find = |module: loom_core::ModuleId, kind: DisplayKind| {
        *on.lights
            .iter()
            .find(|entry| entry.light.0 == module.0 && entry.source.kind() == kind)
            .expect("the light")
    };
    let lights = TestLights {
        trigger: find(divide, DisplayKind::Trigger),
        levels: [find(first, DisplayKind::Level), find(second, DisplayKind::Level)],
    };
    (app, lights)
}

/// The value `change` shows for `light`.
fn shown(change: &Change, light: Light) -> f32 {
    change
        .snapshot
        .iter()
        .find(|(l, _)| *l == light)
        .map(|&(_, value)| value)
        .expect("the light is in the snapshot")
}

/// A trigger light that stays on the list while another leaves keeps its
/// fade: it goes on fading, and does not flash again (panels.md, Redraw, A
/// pan or zoom does not disturb a light).
#[test]
fn a_light_that_stays_keeps_its_fade() {
    let (_app, lights) = lights_on_screen();
    let (trigger, level) = (lights.trigger, lights.levels[0]);
    let start = Instant::now();
    let mut tracker = Tracker::default();
    let first = tracker.read(&[trigger, level], &[0.0, 0.5], start).unwrap();
    tracker.sent(&first);
    let lit = tracker
        .read(&[trigger, level], &[1.0, 0.5], start + INTERVAL)
        .expect("a trigger lights it");
    assert_eq!(shown(&lit, trigger.light), 1.0);
    tracker.sent(&lit);
    // A pan: the level light leaves, the trigger light stays.
    let later = tracker
        .read(&[trigger], &[0.0], start + INTERVAL * 2)
        .expect("the fade goes on");
    let fading = shown(&later, trigger.light);
    assert!(
        fading > 0.0 && fading < 1.0,
        "still fading, not flashed again: {fading}"
    );
}

/// A light new to the list has no last step, so its first reading is sent,
/// even one the same as what the other lights show.
#[test]
fn a_new_lights_first_reading_is_sent() {
    let (_app, lights) = lights_on_screen();
    let [first, second] = lights.levels;
    let start = Instant::now();
    let mut tracker = Tracker::default();
    let change = tracker.read(&[first], &[0.0], start).expect("the first reading");
    tracker.sent(&change);
    assert_eq!(tracker.read(&[first], &[0.0], start + INTERVAL), None, "nothing moved");
    let change = tracker
        .read(&[first, second], &[0.0, 0.0], start + INTERVAL * 2)
        .expect("a light came into view");
    assert_eq!(change.snapshot, vec![(first.light, 0.0), (second.light, 0.0)]);
}

/// A trigger light new to the list throws its first read away and starts
/// dark, though the latch held a trigger. It flashes at the next trigger.
#[test]
fn a_trigger_light_new_to_the_list_starts_dark() {
    let (_app, lights) = lights_on_screen();
    let trigger = lights.trigger;
    let start = Instant::now();
    let mut tracker = Tracker::default();
    let change = tracker.read(&[trigger], &[1.0], start).expect("its first reading");
    assert_eq!(shown(&change, trigger.light), 0.0, "an old trigger shows nothing");
    tracker.sent(&change);
    assert_eq!(tracker.read(&[trigger], &[0.0], start + INTERVAL), None, "still dark");
    let change = tracker
        .read(&[trigger], &[1.0], start + INTERVAL * 2)
        .expect("a new trigger");
    assert_eq!(shown(&change, trigger.light), 1.0, "flashes at the next trigger");
}

/// A light that leaves the list is forgotten: back on it, its first reading
/// is sent again, and a trigger light starts dark again, mid-fade or not.
#[test]
fn a_light_that_leaves_is_forgotten() {
    let (_app, lights) = lights_on_screen();
    let (trigger, level) = (lights.trigger, lights.levels[0]);
    let start = Instant::now();
    let mut tracker = Tracker::default();
    let both = [trigger, level];
    let change = tracker.read(&both, &[0.0, 0.5], start).unwrap();
    tracker.sent(&change);
    let change = tracker.read(&both, &[1.0, 0.5], start + INTERVAL).unwrap();
    tracker.sent(&change);
    // Both leave mid-fade, and nothing is sent for lights not on screen.
    assert_eq!(tracker.read(&[], &[], start + INTERVAL * 2), None);
    let back = tracker
        .read(&both, &[1.0, 0.5], start + INTERVAL * 3)
        .expect("both are new again");
    assert_eq!(shown(&back, trigger.light), 0.0, "the trigger light starts dark");
    assert_eq!(shown(&back, level.light), 0.5, "the level light is sent again");
}

/// A light whose display source changed while it stayed on the list starts
/// again: its first reading from the new source is sent.
#[test]
fn a_light_whose_source_changes_starts_again() {
    let (_app, lights) = lights_on_screen();
    let [first, second] = lights.levels;
    let start = Instant::now();
    let mut tracker = Tracker::default();
    let change = tracker.read(&[first], &[0.5], start).unwrap();
    tracker.sent(&change);
    let moved = Entry {
        source: second.source,
        ..first
    };
    assert!(
        tracker.read(&[moved], &[0.5], start + INTERVAL).is_some(),
        "a new source counts as a new light"
    );
}

/// `--frames` reads every light once with no thread, and keeps the latch:
/// a trigger that fired during the run shows lit (panels.md, Redraw).
#[test]
fn the_frames_read_shows_a_fired_trigger_lit() {
    let mut app = crate::tests::app();
    let gate = app.session.controller.add_module("loom.math.constant").unwrap();
    app.session.controller.set_param_named(gate, "value", 1.0).unwrap();
    let divide = app.session.controller.add_module("loom.logic.divide").unwrap();
    app.session.controller.set_param_named(divide, "n", 1.0).unwrap();
    app.session
        .controller
        .connect_named(gate, "out", divide, "trigger")
        .unwrap();
    app.commit();
    let d = app.session.controller.graph().node(divide).unwrap().descriptor;
    let light = d.panel.widgets.iter().position(|w| w.binds("fired")).unwrap();
    // The constant's rise is one trigger in the first frame.
    app.run_engine(4_800);
    assert_eq!(app.session.lights.get(&(divide.0, light)), Some(&1.0));
}

/// A trigger light panned away mid-fade is dark when it comes back, before
/// the thread's first reading, so the pan's own frame does not draw it lit.
/// A level light keeps its last value.
#[test]
fn a_trigger_light_panned_away_comes_back_dark() {
    let mut ui = crate::ui_test::Ui::open("sine.ron");
    let (trigger, level) = {
        let app = ui.app_mut();
        let divide = app.session.controller.add_module("loom.logic.divide").unwrap().0;
        let env = app.session.controller.add_module("loom.env.envelope").unwrap().0;
        app.session.positions.insert(divide, (600.0, 0.0));
        app.session.positions.insert(env, (600.0, 200.0));
        app.commit();
        let widget = |id: u32, name: &str| {
            let d = app
                .session
                .controller
                .graph()
                .node(loom_core::ModuleId(id))
                .unwrap()
                .descriptor;
            d.panel.widgets.iter().position(|w| w.binds(name)).unwrap()
        };
        ((divide, widget(divide, "fired")), (env, widget(env, "level")))
    };
    ui.redraw();
    let show = |ui: &mut crate::ui_test::Ui, ids: Vec<u32>| {
        let _ = ui
            .app_mut()
            .send(crate::Message::Canvas(loom_canvas::CanvasEvent::VisibleDisplays(ids)));
    };
    show(&mut ui, vec![trigger.0, level.0]);
    // Mid-fade, as the thread sends it.
    let _ = ui
        .app_mut()
        .send(crate::Message::Lights(vec![(trigger, 0.6), (level, 0.8)]));
    show(&mut ui, Vec::new());
    show(&mut ui, vec![trigger.0, level.0]);
    ui.redraw();
    let drawn = |ui: &crate::ui_test::Ui, (module, widget): Light| {
        let snapshot = ui.app().ui_snapshot();
        let tile = snapshot.tiles.iter().find(|t| t.id == module).expect("a tile");
        tile.widgets[widget].position
    };
    assert_eq!(drawn(&ui, trigger), 0.0, "the trigger light comes back dark");
    assert_eq!(drawn(&ui, level), 0.8, "the level light keeps its value");
}
