use super::*;
use crate::view::severity_colour;
use iced::Rectangle;
use loom_core::{EngineConfig, TRANSPORT_MODULE, create_engine};
use std::time::Duration;

/// A full queue stages fresh modules until the idle engine drains it;
/// the next update commits them without another edit from the player.
#[test]
fn a_full_queue_retries_staged_modules() {
    let mut app = app();
    let config = EngineConfig {
        command_capacity: 4,
        ..EngineConfig::default()
    };
    let parts = create_engine(registry(true), config);
    app.controller = parts.controller;
    app.stream.keep_idle(parts.engine);
    app.stream.set_running(None);
    for _ in 0..4 {
        app.controller.add_module("loom.osc.sine").unwrap();
    }
    assert!(!app.commit());
    assert!(app.retry_commit);
    let _ = app.update(Message::Tick);
    assert!(!app.retry_commit);
    assert!(!app.controller.has_uncommitted_changes());
    assert_eq!(app.controller.last_report().modules, 4);
}

pub(crate) fn app() -> App {
    App::new(app_args())
}

/// A full live-value table warns on entry into overflow and after a
/// return below the limit, without repeating on unrelated compiles.
#[test]
fn live_value_overflow_warning_is_latched_per_patch() {
    let mut app = app();
    let parts = create_engine(registry(true), EngineConfig::default());
    app.controller = parts.controller;
    app.stream.keep_idle(parts.engine);
    app.stream.set_running(None);
    let source = app.controller.add_module("loom.math.constant").unwrap();
    let mut last = None;
    for _ in 0..257 {
        let target = app.controller.add_module("loom.math.add").unwrap();
        let offset = app
            .controller
            .graph()
            .node(target)
            .unwrap()
            .descriptor
            .param_index("offset")
            .unwrap();
        let assignment = app.controller.add_assignment(out(source), target, offset, 0.5).unwrap();
        last = Some((target, offset, assignment));
    }
    let (last_target, last_offset, last_id) = last.unwrap();
    assert!(app.commit());
    assert!(app.alerts.status().contains("table full"), "{}", app.alerts.status());
    assert!(app.commit());
    assert!(!app.alerts.status().contains("table full"), "{}", app.alerts.status());
    app.controller.remove_assignment(last_id).unwrap();
    assert!(app.commit());
    assert!(!app.alerts.status().contains("table full"), "{}", app.alerts.status());
    app.controller
        .add_assignment(out(source), last_target, last_offset, 0.5)
        .unwrap();
    assert!(app.commit());
    assert!(app.alerts.status().contains("table full"), "{}", app.alerts.status());
}

/// Audio watcher events update the warning peak and rolling maximum,
/// including the last callback time used to detect a stalled stream.
#[test]
fn audio_watcher_events_update_the_stats_model() {
    let mut app = app();
    let now = Instant::now();
    let _ = app.update(Message::Audio(audio_watch::Event::Stats {
        dropouts: 1,
        at: now,
        peak: 0.95,
        max: 0.95,
    }));
    assert_eq!(app.stream.load().0, 0.95);
    assert_eq!(app.stream.load().1, 0.95);
    assert_eq!(app.alerts.dropout_count(), 1);
    let _ = app.update(Message::Audio(audio_watch::Event::Stalled {
        callbacks: 3,
        advanced: now,
    }));
    assert_eq!(app.stream.callbacks().0, 3);
    assert!(app.stream.callbacks().1 >= now);
}

/// #16: the inspector help text names every canvas shortcut, the new
/// Cmd/Ctrl+0 zoom reset and the ones that were there before it.
#[test]
fn help_text_lists_every_canvas_shortcut() {
    for shortcut in [
        "A or a double-click",
        "Cmd/Ctrl+A",
        "Delete or Backspace",
        "Tab and Shift-Tab",
        "arrow keys",
        "Space or Enter",
        "Escape clears control focus",
        "Space toggles Run",
        "Cmd/Ctrl+0",
        "F or Home",
        "press Escape",
        "Cmd/Ctrl+C",
        "V pastes",
        "D duplicates",
        "Cmd/Ctrl+Z",
        "Cmd/Ctrl+Shift+Z",
    ] {
        assert!(HELP.contains(shortcut), "help text lacks {shortcut:?}");
    }
}

/// Text in the window chrome meets WCAG 1.4.3 (4.5:1) on the theme
/// background: the theme text colour, the dimmed `DIM` text, and the
/// stats bar warning colours. Issue #257.
#[test]
fn window_text_meets_wcag_minimum_contrast() {
    let theme = app().theme.palette();
    let pairs = [
        ("theme text on background", theme.text),
        ("DIM text on background", DIM),
        ("warning text on background", severity_colour(alerts::Severity::Warning)),
        ("alarm text on background", severity_colour(alerts::Severity::Alarm)),
    ];
    for (name, text) in pairs {
        let ratio = loom_canvas::contrast_ratio(text, theme.background);
        assert!(
            ratio >= loom_canvas::MIN_TEXT_CONTRAST,
            "{name}: {ratio:.2}:1 is below {}:1",
            loom_canvas::MIN_TEXT_CONTRAST
        );
    }
}

/// The test constructors never open a real MIDI port (issue 221): the
/// app built by `App::new` has no open inputs, and the host it uses
/// reports none, whatever is plugged into the machine.
#[test]
fn test_app_opens_no_midi_ports() {
    let app = app();
    assert!(app.rig.ports.is_empty());
    assert!(devices::Host::midi_inputs(&devices::fake::NoMidi).unwrap().is_empty());
}

pub(crate) fn app_args() -> Args {
    Args {
        patch: None,
        generate: None,
        no_audio: true,
        fake_audio: false,
        record: None,
        bench: bench::Options::default(),
        control: control::Options::default(),
        output_safety: None,
        devices: crate::devices::Options::default(),
        view: screenshot::Options::default(),
        dump_ui: false,
        select: Vec::new(),
        show_help: false,
        frames: None,
        hold_note: false,
        recovery_dir: None,
        no_recovery: false,
        test_modules: false,
        dev: false,
    }
}

