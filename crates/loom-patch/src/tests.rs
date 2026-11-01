use super::*;
use loom_core::{EngineConfig, create_engine};

const SIMPLE: &str = r#"
// Comments are allowed.
Patch(
    version: 1,
    name: "Simple sine",
    modules: [
        (id: 3, kind: "loom.osc.sine@1"),
        (id: 1, kind: "loom.midi.note@1"),
        (id: 2, kind: "loom.phase.generator@1", params: {"tune": 12.0}),
        (id: 4, kind: "loom.io.audio-out@1"),
        (id: 5, kind: "loom.clock.transport@1"),
    ],
    cables: [
        (from: (2, "phase"), to: (3, "phase")),
        (from: (1, "pitch"), to: (2, "pitch")),
        (from: (3, "out"), to: (4, "in")),
    ],
)
"#;

/// Loads `text` into a new engine. Returns the controller and the notes,
/// without the note that a Transport was added.
fn load_text(text: &str) -> (Controller, Vec<String>) {
    let patch = Patch::from_ron(text).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let mut notes = patch.load_into(&mut parts.controller).unwrap();
    notes.retain(|note| !note.contains("no Transport"));
    (parts.controller, notes)
}

/// clock.md: a patch without a Transport gets one on load, with a note.
/// One with two fails with an error that names both. The settings of
/// the one it has, and its run state, survive a save and a load; its
/// transient `reset` is never saved.
#[test]
fn every_patch_loads_with_exactly_one_transport() {
    let patch = Patch::from_ron(r#"Patch(version: 1, modules: [(id: 3, kind: "loom.math.mul@1")])"#).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let notes = patch.load_into(&mut parts.controller).unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains("no Transport") && notes[0].contains("module 4"),
        "{notes:?}"
    );
    assert_eq!(parts.controller.transport_module(), Some(ModuleId(4)));

    let two = Patch::from_ron(
        r#"Patch(version: 1, modules: [
                (id: 2, kind: "loom.clock.transport@1"),
                (id: 7, kind: "loom.clock.transport@1"),
            ])"#,
    )
    .unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let error = two.load_into(&mut parts.controller).unwrap_err();
    assert!(matches!(error, PatchError::TwoTransports { first: 2, second: 7 }));
    let message = error.to_string();
    assert!(message.contains('2') && message.contains('7'), "{message}");

    let (mut c, notes) = load_text(SIMPLE);
    assert!(notes.is_empty());
    let transport = ModuleId(5);
    c.set_param_named(transport, "tempo", 96.0).unwrap();
    c.set_param_named(transport, "numerator", 7.0).unwrap();
    c.set_param_named(transport, "denominator", 3.0).unwrap();
    c.set_param_named(transport, "run", 0.0).unwrap();
    c.set_param_named(transport, "reset", 1.0).unwrap();
    let text = Patch::from_graph("saved", c.graph()).to_ron().unwrap();
    assert!(!text.contains("reset"), "{text}");
    let (reloaded, notes) = load_text(&text);
    assert!(notes.is_empty());
    let node = reloaded.graph().node(transport).unwrap();
    let value = |name: &str| node.params[node.descriptor.param_index(name).unwrap()];
    assert_eq!(
        (
            value("tempo"),
            value("numerator"),
            value("denominator"),
            value("run"),
            value("reset")
        ),
        (96.0, 7.0, 3.0, 0.0, 0.0)
    );
}

/// Value and selected range of Multiply #1's gain.
fn gain(controller: &Controller) -> (f32, Option<ParamRange>) {
    let node = controller.graph().node(ModuleId(1)).unwrap();
    (node.params[0], node.range(0))
}

