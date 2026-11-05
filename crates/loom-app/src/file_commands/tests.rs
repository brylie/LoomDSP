use crate::files::fake::{self, Asked};
use crate::input_owner::{InputOwner, tests::open_overlay};
use crate::matrix::Tab;
use crate::recovery::tests::TempDir;
use crate::session::{EngineLink, PatchSession};
use crate::tests::{app, app_args};
use crate::ui_test::Ui;
use crate::{App, Source, alerts, control, controls, editing, help, live, settings};
use iced::keyboard::{Key, Modifiers};
use loom_core::{OutputRef, OutputSafety, ProbeId};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// New and Open keep what outlives a patch and start the rest again. The
/// pattern names every field with no `..`, so a new field does not compile
/// until someone decides which it is.
#[test]
fn replacing_the_patch_keeps_or_resets_every_field() {
    let mut args = app_args();
    args.bench.measure = true;
    args.bench.bench_seconds = Some(30.0);
    let mut app = App::new(args);
    let report = format!("{:?}", app.reporter);
    let added = app.session.controller.add_module("loom.osc.sine").unwrap();
    app.commit();

    // What goes with the patch.
    app.session.retry_commit = true;
    app.session.positions.insert(added.0, (1.0, 2.0));
    app.session.unknown_positions.insert(99, (None, (0.0, 0.0)));
    app.session.sizes.insert(added.0, (4, 4));
    app.session.mode_key.push((99, 0, 1));
    app.session.gestures.toggle_adding((added.0, 0));
    app.session.value_draft = Some(controls::ValueDraft {
        module: added.0,
        index: 0,
        draft: loom_canvas::TypedDraft::new("1".to_string(), |_| Ok(())),
    });
    app.text_focus = editing::TextFocus { any: true, field: None };
    app.session.lights.insert((added.0, 0), 1.0);
    app.session.run_driven = true;
    app.session.meter_levels.insert(ProbeId(99), live::Level::default());
    app.session.clip_holds.reset((added.0, 0));
    app.session.traces.set(added.0, 0, loom_canvas::Trace::default());
    app.session.carrying.insert(OutputRef { module: added, port: 0 });
    app.session.full_traces = false;
    app.session.visible_displays.push(added.0);
    app.session.selection.insert(added.0);
    app.session.engine.holding = true;
    app.session.patch_info.tag_draft = "draft".to_string();
    app.drops.push("dropped.ron".into());
    let now = Instant::now();
    for frame in 0..20 {
        app.meter.record(now + Duration::from_millis(frame * 10));
    }
    app.session.engine.control_seen = Some((Arc::default(), 1));
    app.session.learn.start();
    app.session.edits.set_arrow_group(crate::edits::ArrowGroup::Released {
        due: Instant::now() + Duration::from_secs(60),
    });
    app.session.matrix.focused = true;
    app.session.step_name = Some("step".to_string());
    // Overlays close, all but the Preferences window.
    app.overlays.help = Some(help::Help::shortcuts());

    // What outlives it.
    app.animating = true;
    app.right_tab = Tab::Matrix;
    app.browser_key = 7;
    app.patches_open = 5;
    app.frame_key = 9;
    app.control = vec![control::Listener::Script("script.jsonl".into())];
    let serial = app.pointer.push(Vec::new());
    app.preferences(settings::Message::Open);
    app.alerts.warn_kept(alerts::Cause::Autosave, "kept".to_string());
    // No input is wanted, so a kept problem with it is over.
    app.alerts.warn_kept(alerts::Cause::AudioInput, "input".to_string());
    app.alerts.warn("said".to_string());

    assert!(app.replace(Source::Empty));

    let App {
        session,
        // `document/tests.rs` and `autosave_tests.rs` cover what it does.
        run_files: _,
        right_tab,
        text_focus,
        generation,
        test_modules,
        // The update that sets it ends by exiting the app, so a replace never
        // starts with it set.
        quit: _,
        // Built the same way every time.
        theme: _,
        // What belongs to the machine. `Rig`'s own tests cover it.
        rig: _,
        alerts,
        modules,
        overlays,
        drops,
        patches_open,
        browser_key,
        frame_key,
        meter,
        reporter,
        control,
        // `control::end_samples` decides what stays. Empty here.
        control_pending,
        pointer,
        measuring,
        animating,
        dev_tools,
        // Set only on the screenshot path, which never replaces the patch.
        still: _,
        // Set and taken within one `update`.
        history_key: _,
    } = &app;
    let PatchSession {
        controller,
        retry_commit,
        positions,
        unknown_positions,
        sizes,
        selection,
        document,
        patch_info,
        edits,
        step_name,
        gestures,
        value_draft,
        learn,
        matrix,
        run_driven,
        scene,
        mode_key,
        // Rebuilt from the new patch, as the scene is.
        values: _,
        lights,
        meter_levels,
        clip_holds,
        traces,
        carrying,
        full_traces,
        visible_displays,
        engine,
    } = session;
    let EngineLink {
        stream,
        // Rebuilt around the new engine's telemetry.
        monitor: _,
        live,
        midi,
        holding,
        control_seen,
    } = engine;

    // A new patch: only its Transport.
    assert_eq!(controller.graph().nodes().count(), 1);
    assert_eq!(scene.modules().len(), 1);
    assert!(!retry_commit);
    assert!(!positions.contains_key(&added.0));
    assert!(unknown_positions.is_empty());
    assert!(sizes.is_empty());
    assert!(!mode_key.contains(&(99, 0, 1)));
    assert_eq!(gestures.adding(), None);
    assert!(value_draft.is_none());
    assert!(!text_focus.any);
    assert!(lights.is_empty());
    assert!(!run_driven);
    assert!(meter_levels.is_empty());
    assert_eq!(clip_holds.resets((added.0, 0)), 0);
    assert!(traces.get(added.0, 0).is_none());
    assert!(carrying.is_empty());
    assert!(*full_traces);
    assert!(visible_displays.is_empty());
    assert!(selection.is_empty());
    assert_eq!(document.file(), None);
    assert_eq!(*generation, 2, "one engine more");
    assert!(stream.playback().is_off(), "this run still plays nothing");
    assert_eq!(live.probes().count(), 0);
    assert!(midi.is_none());
    assert!(!holding);
    assert!(patch_info.tag_draft.is_empty());
    assert!(drops.is_empty());
    assert_eq!(meter.stats().fps, 0.0, "the frame meter starts again");
    assert!(control_seen.is_none());
    assert!(!learn.is_on());
    assert!(edits.history.undo_step().is_none());
    assert_eq!(
        edits.arrow_group(),
        crate::edits::ArrowGroup::Closed,
        "an old arrow group cannot reach the new patch"
    );
    assert!(!matrix.focused);
    assert!(step_name.is_none());
    assert!(control_pending.is_empty());

    // Every overlay closes, but the Preferences window stays.
    assert_eq!(overlays.owner(), InputOwner::Preferences);
    assert!(overlays.help.is_none());

    // What outlives the patch.
    assert!(!test_modules);
    assert!(!modules.is_empty());
    assert_eq!(*right_tab, Tab::Matrix);
    assert_eq!(*patches_open, 5);
    assert_eq!(*browser_key, 7);
    assert_eq!(*frame_key, 10, "a new patch is framed");
    assert_eq!(format!("{reporter:?}"), report, "--bench-seconds still counts");
    assert_eq!(control.len(), 1);
    assert_eq!(pointer.push(Vec::new()), serial + 1);
    assert!(*measuring);
    assert!(*animating);
    assert!(*dev_tools);
    let kept: Vec<_> = alerts.kept().iter().map(|kept| kept.text.as_str()).collect();
    assert_eq!(kept, ["kept"]);
    assert_ne!(alerts.status(), "said");
}