/// `--record` saves what the engine plays. The file is complete once the
/// app is dropped.
#[test]
fn record_writes_a_wav_file() {
    let dir = std::env::temp_dir().join(format!("loom-app-record-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.wav");
    let app = App::new(Args {
        fake_audio: true,
        no_audio: false,
        record: Some(path.clone()),
        ..app_args()
    });
    std::thread::sleep(Duration::from_millis(200));
    drop(app);
    let reader = hound::WavReader::open(&path).unwrap();
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().sample_rate, 48_000);
    assert!(reader.duration() > 0, "some audio was recorded");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// With `--fake-audio`, a timer thread plays the engine and drains its
/// command queue, so edits keep working with no audio device (#220).
/// The edits come in bursts with short pauses, as fast as a person
/// could turn a knob, many times over.
#[test]
fn fake_audio_keeps_taking_edits() {
    let mut app = App::new(Args {
        fake_audio: true,
        no_audio: false,
        ..app_args()
    });
    assert!(app.stream.is_playing(), "the clock plays the engine");
    let filter = app.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    for burst in 0..10 {
        for i in 0..500 {
            let _ = app.update(Message::Param {
                module: filter.0,
                index: 0,
                position: ((burst * 500 + i) % 100) as f32 / 100.0,
            });
            assert!(
                !app.alerts.status().contains("queue is full"),
                "{}",
                app.alerts.status()
            );
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

/// Without an audio device the app keeps taking edits (#220). Each
/// update applies the idle engine's queued commands, so 5,000 knob
/// moves, far more than the queue holds, never fill it. The engine
/// renders nothing in the meantime.
#[test]
fn no_audio_keeps_taking_edits() {
    let mut app = app();
    assert!(!app.stream.is_playing() && app.stream.is_idle());
    let filter = app.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    let capacity = loom_core::EngineConfig::default().command_capacity;
    for i in 0..5_000 {
        let _ = app.update(Message::Param {
            module: filter.0,
            index: 0,
            position: (i % 100) as f32 / 100.0,
        });
        assert!(
            !app.alerts.status().contains("queue is full"),
            "move {i}: {}",
            app.alerts.status()
        );
    }
    assert!(5_000 > capacity);
    let stats = app.controller.stats();
    assert_eq!(stats.buffers.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert_eq!(stats.leaked.load(std::sync::atomic::Ordering::Relaxed), 0);
}

fn open_at(app: &mut App, world: Point) {
    let _ = app.update(Message::Canvas(CanvasEvent::OpenBrowser {
        world,
        at: Point::ORIGIN,
        area: Rectangle::new(Point::ORIGIN, iced::Size::new(1000.0, 800.0)),
    }));
}

fn type_query(app: &mut App, query: &str) {
    let _ = app.update(Message::Browser(browser::Message::Query(query.to_string())));
}

#[test]
fn adding_from_the_browser_is_one_undo_step_at_the_snapped_point() {
    let mut app = app();
    let before = app.controller.graph().nodes().count();
    let undo = app.edits.history.undo_step().map(|s| s.name().to_string());

    // Opening, typing, and closing are not edits.
    open_at(&mut app, Point::new(250.0, 130.0));
    type_query(&mut app, "svf");
    let _ = app.update(Message::Browser(browser::Message::Key(browser::Nav::Escape)));
    assert!(app.browser.is_none());
    assert_eq!(app.edits.history.undo_step().map(|s| s.name().to_string()), undo);

    open_at(&mut app, Point::new(250.0, 130.0));
    type_query(&mut app, "svf");
    let _ = app.update(Message::Browser(browser::Message::Submit));
    assert!(app.browser.is_none());
    assert_eq!(app.controller.graph().nodes().count(), before + 1);
    let id = *app.selection.iter().next().expect("the new module is selected");
    assert_eq!(app.selection.len(), 1);
    assert_eq!(app.positions[&id], Point::new(252.0, 132.0));

    app.undo();
    assert_eq!(app.controller.graph().nodes().count(), before);
    assert_eq!(app.edits.history.undo_step().map(|s| s.name().to_string()), undo);
}

#[test]
fn keys_go_to_the_browser_not_the_shortcuts() {
    let mut app = app();
    let before = app.controller.graph().nodes().count();
    open_at(&mut app, Point::ORIGIN);
    type_query(&mut app, "saw");
    let _ = app.update(Message::Browser(browser::Message::Submit));
    assert_eq!(app.controller.graph().nodes().count(), before + 1);

    // Cmd+Z or Ctrl+Z while the browser is open does not undo.
    open_at(&mut app, Point::ORIGIN);
    let _ = app.update(Message::Key(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character("z".into()),
        modified_key: iced::keyboard::Key::Character("z".into()),
        physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::COMMAND,
        text: None,
        repeat: false,
    }));
    assert_eq!(app.controller.graph().nodes().count(), before + 1);
}

/// An envelope and a filter, with the filter's cutoff knob widget.
fn env_and_filter(app: &mut App) -> (ModuleId, ModuleId, usize, usize) {
    let env = app.controller.add_module("loom.env.envelope").unwrap();
    let filter = app.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    let d = app.controller.graph().node(filter).unwrap().descriptor;
    let cutoff = d.param_index("cutoff").unwrap();
    let widget = d.panel.widgets.iter().position(|w| w.bind == "cutoff").unwrap();
    app.edits.history.clear();
    (env, filter, cutoff, widget)
}

fn drop_env(app: &mut App, env: ModuleId, filter: ModuleId, widget: usize) {
    let from = loom_canvas::PortRef {
        module: env.0,
        output: true,
        index: 0,
    };
    let _ = app.update(Message::Canvas(CanvasEvent::Assign {
        from,
        module: filter.0,
        widget,
    }));
}

fn assignments(app: &App) -> Vec<loom_core::Assignment> {
    app.controller.graph().assignments().map(|(_, a)| *a).collect()
}

/// Dropping an output on a knob assigns it with depth 0.25, following
/// the source's polarity. A second drop of the same output adds nothing
/// and says why. Undo and redo take the assignment back and forth.
#[test]
fn a_drop_assigns_once_and_is_one_undo_step() {
    let mut app = app();
    let (env, filter, cutoff, widget) = env_and_filter(&mut app);
    drop_env(&mut app, env, filter, widget);
    let made = assignments(&app);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].source, OutputRef { module: env, port: 0 });
    assert_eq!((made[0].target, made[0].param), (filter, cutoff));
    assert_eq!(made[0].depth, 0.25);
    assert_eq!(made[0].polarity, None);
    assert!(made[0].enabled);
    assert_eq!(
        app.edits.history.undo_step().map(undo::Step::name),
        Some("add modulation")
    );

    drop_env(&mut app, env, filter, widget);
    assert_eq!(assignments(&app).len(), 1);
    assert!(
        app.alerts.status().contains("already modulates"),
        "{}",
        app.alerts.status()
    );

    app.undo();
    assert!(assignments(&app).is_empty());
    app.redo();
    assert_eq!(assignments(&app), made);
}

/// `--test-modules` loads the `test.` modules and lists them in the
/// module browser, so the example patch for them opens. Without it the
/// browser lists only the built-ins, though tests can still add them.
#[test]
fn test_modules_are_listed_only_with_the_flag() {
    let patch = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/dev/test-widgets.ron");
    let app = App::new(Args {
        patch: Some(patch),
        test_modules: true,
        ..app_args()
    });
    // The two test modules and the patch's Transport.
    assert_eq!(app.controller.graph().nodes().count(), 3, "{}", app.alerts.status());
    assert_eq!(app.controller.graph().assignments().count(), 7);
    assert!(app.modules.iter().any(|d| d.id == "test.panel"));

    let app = self::app();
    assert!(app.modules.iter().any(|d| d.id == "loom.osc.sine"));
    assert!(!app.modules.iter().any(|d| d.id.starts_with("test.")));
}

/// A drop on a toggle gives depth 1, and on radio buttons with three
/// options 0.5: one step of the parameter. The assignment follows the
/// source's polarity, unipolar or bipolar (spec, Making an assignment).
#[test]
fn a_drop_on_a_toggle_or_radio_moves_one_step() {
    let mut app = app();
    let source = app.controller.add_module("test.panel").unwrap();
    let target = app.controller.add_module("test.panel").unwrap();
    app.commit();
    let d = app.controller.graph().node(target).unwrap().descriptor;
    let cases = [("on", 1.0), ("choice", 0.5)];
    let outputs = [(0, loom_core::Polarity::Unipolar), (1, loom_core::Polarity::Bipolar)];
    for (port, _) in outputs {
        for (bind, _) in cases {
            let widget = d.panel.widgets.iter().position(|w| w.bind == bind).unwrap();
            let _ = app.update(Message::Canvas(CanvasEvent::Assign {
                from: loom_canvas::PortRef {
                    module: source.0,
                    output: true,
                    index: port,
                },
                module: target.0,
                widget,
            }));
        }
    }

    let graph = app.controller.graph();
    assert_eq!(graph.assignments().count(), 4);
    for (port, polarity) in outputs {
        for (bind, depth) in cases {
            let param = d.param_index(bind).unwrap();
            let made = assignments(&app)
                .into_iter()
                .find(|a| a.source == OutputRef { module: source, port } && a.param == param)
                .unwrap_or_else(|| panic!("no assignment from output {port} to {bind}"));
            assert_eq!(made.target, target);
            assert_eq!(made.depth, depth, "{bind}");
            assert_eq!(made.polarity, None, "{bind} follows the source");
            let reach = modulation::reach(graph, &made).unwrap();
            assert_eq!(reach.polarity, polarity, "{bind} from output {port}");
        }
    }
}

/// **Add modulation** makes the same assignment as a drop, from a list
/// of every output in module order, where outputs that already drive
/// the parameter cannot be picked.
#[test]
fn add_modulation_matches_a_drop() {
    let mut app = app();
    let (env, filter, cutoff, widget) = env_and_filter(&mut app);
    let source = OutputRef { module: env, port: 0 };
    let graph = app.controller.graph();
    let sources = modulation::sources(graph);
    // The new patch's Transport comes first; the envelope is next.
    let transport = app.controller.transport_module();
    assert_eq!(sources.iter().find(|s| Some(s.module) != transport), Some(&source));
    let mut sorted = sources.clone();
    sorted.sort();
    assert_eq!(sources, sorted, "module order, then port order");

    let _ = app.update(Message::AddModulation {
        module: filter.0,
        index: cutoff,
    });
    assert_eq!(app.gestures.adding(), Some((filter.0, cutoff)));
    let _ = app.update(Message::Assign {
        source,
        module: filter.0,
        index: cutoff,
    });
    assert_eq!(app.gestures.adding(), None);
    let picked = assignments(&app);
    assert!(modulation::exists(app.controller.graph(), source, filter, cutoff));

    let mut dropped = self::app();
    let (env, filter, _, widget2) = env_and_filter(&mut dropped);
    assert_eq!(widget, widget2);
    drop_env(&mut dropped, env, filter, widget);
    assert_eq!(assignments(&dropped), picked);
}

/// The inspector row edits depth, polarity, and on/off, and removes. A
/// depth slider drag is one undo step. None of these but removal
/// rebuilds the scene, so the GPU keeps its tiles, ports, and cables.
#[test]
fn inspector_rows_edit_and_remove_assignments() {
    let mut app = app();
    let (env, filter, cutoff, widget) = env_and_filter(&mut app);
    drop_env(&mut app, env, filter, widget);
    let id = app.controller.graph().assignments().next().unwrap().0;
    let generation = app.scene.generation();
    let steps = app.edits.history.len();

    for depth in [0.3, 0.4, 0.5] {
        let _ = app.update(Message::Depth { id, depth });
    }
    let _ = app.update(Message::SliderReleased);
    assert_eq!(assignments(&app)[0].depth, 0.5);
    assert_eq!(app.edits.history.len(), steps + 1, "one drag, one step");

    let _ = app.update(Message::Polarity {
        id,
        choice: PolarityChoice::Bipolar,
    });
    let _ = app.update(Message::Enabled { id, enabled: false });
    let a = assignments(&app)[0];
    assert_eq!(a.polarity, Some(loom_core::Polarity::Bipolar));
    assert!(!a.enabled);
    assert_eq!(app.scene.generation(), generation, "no scene rebuild for edits");

    app.undo();
    app.undo();
    app.undo();
    assert_eq!(assignments(&app)[0].depth, 0.25);

    let _ = app.update(Message::Unassign(id));
    assert!(assignments(&app).is_empty());
    assert!(modulation::on_param(app.controller.graph(), filter, cutoff).is_empty());
}

/// The source's output shows how many assignments it drives, counting
/// those that are off, and follows adds, removals, and deletes.
#[test]
fn badges_count_every_assignment_on_an_output() {
    let mut app = app();
    let (env, filter, _, widget) = env_and_filter(&mut app);
    let badge = |app: &App| app.scene.module(env.0).unwrap().outputs[0].badge;
    assert_eq!(badge(&app), 0);
    drop_env(&mut app, env, filter, widget);
    let resonance = app
        .controller
        .graph()
        .node(filter)
        .unwrap()
        .descriptor
        .param_index("resonance")
        .unwrap();
    let _ = app.update(Message::Assign {
        source: OutputRef { module: env, port: 0 },
        module: filter.0,
        index: resonance,
    });
    assert_eq!(badge(&app), 2);
    let id = app.controller.graph().assignments().next().unwrap().0;
    let _ = app.update(Message::Enabled { id, enabled: false });
    assert_eq!(badge(&app), 2, "off still counts");
    let _ = app.update(Message::Unassign(id));
    assert_eq!(badge(&app), 1);
    let _ = app.update(Message::Canvas(CanvasEvent::Delete(vec![filter.0])));
    assert_eq!(badge(&app), 0);
}

/// **Drives** lists what each output drives, in the order added, and a
/// line selects its target.
#[test]
fn drives_lists_targets_and_selects_them() {
    let mut app = app();
    let (env, filter, cutoff, widget) = env_and_filter(&mut app);
    drop_env(&mut app, env, filter, widget);
    let drives = modulation::drives(app.controller.graph(), env);
    let targets: Vec<_> = drives[&0].iter().map(|(_, a)| (a.target, a.param)).collect();
    assert_eq!(targets, vec![(filter, cutoff)]);
    assert!(modulation::drives(app.controller.graph(), filter).is_empty());
    let _ = app.update(Message::SelectModule(filter.0));
    assert_eq!(app.selection, BTreeSet::from([filter.0]));
}

/// Saved and loaded, the inspector lists the same assignments in the
/// same order.
#[test]
fn assignments_keep_their_order_through_a_save() {
    let mut app = app();
    let (env, filter, cutoff, widget) = env_and_filter(&mut app);
    let other = app.controller.add_module("loom.osc.sine").unwrap();
    app.commit();
    let _ = app.update(Message::Assign {
        source: OutputRef { module: other, port: 0 },
        module: filter.0,
        index: cutoff,
    });
    drop_env(&mut app, env, filter, widget);
    let rows = |c: &Controller| -> Vec<(OutputRef, f32)> {
        modulation::on_param(c.graph(), filter, cutoff)
            .into_iter()
            .map(|(_, a)| (a.source, a.depth))
            .collect()
    };
    let before = rows(&app.controller);
    assert_eq!(before[0].0.module, other, "the sine was added first");

    let text = Patch::from_graph("Order", app.controller.graph()).to_ron().unwrap();
    let mut reloaded = create_engine(loom_modules::registry(), EngineConfig::default());
    Patch::from_ron(&text)
        .unwrap()
        .load_into(&mut reloaded.controller)
        .unwrap();
    assert_eq!(rows(&reloaded.controller), before);
}

/// An envelope and a sine both on the filter cutoff.
fn two_sources(app: &mut App) -> (ModuleId, ModuleId, ModuleId, usize, usize) {
    let (env, filter, cutoff, widget) = env_and_filter(app);
    let sine = app.controller.add_module("loom.osc.sine").unwrap();
    app.commit();
    drop_env(app, env, filter, widget);
    let _ = app.update(Message::Assign {
        source: OutputRef { module: sine, port: 0 },
        module: filter.0,
        index: cutoff,
    });
    (env, sine, filter, cutoff, widget)
}

fn out(module: ModuleId) -> OutputRef {
    OutputRef { module, port: 0 }
}

/// The arc drag picks the pinned source, then the selected module's
/// output, then the only enabled assignment, and otherwise nothing.
#[test]
fn the_arc_drag_picks_its_assignment_in_order() {
    let mut app = app();
    let (env, sine, filter, cutoff, _) = two_sources(&mut app);
    let graph = app.controller.graph();
    let id_of = |source: OutputRef| {
        graph
            .assignments()
            .find(|(_, a)| a.source == source)
            .map(|(id, _)| id)
            .unwrap()
    };
    let pick = |pinned, selected| modulation::drag_target(graph, filter, cutoff, pinned, selected);
    assert_eq!(pick(Some(out(sine)), Some(env)), Ok(id_of(out(sine))));
    assert_eq!(pick(None, Some(env)), Ok(id_of(out(env))));
    // A pinned output that does not drive this knob falls through.
    assert_eq!(pick(Some(out(filter)), Some(sine)), Ok(id_of(out(sine))));
    assert!(pick(None, Some(filter)).is_err(), "two enabled: none fits");
    assert!(pick(None, None).unwrap_err().contains("pin one"));

    // With one of them off, the other is the only enabled one.
    let (sine_id, env_id) = (id_of(out(sine)), id_of(out(env)));
    app.controller.set_assignment_enabled(sine_id, false).unwrap();
    let graph = app.controller.graph();
    assert_eq!(modulation::drag_target(graph, filter, cutoff, None, None), Ok(env_id));
}

/// Two assignments from the same output (only possible in a hand-written
/// patch) make the drag change nothing, even when that output is pinned.
#[test]
fn a_duplicate_source_is_not_a_drag_target() {
    let mut app = app();
    let (env, filter, cutoff, _) = env_and_filter(&mut app);
    for depth in [0.1, 0.2] {
        app.controller.add_assignment(out(env), filter, cutoff, depth).unwrap();
    }
    let graph = app.controller.graph();
    for (pinned, selected) in [(Some(out(env)), None), (None, Some(env))] {
        let why = modulation::drag_target(graph, filter, cutoff, pinned, selected).unwrap_err();
        assert!(why.contains("inspector"), "{why}");
    }
}

/// One arc drag changes one assignment's depth as one undo step. While
/// it lasts, the knob's arc shows only that assignment.
#[test]
fn an_arc_drag_is_one_step_and_shows_only_its_assignment() {
    let mut app = app();
    let (_, sine, filter, _, widget) = two_sources(&mut app);
    let steps = app.edits.history.len();
    let pinned = Some(loom_canvas::PortRef {
        module: sine.0,
        output: true,
        index: 0,
    });
    let drag = |delta| {
        Message::Canvas(CanvasEvent::DragDepth {
            module: filter.0,
            widget,
            pinned,
            delta,
        })
    };
    let mark = |app: &App| app.values.get(filter.0, widget).unwrap().modulation;
    let both = mark(&app);
    let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
    let _ = app.update(drag(0.1));
    let _ = app.update(drag(0.1));
    let during = mark(&app);
    assert_ne!(during, both, "the arc shows the dragged assignment alone");
    let _ = app.update(Message::Canvas(CanvasEvent::GestureEnd));
    assert!(!app.gestures.depth_dragging());
    let depth = |app: &App| {
        app.controller
            .graph()
            .assignments()
            .find(|(_, a)| a.source == out(sine))
            .unwrap()
            .1
            .depth
    };
    assert!((depth(&app) - 0.45).abs() < 1e-6, "{}", depth(&app));
    assert!(app.alerts.status().contains("+45%"), "{}", app.alerts.status());
    assert_eq!(app.edits.history.len(), steps + 1);
    app.undo();
    assert_eq!(depth(&app), 0.25);
}

/// A selected source picks the assignment even though the drag starts
/// on another module's knob: the press does not change the selection.
#[test]
fn a_selected_source_picks_the_arc_drag() {
    let mut app = app();
    let (env, sine, filter, _, widget) = two_sources(&mut app);
    app.selection = BTreeSet::from([sine.0]);
    let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
    let _ = app.update(Message::Canvas(CanvasEvent::DragDepth {
        module: filter.0,
        widget,
        pinned: None,
        delta: 0.1,
    }));
    let _ = app.update(Message::Canvas(CanvasEvent::GestureEnd));
    let depth = |source| {
        app.controller
            .graph()
            .assignments()
            .find(|(_, a)| a.source == out(source))
            .unwrap()
            .1
            .depth
    };
    assert!((depth(sine) - 0.35).abs() < 1e-6);
    assert_eq!(depth(env), 0.25);
}

/// With several sources and none in focus, a drag changes nothing and
/// the status bar says why.
#[test]
fn an_arc_drag_with_no_choice_changes_nothing() {
    let mut app = app();
    let (_, _, filter, _, widget) = two_sources(&mut app);
    app.selection.clear();
    let before = assignments(&app);
    let steps = app.edits.history.len();
    let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
    let _ = app.update(Message::Canvas(CanvasEvent::DragDepth {
        module: filter.0,
        widget,
        pinned: None,
        delta: 0.2,
    }));
    let _ = app.update(Message::Canvas(CanvasEvent::GestureEnd));
    assert_eq!(assignments(&app), before);
    assert_eq!(app.edits.history.len(), steps);
    assert!(
        app.alerts.status().contains("Several sources"),
        "{}",
        app.alerts.status()
    );
}

/// The scene has one line per assignment, ending on its control.
#[test]
fn the_scene_has_a_line_per_assignment() {
    let mut app = app();
    let (env, sine, filter, _, widget) = two_sources(&mut app);
    let lines: Vec<_> = app
        .scene
        .modulations()
        .iter()
        .map(|m| (m.from, m.to, m.widget))
        .collect();
    assert_eq!(
        lines,
        vec![
            ((env.0, 0), filter.0, Some(widget)),
            ((sine.0, 0), filter.0, Some(widget))
        ]
    );
}

/// Ctrl+`c` or Cmd+`c`.
pub(crate) fn command_key(c: &str) -> Message {
    Message::Key(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character(c.into()),
        modified_key: iced::keyboard::Key::Character(c.into()),
        physical_key: iced::keyboard::key::Physical::Unidentified(iced::keyboard::key::NativeCode::Unidentified),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::COMMAND,
        text: None,
        repeat: false,
    })
}

/// A sine into a multiply, both selected, at known places.
fn two_selected(app: &mut App) -> (ModuleId, ModuleId) {
    let sine = app.controller.add_module("loom.osc.sine").unwrap();
    let vca = app.controller.add_module("loom.math.mul").unwrap();
    app.controller.connect_named(sine, "out", vca, "a").unwrap();
    app.controller.set_param_named(vca, "gain", 0.4).unwrap();
    app.positions.insert(sine.0, Point::new(0.0, 0.0));
    app.positions.insert(vca.0, Point::new(240.0, 0.0));
    app.commit();
    app.edits.history.clear();
    app.selection = BTreeSet::from([sine.0, vca.0]);
    (sine, vca)
}

fn step_name(app: &App) -> Option<String> {
    app.edits.history.undo_step().map(|s| s.name().to_string())
}

/// Tile dimensions are restored by undo and redo.
#[test]
fn tile_dimensions_undo_and_redo() {
    let mut app = app();
    let id = app.controller.add_module("loom.display.scope").unwrap();
    app.commit();
    app.edits.history.clear();
    app.edit(|app| {
        app.sizes.insert(id.0, (24, 16));
        app.refresh();
    });
    assert_eq!(app.sizes.get(&id.0), Some(&(24, 16)));
    app.undo();
    assert!(!app.sizes.contains_key(&id.0));
    app.redo();
    assert_eq!(app.sizes.get(&id.0), Some(&(24, 16)));
}

/// Resize commands respect the panel minimum, maximum, and growable widgets.
#[test]
fn resize_respects_limits_and_ignores_fixed_panels() {
    let mut app = app();
    let scope = app.controller.add_module("loom.display.scope").unwrap();
    let fixed = app.controller.add_module("loom.math.constant").unwrap();
    app.commit();
    let min = app.scene.module(scope.0).unwrap().minimum_size();
    app.canvas_event(CanvasEvent::Resize {
        module: scope.0,
        size: (255, 255),
    });
    let tile = app.scene.module(scope.0).unwrap();
    assert_eq!(
        (tile.width(), tile.height()),
        (64.0 * loom_canvas::GRID, 64.0 * loom_canvas::GRID)
    );
    app.canvas_event(CanvasEvent::Resize {
        module: scope.0,
        size: (0, 0),
    });
    assert_eq!(app.scene.module(scope.0).unwrap().bounds().size(), min);
    let generation = app.scene.generation();
    app.canvas_event(CanvasEvent::Resize {
        module: fixed.0,
        size: (64, 64),
    });
    assert_eq!(app.scene.generation(), generation);
    assert!(!app.sizes.contains_key(&fixed.0));
}

/// Copy and paste carry a saved tile size to the new module ID.
#[test]
fn copy_and_paste_keep_tile_size() {
    let mut app = app();
    let id = app.controller.add_module("loom.display.scope").unwrap();
    app.positions.insert(id.0, Point::ORIGIN);
    app.sizes.insert(id.0, (24, 16));
    app.selection = BTreeSet::from([id.0]);
    app.commit();
    let text = app.copy().unwrap();
    app.paste(Point::new(100.0, 100.0), Some(text));
    let pasted = *app.selection.iter().next().unwrap();
    assert_ne!(pasted, id.0);
    assert_eq!(app.sizes.get(&pasted), Some(&(24, 16)));
}

#[test]
fn copy_with_nothing_selected_writes_nothing() {
    let mut app = app();
    app.selection.clear();
    assert!(app.copy().is_none());
    assert_eq!(app.alerts.status(), "Nothing selected to copy");
    assert!(app.edits.history.undo_step().is_none(), "copy is not an edit");
}

#[test]
fn a_paste_lands_at_the_snapped_point_is_selected_and_is_one_undo_step() {
    let mut app = app();
    let (sine, vca) = two_selected(&mut app);
    let text = app.copy().unwrap();
    assert_eq!(app.alerts.status(), "Copied 2 modules");
    assert!(app.edits.history.undo_step().is_none(), "copy is not an edit");

    let before = app.controller.graph().nodes().count();
    app.paste(Point::new(605.0, 299.0), Some(text));
    assert_eq!(app.controller.graph().nodes().count(), before + 2);
    assert_eq!(app.alerts.status(), "Pasted 2 modules");
    assert_eq!(step_name(&app).as_deref(), Some("paste 2 modules"));
    let new: Vec<u32> = app.selection.iter().copied().collect();
    assert_eq!(new.len(), 2, "only the pasted modules are selected");
    assert!(!new.contains(&sine.0) && !new.contains(&vca.0));
    assert_eq!(app.positions[&new[0]], Point::new(600.0, 300.0));
    assert_eq!(app.positions[&new[1]], Point::new(840.0, 300.0));

    app.undo();
    assert_eq!(app.controller.graph().nodes().count(), before);
    app.redo();
    let back: Vec<u32> = app
        .controller
        .graph()
        .nodes()
        .map(|(id, _)| id.0)
        .filter(|id| new.contains(id))
        .collect();
    assert_eq!(back, new, "redo brings the paste back with the same ids");
    assert_eq!(app.controller.graph().cables().count(), 2);
}

#[test]
fn duplicate_from_the_keyboard_is_one_step_offset_by_two_grid_units() {
    let mut app = app();
    let (sine, _) = two_selected(&mut app);
    let before = app.controller.graph().nodes().count();
    let _ = app.update(command_key("d"));
    assert_eq!(app.controller.graph().nodes().count(), before + 2);
    assert_eq!(step_name(&app).as_deref(), Some("duplicate 2 modules"));
    let first = *app.selection.iter().next().unwrap();
    assert_ne!(first, sine.0);
    assert_eq!(app.positions[&first], Point::new(24.0, 24.0));
    app.undo();
    assert_eq!(app.controller.graph().nodes().count(), before);
}

#[test]
fn cut_copies_and_deletes_as_one_step() {
    let mut app = app();
    let (sine, vca) = two_selected(&mut app);
    let before = app.controller.graph().nodes().count();
    let text = app.cut().unwrap();
    assert!(text.contains("loom.osc.sine"));
    assert_eq!(app.controller.graph().nodes().count(), before - 2);
    assert_eq!(step_name(&app).as_deref(), Some("cut 2 modules"));
    assert_eq!(app.edits.history.len(), 1);
    app.undo();
    assert!(app.controller.graph().node(sine).is_some() && app.controller.graph().node(vca).is_some());
    assert_eq!(app.controller.graph().cables().count(), 1);
}

#[test]
fn a_refused_paste_adds_nothing_and_no_step() {
    let mut app = app();
    let before = app.controller.graph().nodes().count();
    app.paste(Point::ORIGIN, None);
    assert_eq!(app.alerts.status(), clipboard::NOTHING_TO_PASTE);
    app.paste(Point::ORIGIN, Some("not a patch".to_string()));
    assert_eq!(app.alerts.status(), clipboard::NOTHING_TO_PASTE);
    app.paste(
        Point::ORIGIN,
        Some("Patch(version: 1, modules: [(id: 1, kind: \"loom.none.such\")])".to_string()),
    );
    assert!(app.alerts.status().contains("invalid") && app.alerts.status().contains("loom.none.such"));
    assert_eq!(app.controller.graph().nodes().count(), before);
    assert!(app.edits.history.undo_step().is_none());
}

#[test]
fn clipboard_keys_go_to_the_browser_while_it_is_open() {
    let mut app = app();
    two_selected(&mut app);
    let before = app.controller.graph().nodes().count();
    open_at(&mut app, Point::ORIGIN);
    let _ = app.update(command_key("d"));
    let _ = app.update(command_key("x"));
    assert_eq!(app.controller.graph().nodes().count(), before);
}

/// A command's step name never labels a later edit, even when the
/// command made no step.
#[test]
fn a_step_name_does_not_outlive_its_edit() {
    let mut app = app();
    let (_, vca) = two_selected(&mut app);
    app.edits.set_history_off(true);
    app.step_name = Some("cut 2 modules".to_string());
    app.edit(|_| {});
    assert!(app.step_name.is_none());
    app.edits.set_history_off(false);
    app.edit(|app| {
        app.controller.set_param_named(vca, "gain", 0.9).unwrap();
        app.commit();
    });
    assert_eq!(step_name(&app).as_deref(), Some("change Multiply Gain"));
}

/// A clipboard read that finishes after another patch opened is dropped.
#[test]
fn a_paste_for_a_closed_patch_is_dropped() {
    let mut app = app();
    two_selected(&mut app);
    let text = app.copy().unwrap();
    let before = app.controller.graph().nodes().count();
    let _ = app.update(Message::Pasted {
        world: Point::ORIGIN,
        text: Some(text.clone()),
        generation: app.generation + 1,
    });
    assert_eq!(app.controller.graph().nodes().count(), before);
    let _ = app.update(Message::Pasted {
        world: Point::ORIGIN,
        text: Some(text),
        generation: app.generation,
    });
    assert_eq!(app.controller.graph().nodes().count(), before + 2);
}

/// Panic is not an edit: the value, the cables, and the unsaved mark
/// stay, and there is no new undo step (panic.md, Acceptance criteria).
#[test]
fn panic_is_not_an_edit() {
    let mut app = app();
    let (_, vca) = two_selected(&mut app);
    app.edit(|app| {
        app.controller.set_param_named(vca, "gain", 0.9).unwrap();
        app.commit();
    });
    assert!(app.unsaved());
    let steps = app.edits.history.len();
    let cables = app.controller.graph().cables().count();
    app.holding = true;
    let _ = app.update(Message::Panic);
    // The MIDI poll runs while the app has an engine, and finds nothing.
    let _ = app.subscription();
    let _ = app.update(Message::MidiPoll);
    assert!(app.alerts.status().starts_with("Panic"), "{}", app.alerts.status());
    assert!(!app.holding, "Hold note lets go");
    let gain = app
        .controller
        .graph()
        .node(vca)
        .unwrap()
        .descriptor
        .param_index("gain")
        .unwrap();
    assert_eq!(app.controller.graph().node(vca).unwrap().params[gain], 0.9);
    assert_eq!(app.controller.graph().cables().count(), cables);
    assert!(app.unsaved(), "the unsaved mark stays");
    assert_eq!(app.edits.history.len(), steps, "no undo step");
}

/// The panic key works with a text field focused, the toolbar button
/// runs it, and so do the control channel's `panic` and `press`.
#[test]
fn the_panic_key_button_and_command_run_panic() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    let _ = ui.app.update(Message::OpenBrowser);
    ui.redraw();
    ui.typewrite("sin");
    let messages = ui.tap(
        iced::keyboard::Key::Character(".".into()),
        iced::keyboard::Modifiers::COMMAND,
    );
    assert!(messages.iter().any(|m| matches!(m, Message::Panic)), "{messages:?}");
    assert!(
        ui.app.alerts.status().starts_with("Panic"),
        "{}",
        ui.app.alerts.status()
    );

    ui.app.alerts.say(String::new());
    let _ = ui.app.update(Message::Browser(browser::Message::Close));
    ui.redraw();
    let button = ui.text("Panic").expect("a Panic button on the toolbar");
    ui.move_to(button.center());
    let mut messages = ui.press();
    messages.extend(ui.release());
    assert!(messages.iter().any(|m| matches!(m, Message::Panic)), "{messages:?}");
}