/// control-ranges.md: a selected range saves next to the values and
/// loads back. The default range is not written.
#[test]
fn ranges_save_and_reload() {
    let (mut c, notes) = load_text(
        r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1"), (id: 2, kind: "loom.math.mul@1")])"#,
    );
    assert!(notes.is_empty());
    assert_eq!(gain(&c), (1.0, Some(ParamRange::new(-1.0, 1.0))), "the default range");
    c.set_range(ModuleId(1), 0, ParamRange::new(-0.1, 0.1)).unwrap();
    c.set_param(ModuleId(1), 0, 0.08).unwrap();
    let text = Patch::from_graph("ranges", c.graph()).to_ron().unwrap();
    assert!(text.contains(r#"ranges: {"gain": (-0.1, 0.1)}"#), "{text}");
    assert_eq!(
        text.matches("ranges: {").count(),
        1,
        "module 2 keeps the default range: {text}"
    );

    let (reloaded, notes) = load_text(&text);
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(gain(&reloaded), (0.08, Some(ParamRange::new(-0.1, 0.1))));
    // Two instances keep separate ranges.
    let other = reloaded.graph().node(ModuleId(2)).unwrap();
    assert_eq!(other.range(0), Some(ParamRange::new(-1.0, 1.0)));
}

/// Rule 7: a value outside the default range loads unchanged, with the
/// smallest range that holds it. A saved range that does not hold the
/// value does the same, with a note.
#[test]
fn a_range_never_changes_a_loaded_value() {
    let (c, notes) =
        load_text(r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 5.0})])"#);
    assert!(notes.is_empty());
    assert_eq!(gain(&c), (5.0, Some(ParamRange::new(-5.0, 5.0))));

    let (c, notes) = load_text(
        r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 5.0}, ranges: {"gain": (-0.1, 0.1)})])"#,
    );
    assert_eq!(gain(&c), (5.0, Some(ParamRange::new(-5.0, 5.0))));
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].contains("-0.1 to 0.1") && notes[0].contains("-5 to 5"),
        "{notes:?}"
    );

    let (c, notes) = load_text(
        r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", params: {"gain": 0.5}, ranges: {"gain": (-2.0, 2.0)})])"#,
    );
    assert_eq!(gain(&c), (0.5, Some(ParamRange::new(-1.0, 1.0))));
    assert!(notes[0].contains("does not offer"), "{notes:?}");

    // A range for a parameter the module does not have is an error, as
    // for a value.
    let patch = Patch::from_ron(
        r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.mul@1", ranges: {"nope": (0.0, 1.0)})])"#,
    )
    .unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let error = patch.load_into(&mut parts.controller).unwrap_err();
    assert!(matches!(error, PatchError::UnknownParam { module: 1, .. }), "{error}");

    // A range on a parameter that offers none is ignored, with a note.
    let (_, notes) = load_text(
        r#"Patch(version: 1, modules: [(id: 1, kind: "loom.math.convert@1", ranges: {"mode": (0.0, 1.0)})])"#,
    );
    assert!(notes.iter().any(|n| n.contains("offers no ranges")), "{notes:?}");
}

/// Every example patch loads with the values in its file.
#[test]
fn example_patches_keep_their_values() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "ron") {
            continue;
        }
        let patch = Patch::load(&path).unwrap();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        let notes = patch.load_into(&mut parts.controller).unwrap();
        assert!(notes.is_empty(), "{}: {notes:?}", path.display());
        for module in &patch.modules {
            let node = parts.controller.graph().node(ModuleId(module.id)).unwrap();
            for (name, &value) in &module.params {
                let index = node.descriptor.param_index(name).unwrap();
                let expected = node.descriptor.params[index].clamp(value);
                assert_eq!(node.params[index], expected, "{}: {} {name}", path.display(), module.id);
            }
        }
    }
}