/// `--output-safety` is not saved, and it holds for the whole run: a patch
/// opened later plays with it too.
#[test]
fn the_output_safety_flag_holds_for_a_new_patch() {
    let mut args = app_args();
    args.output_safety = Some(OutputSafety::Clean);
    let mut app = App::new(args);

    assert!(app.replace(Source::Empty));

    assert_eq!(app.session.engine.stream.output_safety(), OutputSafety::Clean);
}

/// Opening a patch closes help, even the help that offered it as an example.
#[test]
fn a_new_patch_closes_help() {
    let mut ui = Ui::open("subtractive.ron");
    open_overlay(&mut ui, InputOwner::Help);

    assert!(ui.app.replace(Source::Empty));

    assert_eq!(ui.app.input_owner(), InputOwner::Canvas);
}

/// Cmd+Shift+O opens Your patches from a menu on the canvas, because a menu
/// does not count as an overlay. One panel is open at a time, so the menu goes.
#[test]
fn your_patches_opens_from_a_menu_and_closes_it() {
    let tmp = TempDir::new("patches-over-menu");
    for owner in [InputOwner::TileMenu, InputOwner::ProbeMenu] {
        let mut ui = Ui::open("subtractive.ron");
        let mut preferences = ui.app.rig.store.saved().clone();
        preferences.library = Some(tmp.0.clone());
        let _ = ui.app.rig.store.save(preferences);
        open_overlay(&mut ui, owner);

        ui.tap(Key::Character("o".into()), Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(ui.app.input_owner(), InputOwner::Patches, "{owner:?}");

        ui.app.overlays.close_patches();
        assert_eq!(ui.app.input_owner(), InputOwner::Canvas, "the menu is gone");
    }
}

/// A patch that does not load shows the reason in the window, and the status
/// line says the same. The running patch stays.
#[test]
fn a_failed_open_shows_the_reason_in_the_window() {
    let tmp = TempDir::new("open-fails");
    let mut app = app();
    let _ = fake::asked();
    let generation = app.generation;

    assert!(!app.replace(Source::File(&tmp.0.join("missing.ron"))));

    let status = app.alerts.status().to_string();
    assert!(status.starts_with("Could not open the patch"), "{status}");
    assert!(status.ends_with("The current patch is unchanged."), "{status}");
    assert_eq!(app.ui_snapshot().question.unwrap().text, status);
    assert_eq!(app.generation, generation, "the engine was not rebuilt");
}

/// A save that cannot be written shows the reason in the window after Save As,
/// and the patch stays untitled.
#[test]
fn a_failed_save_shows_the_reason_in_the_window() {
    let tmp = TempDir::new("save-fails");
    // A file where the folder should be: the save cannot be written.
    std::fs::write(tmp.0.join("blocked"), "").unwrap();
    fake::answer_save(Some(tmp.0.join("blocked").join("patch.ron")));
    let mut ui = Ui::with_app(app());
    let _ = fake::asked();

    ui.tap(Key::Character("s".into()), Modifiers::COMMAND);

    let status = ui.app.alerts.status().to_string();
    assert!(status.starts_with("Could not save \"patch.ron\""), "{status}");
    assert!(status.ends_with("Your changes are not saved."), "{status}");
    let asked = fake::asked();
    assert!(matches!(asked.as_slice(), [Asked::Save(_)]), "{asked:?}");
    assert_eq!(ui.app.ui_snapshot().question.unwrap().text, status);
    assert_eq!(ui.app.session.document.file(), None);
}