/// A momentary press sets the transient hold: the tile shows it, but
/// the patch is unchanged, the saved file has no `hold`, the inspector
/// has no row for it, and a copy pastes a released button
/// (transient-params.md, Acceptance criteria).
#[test]
fn a_momentary_hold_is_not_part_of_the_patch() {
    let mut app = app();
    let id = app.controller.add_module("loom.ui.button").unwrap();
    app.controller.set_param_named(id, "mode", 1.0).unwrap();
    app.positions.insert(id.0, Point::new(0.0, 0.0));
    app.commit();
    app.edits.history.clear();
    app.edits.history.mark_saved();
    let hold = app
        .controller
        .graph()
        .node(id)
        .unwrap()
        .descriptor
        .param_index("hold")
        .unwrap();
    let held = |app: &App| app.controller.graph().node(id).unwrap().params[hold];

    let _ = app.update(Message::Canvas(CanvasEvent::ButtonPressed {
        module: id.0,
        widget: 0,
    }));
    assert_eq!(held(&app), 1.0);
    assert_eq!(tile_widget(&app, id, 0).position, 1.0, "a held button looks held");
    assert!(!app.unsaved(), "a hold does not mark the patch changed");
    assert_eq!(app.edits.history.len(), 0, "a hold is no undo step");
    let text = app.current_patch().to_ron().unwrap();
    assert!(!text.contains("hold"), "{text}");
    app.arm((id.0, hold));
    assert!(app.learn.armed().is_none(), "a hold cannot be a MIDI learn target");

    app.selection = BTreeSet::from([id.0]);
    let snapshot = app.ui_snapshot();
    let rows = serde_json::to_string(&snapshot.inspector).unwrap();
    assert!(!rows.contains("\"Hold\""), "{rows}");
    let copied = app.copy().unwrap();
    let before: BTreeSet<u32> = app.controller.graph().nodes().map(|(m, _)| m.0).collect();
    let _ = app.update(Message::Pasted {
        world: Point::new(400.0, 0.0),
        text: Some(copied),
        generation: app.generation,
    });
    let (pasted, node) = app
        .controller
        .graph()
        .nodes()
        .find(|(m, _)| !before.contains(&m.0))
        .expect("a pasted button");
    assert_ne!(pasted, id);
    assert_eq!(node.params[hold], 0.0, "a pasted button is released");
    assert_eq!(held(&app), 1.0, "the original is still held");

    let _ = app.update(Message::Canvas(CanvasEvent::ButtonReleased {
        module: id.0,
        widget: 0,
    }));
    assert_eq!(held(&app), 0.0);
    assert_eq!(tile_widget(&app, id, 0).position, 0.0);
}
/// The widget on module `id`'s tile at `widget`, as the snapshot shows it.
fn tile_widget(app: &App, id: ModuleId, widget: usize) -> snapshot::Widget {
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == id.0).expect("a tile");
    tile.widgets[widget].clone()
}