/// A held momentary Button saves no `hold`, and a saved `hold` loads as
/// released and is listed for `loom check` (transient-params.md).
#[test]
fn a_transient_hold_is_never_saved_or_loaded() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let c = &mut parts.controller;
    let id = c.add_module("loom.ui.button").unwrap();
    c.set_param_named(id, "mode", 1.0).unwrap();
    c.set_param_named(id, "hold", 1.0).unwrap();
    c.commit().unwrap();
    let saved = Patch::from_graph("held", c.graph());
    let text = saved.to_ron().unwrap();
    assert!(!text.contains("hold"), "{text}");
    assert!(text.contains("mode"), "{text}");

    let file = r#"Patch(version: 1, modules: [(id: 1, kind: "loom.ui.button@1", params: {"hold": 1.0, "on": 1.0})])"#;
    let patch = Patch::from_ron(file).unwrap();
    let registry = loom_modules::registry();
    assert_eq!(patch.transient_values(&registry), vec![(1, "hold".to_string())]);
    let mut parts = create_engine(registry, EngineConfig::default());
    patch.load_into(&mut parts.controller).unwrap();
    let node = parts.controller.graph().node(ModuleId(1)).unwrap();
    let hold = node.descriptor.param_index("hold").unwrap();
    let on = node.descriptor.param_index("on").unwrap();
    assert_eq!(node.params[hold], 0.0);
    assert_eq!(node.params[on], 1.0);
}

/// Tile size is optional in old patches and survives RON serialization.
#[test]
fn tile_size_round_trips_and_old_patches_load() {
    let mut patch = Patch::from_ron(SIMPLE).unwrap();
    assert!(patch.modules.iter().all(|m| m.size.is_none()));
    patch.modules[0].size = Some((24, 16));
    let text = patch.to_ron().unwrap();
    assert!(text.contains("size: (24, 16)"));
    let parsed = Patch::from_ron(&text).unwrap();
    assert_eq!(parsed.modules.iter().find(|m| m.id == 3).unwrap().size, Some((24, 16)));
}

/// Unknown module data, ports, assignments, and MIDI mappings survive a graph round trip.
#[test]
fn unknown_module_preserves_named_links_and_settings() {
    let text = r#"Patch(
            version: 1, name: "Missing",
            modules: [
                (id: 1, kind: "future.effect@3", label: Some("Future"), params: {"amount": 0.3}, ranges: {"amount": (0.0, 2.0)}),
                (id: 2, kind: "loom.phase.generator@1"),
                (id: 3, kind: "loom.clock.transport@1"),
            ],
            cables: [(from: (2, "phase"), to: (1, "input"))],
            modulation: [
                (source: (1, "output"), target: (2, "tune"), depth: 0.5),
                (source: (2, "phase"), target: (1, "other"), depth: 0.25),
            ],
            midi: [(target: (1, "amount"), channel: 0, controller: 74)],
        )"#;
    let patch = Patch::from_ron(text).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let notes = patch.load_into(&mut parts.controller).unwrap();
    assert!(notes.iter().any(|note| note.contains("future.effect@3")));
    let node = parts.controller.graph().node(ModuleId(1)).unwrap();
    let unknown = node.unknown.as_ref().unwrap();
    assert_eq!(unknown.inputs, ["input"]);
    assert_eq!(unknown.outputs, ["output"]);
    let saved = Patch::from_graph("Missing", parts.controller.graph());
    let mut expected = patch;
    expected.canonicalise();
    assert_eq!(saved, expected);
}

/// Unknown versions become placeholders, but malformed kinds still fail.
#[test]
fn unknown_version_and_malformed_kind() {
    let patch = Patch::from_ron(&SIMPLE.replace("loom.osc.sine@1", "loom.osc.sine@2")).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    patch.load_into(&mut parts.controller).unwrap();
    assert_eq!(
        parts
            .controller
            .graph()
            .node(ModuleId(3))
            .unwrap()
            .unknown
            .as_ref()
            .unwrap()
            .kind,
        "loom.osc.sine@2"
    );
    for kind in ["loom.osc.sine", "@2", "unknown@bad"] {
        let patch = Patch::from_ron(&SIMPLE.replace("loom.osc.sine@1", kind)).unwrap();
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        assert!(patch.load_into(&mut parts.controller).is_err(), "{kind}");
    }
}