/// A modulated knob shows no live value until the engine runs, then
/// follows it through the app's own `LiveFrame` path.
#[test]
fn a_running_engine_moves_the_live_dot() {
    let mut app = app();
    let source = app.controller.add_module("loom.math.constant").unwrap();
    app.controller.set_param_named(source, "value", 0.5).unwrap();
    let (_, filter, cutoff, widget) = env_and_filter(&mut app);
    let _ = app.update(Message::Assign {
        source: out(source),
        module: filter.0,
        index: cutoff,
    });
    let _ = app.update(Message::LiveFrame);
    assert_eq!(tile_widget(&app, filter, widget).live, None, "the engine has not run");

    app.process(64);
    let _ = app.update(Message::LiveFrame);
    let knob = tile_widget(&app, filter, widget);
    let live = knob.live.expect("a live value");
    assert!(live > knob.position, "a positive source raises the cutoff: {knob:?}");
    let snapshot::Mark::Range { low, high } = knob.modulation else {
        panic!("an arc: {knob:?}");
    };
    assert!((low..=high).contains(&live), "the dot is on the arc: {knob:?}");
}

/// A light on screen follows the engine: the envelope's level light is
/// dark at rest and lights up while its gate is held.
#[test]
fn a_running_engine_lights_the_envelope() {
    let mut app = app();
    let gate = app.controller.add_module("loom.math.constant").unwrap();
    app.controller.set_param_named(gate, "value", 1.0).unwrap();
    let env = app.controller.add_module("loom.env.envelope").unwrap();
    app.commit();
    let d = app.controller.graph().node(env).unwrap().descriptor;
    let light = d.panel.widgets.iter().position(|w| w.bind == "level").unwrap();
    let _ = app.update(Message::Canvas(CanvasEvent::VisibleDisplays(vec![env.0])));
    let watch = app.watched_lights().expect("the level light is on screen");

    app.process(480);
    let _ = app.update(Message::Lights(watch.read()));
    assert_eq!(tile_widget(&app, env, light).position, 0.0, "no gate, no level");

    app.controller.connect_named(gate, "out", env, "gate").unwrap();
    app.commit();
    app.process(4_800);
    // A level light draws its value as it is, with no fade.
    let mut fades = vec![lights::Fade::default(); watch.lights.len()];
    let _ = app.update(Message::Lights(watch.shown(&mut fades, Instant::now())));
    let level = tile_widget(&app, env, light).position;
    assert!(level > 0.5, "the gate is held: {level}");
}

/// Saved tile dimensions survive session loading and patch creation.
#[test]
fn session_keeps_saved_tile_dimensions() {
    let mut patch = Patch::new("Sized");
    patch.modules.push(loom_patch::PatchModule {
        id: 7,
        kind: "loom.display.scope@1".into(),
        label: None,
        params: Default::default(),
        position: None,
        size: Some((24, 16)),
        ranges: Default::default(),
    });
    let session = App::session(EngineConfig::default(), false, Source::Recovered(Box::new(patch))).unwrap();
    assert_eq!(session.sizes.get(&7), Some(&(24, 16)));
    let scene = scene::build(&session.controller, &session.positions, &session.sizes);
    let tile = scene.module(7).unwrap();
    assert_eq!(
        (tile.width(), tile.height()),
        (24.0 * loom_canvas::GRID, 16.0 * loom_canvas::GRID)
    );
    let mut app = app();
    let id = app.controller.add_module("loom.display.scope").unwrap();
    app.sizes.insert(id.0, (24, 16));
    let saved = app.current_patch();
    assert_eq!(
        saved.modules.iter().find(|m| m.id == id.0).unwrap().size,
        Some((24, 16))
    );
}

/// Unknown tiles remain read-only and survive delete followed by undo.
#[test]
fn unknown_module_rejects_new_connections_and_undo_restores_it() {
    let mut app = app();
    let phase = app.controller.add_module("loom.phase.generator").unwrap();
    let missing = ModuleId(100);
    app.controller
        .add_unknown_with_id(
            missing,
            loom_core::UnknownModule {
                kind: "future.effect@1".into(),
                label: Some("Missing".into()),
                params: BTreeMap::from([("amount".into(), 0.5)]),
                ranges: BTreeMap::new(),
                inputs: vec!["in".into()],
                outputs: vec!["out".into()],
                parameters: vec!["amount".into()],
            },
        )
        .unwrap();
    app.controller.connect_named(phase, "phase", missing, "in").unwrap();
    let assignment = app
        .controller
        .add_assignment_named(phase, "phase", missing, "amount", 0.5)
        .unwrap();
    app.controller
        .map_midi_named(
            loom_core::MidiControl {
                channel: 0,
                controller: 74,
            },
            missing,
            "amount",
        )
        .unwrap();
    app.commit();
    app.edits.history.clear();
    app.selection = BTreeSet::from([missing.0]);
    let inspector::Inspector::Unknown(rows) = app.ui_snapshot().inspector else {
        panic!("unknown inspector");
    };
    assert_eq!(rows.params, vec![("amount".into(), 0.5)]);
    assert_eq!(rows.links.len(), 2);
    let cables = app.controller.graph().cables().count();
    app.canvas_event(CanvasEvent::Connect {
        from: loom_canvas::PortRef {
            module: missing.0,
            index: 0,
            output: true,
        },
        to: loom_canvas::PortRef {
            module: phase.0,
            index: 0,
            output: false,
        },
    });
    assert_eq!(app.alerts.status(), "Cannot connect to an unknown module");
    assert_eq!(app.controller.graph().cables().count(), cables);
    assert!(app.edits.history.undo_step().is_none());
    app.canvas_event(CanvasEvent::Delete(vec![missing.0]));
    assert!(app.controller.graph().node(missing).is_none());
    app.undo();
    let restored = app.controller.graph().node(missing).unwrap().unknown.clone().unwrap();
    assert_eq!(restored.params.get("amount"), Some(&0.5));
    assert_eq!(app.controller.graph().cables().count(), cables);
    app.selection = BTreeSet::from([missing.0]);
    app.edits.history.clear();
    let copied = app.copy().unwrap();
    app.paste(Point::new(400.0, 200.0), Some(copied));
    let pasted = ModuleId(*app.selection.iter().next().unwrap());
    assert_ne!(pasted, missing);
    assert_eq!(
        app.controller.graph().node(pasted).unwrap().unknown.as_ref().unwrap(),
        &restored
    );
    app.undo();
    assert!(app.controller.graph().node(pasted).is_none());
    app.redo();
    assert!(app.controller.graph().node(pasted).unwrap().unknown.is_some());
    app.selection = BTreeSet::from([missing.0]);
    app.duplicate();
    let duplicate = ModuleId(*app.selection.iter().next().unwrap());
    assert_ne!(duplicate, missing);
    assert!(app.controller.graph().node(duplicate).unwrap().unknown.is_some());
    app.undo();
    assert!(app.controller.graph().node(duplicate).is_none());
    let _ = app.update(Message::Unassign(assignment));
    let _ = app.update(Message::Unmap {
        module: missing.0,
        index: 0,
    });
    assert!(app.controller.graph().assignment(assignment).is_none());
    assert!(app.controller.graph().midi_mapping_of(missing, 0).is_none());
    assert_eq!(
        app.controller
            .graph()
            .node(missing)
            .unwrap()
            .unknown
            .as_ref()
            .unwrap()
            .outputs,
        ["out"]
    );
}

/// The inspector reports the nominal range alongside each port description.
#[test]
fn inspector_shows_nominal_port_ranges() {
    let mut app = app();
    let env = app.controller.add_module("loom.env.envelope").unwrap();
    app.commit();
    let _ = app.update(Message::SelectModule(env.0));
    let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
        panic!("one module is selected");
    };
    assert!(
        rows.ports
            .iter()
            .any(|p| p.direction == "Output" && p.label == "Out" && p.description.contains("Usually 0 to 1."))
    );
    assert!(
        rows.ports
            .iter()
            .any(|p| p.direction == "Input" && p.label == "Gate" && p.description.contains("Usually 0 to 1."))
    );
}

/// Button's mode shows its choice names, "Toggle" and "Momentary", as
/// radio buttons, and a parameter without names keeps its slider. No
/// module is special-cased: the names come from the descriptor
/// (panels.md, Choice names). A pick is one undo step.
#[test]
fn the_inspector_names_the_choices_of_a_whole_number_parameter() {
    let mut app = app();
    let button = app.controller.add_module("loom.ui.button").unwrap();
    let lfo = app.controller.add_module("loom.mod.lfo").unwrap();
    app.commit();
    let rows = |app: &App, module: ModuleId| {
        let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
            panic!("one module is selected");
        };
        assert_eq!(rows.id, module.0);
        rows.params
    };
    let control = |row: &inspector::ParamRow| match &row.mode {
        inspector::ParamMode::Edit { control, .. } => control.clone(),
        inspector::ParamMode::Learn { .. } => panic!("not in learn mode"),
    };
    let _ = app.update(Message::SelectModule(button.0));
    let params = rows(&app, button);
    let mode = params.iter().find(|p| p.label == "Mode").unwrap();
    assert_eq!(
        control(mode),
        inspector::ParamControl::Radio {
            choices: vec!["Toggle", "Momentary"],
            selected: 0
        }
    );
    assert_eq!(mode.value, "Toggle", "the value is the name, never the bare number");
    let on = params.iter().find(|p| p.label != "Mode").unwrap();
    assert_eq!(control(on), inspector::ParamControl::Slider);

    let undo = app.edits.history.undo_step().map(|s| s.name().to_string());
    let _ = app.update(Message::Choice {
        module: button.0,
        index: mode.index,
        choice: 1,
    });
    assert_eq!(app.controller.graph().node(button).unwrap().params[mode.index], 1.0);
    let mode = rows(&app, button).into_iter().find(|p| p.label == "Mode").unwrap();
    assert_eq!(mode.value, "Momentary");
    assert_ne!(
        app.edits.history.undo_step().map(|s| s.name().to_string()),
        undo,
        "one step"
    );
    let _ = app.update(Message::Undo);
    assert_eq!(app.controller.graph().node(button).unwrap().params[mode.index], 0.0);

    // More than four names: a pick list.
    let _ = app.update(Message::SelectModule(lfo.0));
    let shape = rows(&app, lfo).into_iter().find(|p| p.label == "Shape").unwrap();
    let inspector::ParamControl::PickList { choices, selected } = control(&shape) else {
        panic!("five shapes are a pick list: {shape:?}");
    };
    assert_eq!(choices.len(), 5);
    assert_eq!(selected, 0);
}

/// A saved range that does not hold its value loads with the smallest
/// range that does, the value unchanged, and a note in the status bar
/// (control-ranges.md, rule 7).
#[test]
fn a_range_note_shows_in_the_status_bar() {
    let dir = std::env::temp_dir().join(format!("loom-app-range-note-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("range.ron");
    std::fs::write(
            &path,
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 5.0}, ranges: {"gain": (-0.1, 0.1)})])"#,
        )
        .unwrap();
    let app = App::new(Args {
        patch: Some(path),
        ..app_args()
    });
    let _ = std::fs::remove_dir_all(&dir);
    let node = app.controller.graph().node(ModuleId(1)).unwrap();
    assert_eq!(
        (node.params[0], node.range(0)),
        (5.0, Some(loom_core::ParamRange::new(-5.0, 5.0)))
    );
    assert!(app.alerts.status().contains("-0.1 to 0.1"), "{}", app.alerts.status());
}