#[test]
fn loads_and_round_trips() {
    let patch = Patch::from_ron(SIMPLE).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    patch.load_into(&mut parts.controller).unwrap();

    let saved = Patch::from_graph("Simple sine", parts.controller.graph());
    let mut expected = patch.clone();
    expected.canonicalise();
    assert_eq!(saved, expected);

    // Canonical text is stable across a parse/print cycle.
    let text = saved.to_ron().unwrap();
    assert_eq!(Patch::from_ron(&text).unwrap().to_ron().unwrap(), text);
}

#[test]
fn rejects_newer_versions() {
    let text = SIMPLE.replace("version: 1", "version: 99");
    assert!(matches!(
        Patch::from_ron(&text),
        Err(PatchError::TooNew { found: 99, .. })
    ));
}

const MODULATED: &str = r#"
Patch(
    version: 1,
    name: "Modulated",
    modules: [
        (id: 1, kind: "loom.math.constant@1", params: {"value": 1.0}),
        (id: 2, kind: "loom.env.envelope@1"),
        (id: 3, kind: "loom.filter.svf@1"),
        (id: 4, kind: "loom.clock.transport@1"),
    ],
    modulation: [
        (source: (2, "out"), target: (3, "resonance"), depth: 0.4, polarity: Unipolar, enabled: false),
        (source: (1, "out"), target: (3, "cutoff"), depth: 0.25),
    ],
)
"#;

#[test]
fn modulation_round_trips_and_old_patches_load() {
    let old = Patch::from_ron(SIMPLE).unwrap();
    assert!(old.modulation.is_empty());
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    old.load_into(&mut parts.controller).unwrap();
    let saved = Patch::from_graph("Simple sine", parts.controller.graph());
    assert!(saved.modulation.is_empty());
    assert!(!saved.to_ron().unwrap().contains("modulation"));

    let patch = Patch::from_ron(MODULATED).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    patch.load_into(&mut parts.controller).unwrap();
    let saved = Patch::from_graph("Modulated", parts.controller.graph());
    let mut expected = patch.clone();
    expected.canonicalise();
    assert_eq!(saved, expected);
    // File order is kept, not sorted by target.
    assert_eq!(saved.modulation[0].target.1, "resonance");
    assert_eq!(saved.modulation[0].polarity, Some(PatchPolarity::Unipolar));
    assert!(!saved.modulation[0].enabled);
    assert_eq!(saved.modulation[1].target.1, "cutoff");
    assert!(saved.modulation[1].polarity.is_none());
    assert!(saved.modulation[1].enabled);

    let text = saved.to_ron().unwrap();
    assert!(text.contains("modulation:"));
    assert!(text.contains("enabled: false"));
    assert!(!text.contains("enabled: true"));
    assert_eq!(Patch::from_ron(&text).unwrap().to_ron().unwrap(), text);
}