/// Opening a patch that loads as saved says only "opened <path>". One
/// with a load note says the note after it.
#[test]
fn opening_a_patch_names_only_its_load_notes() {
    let mut app = app();
    let clean = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
    app.open_path(&clean);
    assert_eq!(app.alerts.status(), format!("opened {}", clean.display()));

    let dir = std::env::temp_dir().join(format!("loom-app-open-note-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let noted = dir.join("noted.ron");
    std::fs::write(
            &noted,
            r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 5.0}, ranges: {"gain": (-0.1, 0.1)})])"#,
        )
        .unwrap();
    app.open_path(&noted);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        app.alerts
            .status()
            .starts_with(&format!("opened {}: module 1", noted.display())),
        "{}",
        app.alerts.status()
    );

    app.open_path(&clean);
    assert_eq!(
        app.alerts.status(),
        format!("opened {}", clean.display()),
        "the old note is gone"
    );
}

/// control-ranges.md: Multiply's gain shows a **Range** selector. A
/// smaller range clamps the value, as one undo step, and the tile knob
/// then spans the selected range. Undo brings back range and value.
#[test]
fn the_inspector_selects_a_control_range() {
    let mut app = app();
    let mul = app.controller.add_module("loom.math.mul").unwrap();
    app.controller.set_param(mul, 0, 0.5).unwrap();
    app.commit();
    let _ = app.update(Message::SelectModule(mul.0));
    let gain_row = |app: &App| {
        let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
            panic!("one module is selected");
        };
        rows.params.into_iter().find(|p| p.label == "Gain").unwrap()
    };
    let range = gain_row(&app).range.expect("a Range selector");
    assert_eq!(range.choices.len(), 7);
    assert_eq!(range.choices[0], "-0.1 to 0.1");
    assert_eq!(range.choices[range.selected.unwrap()], "-1 to 1");

    let _ = app.update(Message::Range {
        module: mul.0,
        index: 0,
        range: 0,
    });
    let row = gain_row(&app);
    assert_eq!(row.range.unwrap().selected, Some(0));
    assert_eq!(app.controller.graph().node(mul).unwrap().params[0], 0.1, "clamped");
    assert_eq!(
        app.edits.history.undo_step().map(|s| s.name().to_string()).as_deref(),
        Some("change Multiply Gain range")
    );
    // The tile knob and its keyboard steps span -0.1 to 0.1.
    let widget = app
        .controller
        .graph()
        .node(mul)
        .unwrap()
        .descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.bind == "gain")
        .unwrap();
    let (_, _, spec) = scene::control_param(&app.controller, mul.0, widget).unwrap();
    assert_eq!((spec.min, spec.max), (-0.1, 0.1));
    assert!(
        (spec.denormalize(0.51) - spec.denormalize(0.5) - 0.002).abs() < 1e-6,
        "1% of the range"
    );

    let _ = app.update(Message::Undo);
    let node = app.controller.graph().node(mul).unwrap();
    assert_eq!(
        (node.params[0], node.range(0)),
        (0.5, Some(loom_core::ParamRange::new(-1.0, 1.0)))
    );
    // A parameter without ranges has no selector.
    let lfo = app.controller.add_module("loom.mod.lfo").unwrap();
    app.commit();
    let _ = app.update(Message::SelectModule(lfo.0));
    let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
        panic!("one module is selected");
    };
    assert!(rows.params.iter().all(|p| p.range.is_none()));
}

/// A one-sample trigger lights Clock Divide's trigger light, though the
/// output went low again long before the light is read. The light fades
/// out in 120 ms, and a second read without a trigger is dark
/// (panels.md, Lights for one-sample triggers).
#[test]
fn a_one_sample_trigger_lights_the_divide_light() {
    let mut app = app();
    let gate = app.controller.add_module("loom.math.constant").unwrap();
    app.controller.set_param_named(gate, "value", 1.0).unwrap();
    let divide = app.controller.add_module("loom.logic.divide").unwrap();
    app.controller.set_param_named(divide, "n", 1.0).unwrap();
    app.controller.connect_named(gate, "out", divide, "trigger").unwrap();
    app.commit();
    let d = app.controller.graph().node(divide).unwrap().descriptor;
    let light = d.panel.widgets.iter().position(|w| w.bind == "fired").unwrap();
    let _ = app.update(Message::Canvas(CanvasEvent::VisibleDisplays(vec![divide.0])));
    let watch = app.watched_lights().expect("the trigger light is on screen");

    // The constant's rise is one trigger in the first frame, then 4 799
    // frames of 0.
    app.process(4_800);
    let start = Instant::now();
    let mut fades = vec![lights::Fade::default(); watch.lights.len()];
    let _ = app.update(Message::Lights(watch.shown(&mut fades, start)));
    assert_eq!(tile_widget(&app, divide, light).position, 1.0, "the trigger lit it");

    app.process(4_800);
    let later = start + lights::FADE;
    let _ = app.update(Message::Lights(watch.shown(&mut fades, later)));
    assert_eq!(
        tile_widget(&app, divide, light).position,
        0.0,
        "faded, and no new trigger"
    );
}

/// A meter on screen follows the engine through the app's `Meters`
/// path: empty before the engine runs, then a sine's level, with its
/// peak above the RMS level (#222).
#[test]
fn a_running_engine_fills_the_meter() {
    let mut app = app();
    let phase = app.controller.add_module("loom.phase.generator").unwrap();
    let sine = app.controller.add_module("loom.osc.sine").unwrap();
    let meter = app.controller.add_module("loom.display.meter").unwrap();
    app.controller.connect_named(phase, "phase", sine, "phase").unwrap();
    app.controller.connect_named(sine, "out", meter, "in").unwrap();
    app.commit();
    let d = app.controller.graph().node(meter).unwrap().descriptor;
    let widget = d.panel.widgets.iter().position(|w| w.bind == "out").unwrap();
    // On screen, so the meter gets a probe.
    let _ = app.update(Message::Canvas(CanvasEvent::VisibleDisplays(vec![meter.0])));
    let _ = app.update(Message::Meters(app.monitor.read()));
    let before = tile_widget(&app, meter, widget);
    assert_eq!((before.position, before.peak), (0.0, 0.0), "the engine has not run");

    app.process(4_800);
    let _ = app.update(Message::Meters(app.monitor.read()));
    let shown = tile_widget(&app, meter, widget);
    assert_eq!(shown.kind, "meter");
    assert!(shown.position > 0.0, "a level: {shown:?}");
    assert!(shown.peak > shown.position, "a sine peaks above its RMS: {shown:?}");
    assert!(shown.peak > 0.99, "a full-scale sine: {shown:?}");
}

/// A MIDI mapping applied in the engine shows on the knob: a control
/// change sent through the engine's MIDI input moves the mapped cutoff
/// to the end of its range once the app reads it with `Midi` (#222).
#[test]
fn a_mapped_control_change_turns_the_knob() {
    let mut app = app();
    let (_, filter, cutoff, widget) = env_and_filter(&mut app);
    let control = loom_core::MidiControl {
        channel: 0,
        controller: 74,
    };
    app.controller.map_midi(control, filter, cutoff).unwrap();
    app.commit();
    let midi = app.rig.midi_in.clone().expect("the idle engine's MIDI input");
    let start = tile_widget(&app, filter, widget).position;
    assert!(start > 0.0 && start < 1.0, "{start}");

    for (value, position) in [(127, 1.0), (0, 0.0)] {
        let change = loom_core::MidiEvent::from_bytes(0, &[0xB0, 74, value]);
        assert!(loom_host_desktop::send_midi(&midi, change));
        app.process(128);
        let _ = app.update(Message::Midi);
        let knob = tile_widget(&app, filter, widget);
        assert_eq!(knob.position, position, "CC 74 at {value}: {knob:?}");
        let param = app.controller.graph().node(filter).unwrap().params[cutoff];
        let spec = &app.controller.graph().node(filter).unwrap().descriptor.params[cutoff];
        assert_eq!(param, if value == 0 { spec.min } else { spec.max });
    }
}

/// The transport moves only while it runs. Its position shows through a
/// clock.md, Starting and stopping: a cable on the Transport's `run`
/// input decides, and the Run toggle shows what it sets. Space changes
/// nothing then. When the cable goes, the transport stays as it was, the
/// toggle takes that state, and Space works again. A second Transport
/// cannot be added, and the one there cannot be deleted.
#[test]
fn the_run_toggle_follows_a_cable_on_run() {
    let mut app = app();
    let (transport, run) = transport_run(&app.controller).expect("a new patch has a Transport");
    let run_param = |app: &App| app.controller.graph().node(transport).unwrap().params[run];
    // A Button's gate is low until pressed: it stops the transport.
    let button = app.controller.add_module("loom.ui.button").unwrap();
    app.controller.connect_named(button, "gate", transport, "run").unwrap();
    app.commit();
    app.process(2048);
    app.refresh_values();
    assert_eq!(transport_running(&app.controller), Some(false));
    assert_eq!(
        tile_widget(&app, transport, 0).position,
        0.0,
        "the toggle shows the input"
    );
    assert_eq!(run_param(&app), 1.0, "Run itself is unchanged");
    let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
    assert_eq!(run_param(&app), 1.0, "Space leaves a driven Run alone");

    let _ = app.update(Message::Canvas(CanvasEvent::Disconnect(loom_canvas::PortRef {
        module: transport.0,
        output: false,
        index: 0,
    })));
    assert_eq!(run_param(&app), 0.0, "the toggle takes the state the input left");
    app.process(1024);
    assert_eq!(transport_running(&app.controller), Some(false), "still stopped");
    let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
    app.process(1024);
    assert_eq!(transport_running(&app.controller), Some(true), "Space works again");

    // Delete leaves the Transport, and a second one is refused.
    let _ = app.update(Message::Canvas(CanvasEvent::Delete(vec![transport.0, button.0])));
    assert_eq!(app.controller.transport_module(), Some(transport));
    assert!(app.controller.graph().node(button).is_none());
    assert!(app.controller.add_module(TRANSPORT_MODULE).is_err());
}

/// A patch that loads with a cable on `run`: removing the cable as the
/// first edit still gives the Run toggle the state the input left
/// (#413 review).
#[test]
fn a_loaded_run_cable_hands_its_state_to_the_toggle() {
    let dir = std::env::temp_dir().join(format!("loom-app-run-cable-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("run.ron");
    std::fs::write(
        &path,
        r#"Patch(version: 1, modules: [
                PatchModule(id: 1, kind: "loom.clock.transport@1"),
                PatchModule(id: 2, kind: "loom.ui.button@1"),
            ], cables: [PatchCable(from: (2, "gate"), to: (1, "run"))])"#,
    )
    .unwrap();
    let mut app = App::new(Args {
        patch: Some(path),
        ..app_args()
    });
    let _ = std::fs::remove_dir_all(&dir);
    let (transport, run) = transport_run(&app.controller).expect("the patch has a Transport");
    app.process(2048);
    assert_eq!(transport_running(&app.controller), Some(false));
    let _ = app.update(Message::Canvas(CanvasEvent::Disconnect(loom_canvas::PortRef {
        module: transport.0,
        output: false,
        index: 0,
    })));
    let param = app.controller.graph().node(transport).unwrap().params[run];
    assert_eq!(param, 0.0, "the toggle takes the state the input left");
}

/// Phase Scale that follows the bar phase and modulates a knob at depth
/// 1: the live dot sits the bar phase above the knob (#222). The
/// controller reports whether the transport runs, not its position.
#[test]
fn the_transport_position_moves_only_while_running() {
    let mut app = app();
    let scale = app.controller.add_module("loom.phase.scale").unwrap();
    let (_, filter, cutoff, widget) = env_and_filter(&mut app);
    app.controller.add_assignment(out(scale), filter, cutoff, 1.0).unwrap();
    app.commit();
    let settings = app.controller.transport();
    let beats_per_frame = settings.tempo / 60.0 / f64::from(app.stream.sample_rate());
    let beats_per_bar = settings.time_signature.beats_per_bar();
    // The bar phase the knob shows, from the live value of the last
    // frame the engine ran.
    let bar_phase = |app: &mut App, frames: usize| {
        app.process(frames);
        let _ = app.update(Message::LiveFrame);
        let knob = tile_widget(app, filter, widget);
        knob.live.expect("a live value") - knob.position
    };
    let expected = |frames: usize| ((frames - 1) as f64 * beats_per_frame / beats_per_bar) as f32;

    // The bar phase is read as live minus knob position, which holds
    // only while the knob plus the phase stays below 1 (no clamp). The
    // test reaches a bar phase of about 0.05.
    let base = tile_widget(&app, filter, widget).position;
    assert!(base < 0.9, "the knob at {base} leaves no room above it for the phase");
    assert!(settings.running, "a new patch starts running");
    // Space flips the Transport's Run toggle.
    let run_on = |app: &App| {
        let (id, index) = transport_run(&app.controller).expect("a new patch has a Transport");
        app.controller.graph().node(id).unwrap().params[index] >= 0.5
    };
    assert!(run_on(&app));
    let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
    assert!(!run_on(&app));
    assert!(bar_phase(&mut app, 4_800).abs() < 1e-6, "stopped at bar 1");

    let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
    assert!(run_on(&app));
    let first = bar_phase(&mut app, 4_800);
    assert!((first - expected(4_800)).abs() < 1e-4, "{first} vs {}", expected(4_800));
    let second = bar_phase(&mut app, 4_800);
    assert!(
        (second - expected(9_600)).abs() < 1e-4,
        "{second} vs {}",
        expected(9_600)
    );

    let _ = app.update(Message::Canvas(CanvasEvent::ToggleRun));
    let held = bar_phase(&mut app, 4_800);
    assert!(
        (held - expected(9_600)).abs() < 1e-4,
        "stopping holds the position: {held}"
    );
}

/// Meter levels follow their probes: turning metering on and off many
/// times leaves at most one level per live probe, and none once the
/// cable is gone.
#[test]
fn removed_probes_lose_their_levels() {
    let mut app = app();
    let gate = app.controller.add_module("loom.math.constant").unwrap();
    let env = app.controller.add_module("loom.env.envelope").unwrap();
    app.controller.connect_named(gate, "out", env, "gate").unwrap();
    app.commit();
    for _ in 0..20 {
        app.live.set_activity(true);
        app.sync_probes();
        app.process(64);
        let probe = app.live.probes().next().expect("a probe on the cable");
        let level = live::Level {
            rms: 0.5,
            peak: 0.6,
            right: None,
        };
        let _ = app.update(Message::Meters(vec![(probe, level)]));
        assert_eq!(app.meter_levels.len(), 1);

        app.live.set_activity(false);
        app.sync_probes();
        assert!(app.meter_levels.is_empty(), "{:?}", app.meter_levels);
        // A late reading for the removed probe is ignored.
        let _ = app.update(Message::Meters(vec![(probe, level)]));
        assert!(app.meter_levels.is_empty());
    }
}

/// The inspector lists a parameter's assignments in order, with the one
/// that is off shown off. **Add modulation** greys out the outputs that
/// already drive it, and the source's **Drives** line says "(off)".
#[test]
fn the_snapshot_shows_the_inspector_rows() {
    let mut app = app();
    let (env, sine, filter, cutoff, _) = two_sources(&mut app);
    let off = app
        .controller
        .graph()
        .assignments()
        .find(|(_, a)| a.source == out(sine))
        .unwrap()
        .0;
    let _ = app.update(Message::Enabled {
        id: off,
        enabled: false,
    });
    let _ = app.update(Message::SelectModule(filter.0));
    let _ = app.update(Message::AddModulation {
        module: filter.0,
        index: cutoff,
    });

    let snapshot = app.ui_snapshot();
    assert_eq!(snapshot.undo.as_deref(), Some("change modulation"));
    let inspector::Inspector::Module(rows) = snapshot.inspector else {
        panic!("one module is selected: {:?}", snapshot.inspector);
    };
    assert_eq!(rows.name, "SVF");
    let row = &rows.params[cutoff];
    assert_eq!(row.label, "Cutoff");
    let inspector::ParamMode::Edit {
        assignments, adding, ..
    } = &row.mode
    else {
        panic!("not in learn mode");
    };
    let (env_out, sine_out) = (format!("Envelope #{}: Out", env.0), format!("Sine #{}: Out", sine.0));
    let shown: Vec<_> = assignments
        .iter()
        .map(|a| (a.source.as_str(), a.depth_text.as_str(), a.enabled))
        .collect();
    assert_eq!(shown, vec![(&*env_out, "+25%", true), (&*sine_out, "+25%", false)]);
    let pickable: Vec<_> = adding
        .as_ref()
        .expect("Add modulation is open")
        .iter()
        .map(|s| (s.name.as_str(), s.enabled))
        .collect();
    assert!(pickable.contains(&(&*env_out, false)), "{pickable:?}");
    assert!(pickable.contains(&(&*sine_out, false)), "{pickable:?}");
    assert!(pickable.iter().any(|&(_, enabled)| enabled), "{pickable:?}");
    assert!(rows.drives.is_empty(), "the filter drives nothing");

    let _ = app.update(Message::SelectModule(sine.0));
    let inspector::Inspector::Module(rows) = app.ui_snapshot().inspector else {
        panic!("the sine is selected");
    };
    let line = &rows.drives[0].lines[0];
    assert_eq!((line.target, line.enabled), (filter.0, false));
    assert!(line.text.ends_with("(off)"), "{}", line.text);
}

/// The snapshot's places are the ones the canvas draws and hits: a
/// badge, a port, a widget, and a cable's ends.
#[test]
fn the_snapshot_places_what_the_canvas_draws() {
    use loom_canvas::{Hit, PortRef};
    let mut app = app();
    let (env, sine, filter, _, widget) = two_sources(&mut app);
    app.controller.connect_named(sine, "out", filter, "in").unwrap();
    app.commit();
    let snapshot = app.ui_snapshot();
    let tile = |id: ModuleId| snapshot.tiles.iter().find(|t| t.id == id.0).expect("a tile");
    let view = app.scene.module(env.0).unwrap();

    let output = &tile(env).outputs[0];
    let badge = output.badge_bounds.expect("the envelope drives one assignment");
    let drawn = app.scene.badge_bounds(env.0, 0).unwrap();
    assert_eq!(badge, drawn.into());
    let port = PortRef {
        module: env.0,
        output: true,
        index: 0,
    };
    assert_eq!(
        app.scene.hit(drawn.center(), snapshot.port_hit_radius),
        Hit::Badge(port)
    );
    let centre = Point::new(output.centre.x, output.centre.y);
    assert_eq!(app.scene.hit(centre, snapshot.port_hit_radius), Hit::Port(port));
    assert_eq!(tile(env).bounds, view.bounds().into());
    assert_eq!(tile(sine).outputs[0].badge_bounds.map(|b| b.width), Some(drawn.width));

    let knob = tile(filter).widgets[widget].bounds.expect("a shown knob");
    let filter_view = app.scene.module(filter.0).unwrap();
    assert_eq!(knob, filter_view.widget_bounds(widget).unwrap().into());

    let cable = snapshot
        .cables
        .iter()
        .find(|c| c.to.module == filter.0)
        .expect("a cable");
    assert_eq!((cable.from.module, cable.from.index), (sine.0, 0));
    assert_eq!(cable.from.point, tile(sine).outputs[0].centre);
    assert_eq!(cable.to.point, tile(filter).inputs[cable.to.index].centre);
}

/// An app with one filter, its ID, and the index of its Cutoff.
fn app_with_filter() -> (App, u32, usize) {
    let mut app = app();
    let id = app.controller.add_module("loom.filter.svf").unwrap();
    app.commit();
    let cutoff = app
        .controller
        .graph()
        .node(id)
        .unwrap()
        .descriptor
        .param_index("cutoff")
        .unwrap();
    (app, id.0, cutoff)
}

/// Turns the filter's Cutoff with the inspector slider.
fn slide(app: &mut App, module: u32, index: usize, values: &[f32]) {
    for &value in values {
        let _ = app.update(Message::ParamValue { module, index, value });
    }
}

/// Undo and inspector edits close a released arrow step before the canvas
/// timer does, and its late end cannot close the next inspector gesture.
#[test]
fn pending_arrow_group_yields_to_undo_and_inspector() {
    let (mut app, module, cutoff) = app_with_filter();
    let widget = app
        .controller
        .graph()
        .node(ModuleId(module))
        .unwrap()
        .descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.bind == "cutoff")
        .unwrap();
    let original = app.controller.graph().node(ModuleId(module)).unwrap().params[cutoff];
    let set = |app: &mut App, position| {
        let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
        let _ = app.update(Message::Canvas(CanvasEvent::SetControl {
            module,
            widget,
            position,
        }));
        let _ = app.update(Message::Canvas(CanvasEvent::ArrowGroupPending));
    };
    let value = |app: &App| app.controller.graph().node(ModuleId(module)).unwrap().params[cutoff];

    set(&mut app, 0.6);
    assert!(app.edits.gesture_open());
    let _ = app.update(Message::Undo);
    assert_eq!(value(&app), original, "Undo must act before the 500 ms timer");
    assert!(!app.edits.gesture_open());
    let _ = app.update(Message::Redo);
    assert_ne!(value(&app), original, "Redo must also act before the timer");
    let _ = app.update(Message::Undo);
    let _ = app.update(Message::Canvas(CanvasEvent::ArrowGroupEnd));
    assert!(!app.edits.gesture_open());

    set(&mut app, 0.7);
    let arrow_value = value(&app);
    slide(&mut app, module, cutoff, &[800.0, 900.0]);
    assert!(app.edits.slider_gesture_open());
    let _ = app.update(Message::Canvas(CanvasEvent::ArrowGroupEnd));
    assert!(
        app.edits.slider_gesture_open(),
        "late arrow end must leave the slider open"
    );
    let _ = app.update(Message::SliderReleased);
    let _ = app.update(Message::Undo);
    assert_eq!(value(&app), arrow_value, "slider and arrow are separate steps");
}