/// The engine adds assignments on one parameter in id order. Float sums
/// depend on order, so a reloaded patch must keep it (review on #133).
#[test]
fn save_replaces_the_file_and_leaves_no_temporary() {
    let dir = std::env::temp_dir().join(format!("loom-patch-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.ron");
    std::fs::write(&path, "old").unwrap();
    let patch = Patch::new("Saved");
    patch.save(&path).unwrap();
    assert_eq!(Patch::load(&path).unwrap().name, "Saved");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file is left");
    // A target that cannot be written reports the error and keeps the old file.
    let missing = dir.join("no-such-folder").join("p.ron");
    assert!(matches!(patch.save(&missing), Err(PatchError::Io { .. })));
    // A read-only target is refused and keeps its content.
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).unwrap();
    assert!(matches!(Patch::new("Other").save(&path), Err(PatchError::Io { .. })));
    assert_eq!(Patch::load(&path).unwrap().name, "Saved");
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&path, perms).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn save_through_a_symlink_updates_the_target() {
    let dir = std::env::temp_dir().join(format!("loom-patch-link-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let real = dir.join("real.ron");
    let link = dir.join("link.ron");
    Patch::new("First").save(&real).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    Patch::new("Second").save(&link).unwrap();
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(Patch::load(&real).unwrap().name, "Second");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn assignment_order_survives_save_and_load() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let controller = &mut parts.controller;
    let low = controller.add_module("loom.math.constant").unwrap();
    let high = controller.add_module("loom.math.constant").unwrap();
    let filter = controller.add_module("loom.filter.svf").unwrap();
    // Made from the higher module id first, so sorting by source would
    // swap them.
    controller
        .add_assignment_named(high, "out", filter, "cutoff", 0.5)
        .unwrap();
    controller
        .add_assignment_named(low, "out", filter, "cutoff", -0.25)
        .unwrap();
    let order =
        |graph: &Graph| -> Vec<(OutputRef, f32)> { graph.assignments().map(|(_, a)| (a.source, a.depth)).collect() };
    let before = order(controller.graph());

    let text = Patch::from_graph("Order", controller.graph()).to_ron().unwrap();
    let mut reloaded = create_engine(loom_modules::registry(), EngineConfig::default());
    Patch::from_ron(&text)
        .unwrap()
        .load_into(&mut reloaded.controller)
        .unwrap();
    assert_eq!(order(reloaded.controller.graph()), before);
}

const MAPPED: &str = r#"
Patch(
    version: 1,
    name: "Mapped",
    modules: [
        (id: 1, kind: "loom.filter.svf@1"),
        (id: 2, kind: "loom.clock.transport@1"),
    ],
    midi: [
        (target: (1, "resonance"), channel: 0, controller: 71),
        (target: (1, "cutoff"), channel: 2, controller: 74),
    ],
)
"#;

#[test]
fn midi_mappings_round_trip_and_old_patches_load() {
    let old = Patch::from_ron(SIMPLE).unwrap();
    assert!(old.midi.is_empty());
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    old.load_into(&mut parts.controller).unwrap();
    assert_eq!(parts.controller.graph().midi_mappings().count(), 0);
    let text = Patch::from_graph("Simple sine", parts.controller.graph())
        .to_ron()
        .unwrap();
    assert!(!text.contains("midi:"));

    let patch = Patch::from_ron(MAPPED).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    patch.load_into(&mut parts.controller).unwrap();
    let saved = Patch::from_graph("Mapped", parts.controller.graph());
    let mut expected = patch.clone();
    expected.canonicalise();
    assert_eq!(saved, expected);
    // Sorted by target.
    assert_eq!(saved.midi[0].target.1, "cutoff");
    assert_eq!((saved.midi[0].channel, saved.midi[0].controller), (2, 74));
    let text = saved.to_ron().unwrap();
    assert_eq!(Patch::from_ron(&text).unwrap().to_ron().unwrap(), text);
}

#[test]
fn rejects_bad_midi_mappings() {
    let load = |text: &str| {
        let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
        Patch::from_ron(text).unwrap().load_into(&mut parts.controller)
    };
    let same_param = MAPPED.replace("\"resonance\"", "\"cutoff\"");
    assert!(matches!(
        load(&same_param),
        Err(PatchError::DuplicateMidiMapping { .. })
    ));
    let same_control = MAPPED.replace("channel: 2, controller: 74", "channel: 0, controller: 71");
    assert!(matches!(
        load(&same_control),
        Err(PatchError::DuplicateMidiMapping { .. })
    ));
    let unknown = MAPPED.replace("\"cutoff\"", "\"cut\"");
    assert!(matches!(
        load(&unknown),
        Err(PatchError::UnknownParam { module: 1, .. })
    ));
    let bad_channel = MAPPED.replace("channel: 2", "channel: 16");
    assert!(matches!(
        load(&bad_channel),
        Err(PatchError::Engine(LoomError::InvalidMidiControl(_)))
    ));
}

#[test]
fn rejects_unknown_params() {
    let text = SIMPLE.replace("\"tune\"", "\"tuning\"");
    let patch = Patch::from_ron(&text).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    assert!(matches!(
        patch.load_into(&mut parts.controller),
        Err(PatchError::UnknownParam { module: 2, .. })
    ));
}