/// A quick arrow press after Undo starts a fresh step even though the
/// canvas still considers it part of the previous 500 ms group.
#[test]
fn arrow_resumed_after_undo_starts_a_new_step() {
    let (mut app, module, cutoff) = app_with_filter();
    let widget = app
        .controller
        .graph()
        .node(ModuleId(module))
        .unwrap()
        .descriptor
        .panel
        .widgets
        .iter()
        .position(|w| w.bind == "cutoff")
        .unwrap();
    let original = app.controller.graph().node(ModuleId(module)).unwrap().params[cutoff];
    for event in [
        CanvasEvent::GestureStart,
        CanvasEvent::SetControl {
            module,
            widget,
            position: 0.6,
        },
        CanvasEvent::ArrowGroupPending,
    ] {
        let _ = app.update(Message::Canvas(event));
    }
    let _ = app.update(Message::Undo);
    assert_eq!(
        app.controller.graph().node(ModuleId(module)).unwrap().params[cutoff],
        original
    );
    for event in [
        CanvasEvent::ArrowGroupResumed,
        CanvasEvent::SetControl {
            module,
            widget,
            position: 0.7,
        },
        CanvasEvent::ArrowGroupPending,
        CanvasEvent::ArrowGroupEnd,
    ] {
        let _ = app.update(Message::Canvas(event));
    }
    assert_eq!(app.edits.history.len(), 1, "the undone step was replaced");
    assert!(!app.edits.gesture_open());
    let _ = app.update(Message::Undo);
    assert_eq!(
        app.controller.graph().node(ModuleId(module)).unwrap().params[cutoff],
        original
    );
}

/// An inspector slider gesture is one step, and it closes at release,
/// at the next edit, and at undo: a slider sends no start, and arrow
/// keys on it send no release (undo.md, Gestures). Issue #170.
#[test]
fn an_inspector_slider_gesture_closes_at_release_at_the_next_edit_and_at_undo() {
    let (mut app, filter, cutoff) = app_with_filter();
    let steps = app.edits.history.len();

    // Release.
    slide(&mut app, filter, cutoff, &[500.0, 600.0, 700.0]);
    assert!(app.edits.slider_gesture_open());
    let _ = app.update(Message::SliderReleased);
    assert!(!app.edits.gesture_open());
    assert_eq!(app.edits.history.len(), steps + 1, "one drag, one step");

    // The next edit, here a canvas gesture start.
    slide(&mut app, filter, cutoff, &[800.0, 900.0]);
    let _ = app.update(Message::Canvas(CanvasEvent::GestureStart));
    assert!(!app.edits.slider_gesture_open());
    assert_eq!(app.edits.history.len(), steps + 2, "the start closed the slider's step");
    let _ = app.update(Message::Canvas(CanvasEvent::GestureEnd));
    assert_eq!(
        app.edits.history.len(),
        steps + 2,
        "a gesture with no change is no step"
    );

    // Undo, which takes back the slider's step it closes.
    slide(&mut app, filter, cutoff, &[1000.0]);
    let _ = app.update(Message::Undo);
    assert!(!app.edits.gesture_open());
    let value = app.controller.graph().node(ModuleId(filter)).unwrap().params[cutoff];
    assert_eq!(value, 900.0, "undo took back the slider's last step");
    assert_eq!(app.edits.history.len(), steps + 3);
    assert_eq!(
        app.edits.history.redo_step().map(undo::Step::name),
        Some("change SVF Cutoff")
    );
}

/// A mode change during a momentary press does not leave the hold
/// stuck high: the release lets go of it in any mode (review of #402).
#[test]
fn a_release_clears_the_hold_after_a_mode_change() {
    let mut app = app();
    let id = app.controller.add_module("loom.ui.button").unwrap();
    app.commit();
    let node = app.controller.graph().node(id).unwrap();
    let mode = node.descriptor.param_index("mode").unwrap();
    let hold = node.descriptor.param_index("hold").unwrap();
    let (module, widget) = (id.0, 0);
    let held = |app: &App| app.controller.graph().node(id).unwrap().params[hold];

    app.controller.set_param(id, mode, 1.0).unwrap();
    let _ = app.update(Message::Canvas(CanvasEvent::ButtonPressed { module, widget }));
    assert_eq!(held(&app), 1.0, "the hold is down");
    app.controller.set_param(id, mode, 0.0).unwrap();
    let _ = app.update(Message::Canvas(CanvasEvent::ButtonReleased { module, widget }));
    assert_eq!(held(&app), 0.0, "the release let go of the hold");
    app.controller.set_param(id, mode, 1.0).unwrap();
    assert_eq!(held(&app), 0.0, "Momentary mode comes back released");
}

/// A momentary button hold makes no step, and a toggle press does
/// (undo.md, What is not undoable). A button sends a press and a
/// release, with no gesture around them.
#[test]
fn a_momentary_hold_makes_no_step_and_a_toggle_press_does() {
    let mut app = app();
    let id = app.controller.add_module("loom.ui.button").unwrap();
    app.commit();
    let node = app.controller.graph().node(id).unwrap();
    let mode = node.descriptor.param_index("mode").unwrap();
    let on = node
        .descriptor
        .param_index(node.descriptor.panel.widgets[0].bind)
        .unwrap();
    // Momentary mode writes the transient hold, not the latch (#161).
    let hold = node.descriptor.param_index("hold").unwrap();
    let (module, widget) = (id.0, 0);
    let press = |app: &mut App, param: usize, release: bool| {
        let _ = app.update(Message::Canvas(CanvasEvent::ButtonPressed { module, widget }));
        let held = app.controller.graph().node(id).unwrap().params[param];
        if release {
            let _ = app.update(Message::Canvas(CanvasEvent::ButtonReleased { module, widget }));
        }
        held
    };

    app.controller.set_param(id, mode, 1.0).unwrap();
    app.commit();
    let steps = app.edits.history.len();
    assert_eq!(press(&mut app, hold, false), 1.0, "the hold is down");
    assert_eq!(app.edits.history.len(), steps, "a momentary press is no step");
    let _ = app.update(Message::Canvas(CanvasEvent::ButtonReleased { module, widget }));
    assert_eq!(app.controller.graph().node(id).unwrap().params[hold], 0.0);
    assert_eq!(app.edits.history.len(), steps, "nor is its release");

    app.controller.set_param(id, mode, 0.0).unwrap();
    app.commit();
    let steps = app.edits.history.len();
    assert_eq!(press(&mut app, on, true), 1.0, "the toggle is on");
    assert_eq!(app.edits.history.len(), steps + 1, "a toggle press is a step");
    assert_eq!(app.controller.graph().node(id).unwrap().params[on], 1.0, "and stays on");
}

/// File > New on a saved patch starts an empty one and says so.
#[test]
fn new_says_it_started_an_empty_patch() {
    let (mut app, id, _) = app_with_filter();
    let _ = app.update(Message::File(FileAction::New));
    assert_eq!(app.alerts.status(), "new patch");
    let filters = app
        .controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == "loom.filter.svf");
    assert_eq!(filters.count(), 0, "module {id} went with the old patch");
}

/// With unsaved changes and no dialog program, New keeps the patch and
/// says how to save first.
#[test]
fn new_without_a_dialog_program_keeps_the_patch() {
    let (mut app, id, cutoff) = app_with_filter();
    app.edit(|app| {
        app.controller.set_param(ModuleId(id), cutoff, 0.5).unwrap();
        app.commit();
    });
    assert!(app.unsaved());
    files::fake::answer_discard(files::Choice::Unavailable);
    let _ = app.update(Message::File(FileAction::New));
    assert!(app.controller.graph().node(ModuleId(id)).is_some(), "the patch stays");
    assert!(
        app.alerts.status().starts_with("Unsaved changes."),
        "{}",
        app.alerts.status()
    );
}

/// A help link to a document that is not bundled leaves the help as it is
/// and says so.
#[test]
fn an_unbundled_help_link_says_so() {
    let mut app = app();
    let _ = app.update(Message::HelpLink("nowhere.md".to_string()));
    assert!(app.help.is_none());
    assert_eq!(app.alerts.status(), "Linked document is not bundled: nowhere.md");
}

/// Clicking the armed control again disarms it, and a parameter the
/// module does not have changes nothing but the status line.
#[test]
fn learn_disarms_on_a_second_click_and_bad_params_say_why() {
    let (mut app, id, cutoff) = app_with_filter();
    let _ = app.update(Message::Learn(learn::Message::Toggle));
    let _ = app.update(Message::Learn(learn::Message::Arm((id, cutoff))));
    assert!(app.learn.is_armed((id, cutoff)));
    let _ = app.update(Message::Learn(learn::Message::Arm((id, cutoff))));
    assert!(!app.learn.is_armed((id, cutoff)));
    assert_eq!(app.alerts.status(), "MIDI learn: click a control to map it");

    app.set_param(id, 999, 0.5);
    assert!(app.alerts.status().starts_with("error: "), "{}", app.alerts.status());
}

/// The matrix Add form needs both ends before it adds anything.
#[test]
fn the_matrix_add_form_needs_a_source_and_a_target() {
    let (mut app, _, _) = app_with_filter();
    let _ = app.update(Message::Matrix(matrix::Event::AddOpen));
    let _ = app.update(Message::Matrix(matrix::Event::AddApply));
    assert_eq!(app.alerts.status(), "Choose a source and a target");
    assert!(app.controller.graph().assignments().next().is_none());
}

/// With no free module slot, Duplicate and an Undo that brings a module
/// back change nothing and say why.
#[test]
fn a_full_patch_refuses_duplicate_and_undo() {
    let mut app = app();
    let config = EngineConfig {
        max_modules: 1,
        ..EngineConfig::default()
    };
    let parts = create_engine(registry(true), config);
    app.controller = parts.controller;
    app.stream.keep_idle(parts.engine);
    app.stream.set_running(None);
    let sine = app.controller.add_module("loom.osc.sine").unwrap();
    assert!(app.commit());
    app.selection = BTreeSet::from([sine.0]);
    app.duplicate();
    assert!(
        app.alerts.status().starts_with("Cannot duplicate: "),
        "{}",
        app.alerts.status()
    );
    assert_eq!(app.controller.graph().nodes().count(), 1);

    app.edit(|app| {
        app.controller.remove_module(sine).unwrap();
        app.commit();
    });
    // Something else takes the freed slot.
    app.controller.add_module("loom.osc.saw").unwrap();
    assert!(app.commit());
    let _ = app.update(Message::Undo);
    assert!(
        app.alerts.status().starts_with("Cannot undo "),
        "{}",
        app.alerts.status()
    );
    assert!(app.controller.graph().node(sine).is_none());
}

/// A help example that does not parse leaves the patch and says why.
#[test]
fn a_broken_help_example_says_why() {
    let mut app = app();
    let descriptor = *app.modules.first().unwrap();
    let mut help = help::Help::new(descriptor);
    help.model.example = Some("not a patch".to_string());
    app.help = Some(help);
    let _ = app.update(Message::HelpExample);
    assert!(
        app.alerts.status().starts_with("Could not open example: "),
        "{}",
        app.alerts.status()
    );
}
