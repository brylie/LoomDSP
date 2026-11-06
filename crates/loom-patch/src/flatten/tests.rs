use super::*;
use crate::{LoadWarning, PatchError};
use loom_core::{EngineConfig, create_engine};

/// Flattens `text` with a fresh id table.
fn flatten(text: &str) -> (Flat, FlatIds) {
    let patch = Patch::from_ron(text).unwrap();
    let mut ids = FlatIds::new();
    let flat = patch.flatten(&loom_modules::registry(), &mut ids).unwrap();
    (flat, ids)
}

/// The error flattening `text` gives.
fn refuse(text: &str) -> PatchError {
    let patch = Patch::from_ron(text).unwrap();
    patch
        .flatten(&loom_modules::registry(), &mut FlatIds::new())
        .expect_err("refused")
}

/// The flat id of the module at `path`.
fn id(ids: &FlatIds, path: &[u32]) -> u32 {
    let path = ModulePath(path.to_vec());
    ids.paths()
        .find(|(p, _)| **p == path)
        .unwrap_or_else(|| panic!("no module at {path}"))
        .1
        .0
}

/// A cable's two ends, by flat id and port name.
type Ends<'a> = ((u32, &'a str), (u32, &'a str));

/// The flat module at `path`.
fn module<'a>(flat: &'a Flat, path: &[u32]) -> &'a FlatModule {
    flat.modules
        .iter()
        .find(|m| m.path.0 == path)
        .unwrap_or_else(|| panic!("no module at {}", ModulePath(path.to_vec())))
}

/// A patch with one definition `voice` (pitch in, out out) and one
/// instance of it, between a note input and the audio output.
const VOICE: &str = r#"Patch(
    version: 1,
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
                PatchModule(id: 2, kind: "loom.phase.generator@1"),
                PatchModule(id: 3, kind: "loom.osc.sine@1"),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 5, kind: "loom.filter.svf@1", params: {"cutoff": 1000.0}),
                PatchModule(id: 6, kind: "loom.math.mul@1", ranges: {"gain": (0.0, 1.0)}),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "pitch")),
                PatchCable(from: (2, "phase"), to: (3, "phase")),
                PatchCable(from: (3, "out"), to: (5, "in")),
                PatchCable(from: (5, "lowpass"), to: (6, "a")),
                PatchCable(from: (6, "out"), to: (4, "in")),
            ],
            exposed: [
                Exposed(name: "cutoff", module: 5, param: "cutoff", range: Some((200.0, 2000.0))),
                Exposed(name: "gain", module: 6, param: "gain"),
            ],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"cutoff": 5000.0, "gain": 4.0}),
        PatchModule(id: 9, kind: "loom.io.audio-out@1"),
        PatchModule(id: 10, kind: "loom.clock.transport@1"),
        PatchModule(id: 11, kind: "loom.mod.lfo@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (7, "pitch")),
        PatchCable(from: (7, "out"), to: (9, "in")),
    ],
    modulation: [PatchModulation(source: (11, "out"), target: (7, "cutoff"), depth: 0.5)],
    midi: [PatchMidiMapping(target: (7, "cutoff"), channel: 0, controller: 74)],
)"#;

/// Rule 2: a Patch input and output are removed, and the cables into and
/// out of the instance continue to the inner modules. Top-level modules
/// keep their ids, instances included, and inner ids come after them.
#[test]
fn ports_become_cables_through() {
    let (flat, ids) = flatten(VOICE);
    let kinds: Vec<&str> = flat.modules.iter().map(|m| m.module.descriptor.id).collect();
    assert!(!kinds.iter().any(|k| k.starts_with("loom.struct")), "{kinds:?}");
    assert_eq!(id(&ids, &[1]), 1);
    assert_eq!(id(&ids, &[7]), 7);
    assert_eq!(id(&ids, &[9]), 9);
    assert_eq!(id(&ids, &[11]), 11);
    let (phase, sine, svf) = (id(&ids, &[7, 2]), id(&ids, &[7, 3]), id(&ids, &[7, 5]));
    assert!(phase > 11 && sine > 11 && svf > 11, "{phase} {sine} {svf}");
    let cables: Vec<Ends<'_>> = flat
        .cables
        .iter()
        .map(|c| ((c.from.0, c.from.1.as_str()), (c.to.0, c.to.1.as_str())))
        .collect();
    assert!(cables.contains(&((1, "pitch"), (phase, "pitch"))), "{cables:?}");
    let mul = id(&ids, &[7, 6]);
    assert!(cables.contains(&((svf, "lowpass"), (mul, "a"))), "{cables:?}");
    assert!(cables.contains(&((mul, "out"), (9, "in"))), "{cables:?}");
    assert!(cables.contains(&((phase, "phase"), (sine, "phase"))), "{cables:?}");
    assert_eq!(flat.cables.len(), 5);
    assert_eq!(flat.transport, Some(ModuleId(10)));
    assert_eq!(ModulePath(vec![7, 5]).to_string(), "7 › 5");
}

/// Rule 3: an exposed value is written to the inner parameter, clamped to
/// the exposed range, and the inner control range holds it without a
/// note. An assignment and a MIDI mapping on the exposed name reach the
/// inner parameter.
#[test]
fn exposed_values_assignments_and_mappings_reach_inside() {
    let (flat, ids) = flatten(VOICE);
    let svf = module(&flat, &[7, 5]);
    let cutoff = svf.module.descriptor.param_index("cutoff").unwrap();
    assert_eq!(svf.module.params[cutoff], 2000.0);
    // The saved range (0, 1) does not hold the exposed value 4, and is
    // dropped rather than reported: the loader picks the smallest range
    // that holds the value (control-ranges.md, rule 7).
    let mul = module(&flat, &[7, 6]);
    let gain = mul.module.descriptor.param_index("gain").unwrap();
    assert_eq!(mul.module.params[gain], 4.0);
    let range = mul.module.ranges[gain].expect("gain offers ranges");
    assert!(range.min <= 4.0 && 4.0 <= range.max, "{range:?}");
    assert!(flat.warnings.is_empty(), "{:?}", flat.warnings);
    assert_eq!(flat.modulation.len(), 1);
    assert_eq!(flat.modulation[0].source, (11, "out".to_string()));
    assert_eq!(flat.modulation[0].target, (id(&ids, &[7, 5]), "cutoff".to_string()));
    assert_eq!(flat.midi[0].target, (id(&ids, &[7, 5]), "cutoff".to_string()));
}

/// The flat patch loads and plays: the graph holds only ordinary modules,
/// and the controller knows the cables.
#[test]
fn a_nested_patch_loads_into_a_controller() {
    let patch = Patch::from_ron(VOICE).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let notes = patch.load_into(&mut parts.controller).unwrap();
    assert!(notes.is_empty(), "{notes:?}");
    let graph = parts.controller.graph();
    assert_eq!(graph.len(), 8);
    assert_eq!(graph.cables().count(), 5);
    assert!(graph.nodes().all(|(_, n)| !n.descriptor.id.starts_with("loom.struct")));
}

/// One Patch input feeding two inner modules fans the outer cable out to
/// both; one inner output feeding two outside inputs is two cables.
#[test]
fn a_port_fans_out() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "two": SubPatch(
                metadata: Metadata(name: "Two"),
                modules: [
                    PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("gate")),
                    PatchModule(id: 2, kind: "loom.env.envelope@1"),
                    PatchModule(id: 3, kind: "loom.env.envelope@1"),
                    PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("a")),
                ],
                cables: [
                    PatchCable(from: (1, "out"), to: (2, "gate")),
                    PatchCable(from: (1, "out"), to: (3, "gate")),
                    PatchCable(from: (2, "out"), to: (4, "in")),
                ],
            ),
        },
        modules: [
            PatchModule(id: 1, kind: "loom.midi.note@1"),
            PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("two")),
            PatchModule(id: 3, kind: "loom.math.mul@1"),
            PatchModule(id: 4, kind: "loom.math.mul@1"),
        ],
        cables: [
            PatchCable(from: (1, "gate"), to: (2, "gate")),
            PatchCable(from: (2, "a"), to: (3, "a")),
            PatchCable(from: (2, "a"), to: (4, "a")),
        ],
    )"#;
    let (flat, ids) = flatten(text);
    let (env_a, env_b) = (id(&ids, &[2, 2]), id(&ids, &[2, 3]));
    let into = |to: u32| flat.cables.iter().filter(|c| c.to.0 == to).count();
    assert_eq!((into(env_a), into(env_b)), (1, 1));
    assert_eq!(
        flat.cables.iter().filter(|c| c.from == (env_a, "out".into())).count(),
        2
    );
    assert_eq!(flat.cables.len(), 4);
}

/// A Patch input cabled straight to a Patch output is a pass-through: the
/// outer cable continues from whatever feeds the instance, through one
/// level and through two.
#[test]
fn a_pass_through_port_continues_from_outside() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "thru": SubPatch(
                metadata: Metadata(name: "Thru"),
                modules: [
                    PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                    PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
                ],
                cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
            ),
            "outer": SubPatch(
                metadata: Metadata(name: "Outer"),
                modules: [
                    PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                    PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("thru")),
                    PatchModule(id: 3, kind: "loom.struct.output@1", label: Some("out")),
                ],
                cables: [
                    PatchCable(from: (1, "out"), to: (2, "in")),
                    PatchCable(from: (2, "out"), to: (3, "in")),
                ],
            ),
        },
        modules: [
            PatchModule(id: 1, kind: "loom.osc.sine@1"),
            PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("thru")),
            PatchModule(id: 3, kind: "loom.struct.subpatch@1", subpatch: Some("outer")),
            PatchModule(id: 4, kind: "loom.math.mul@1"),
            PatchModule(id: 5, kind: "loom.struct.subpatch@1", subpatch: Some("thru")),
        ],
        cables: [
            PatchCable(from: (1, "out"), to: (2, "in")),
            PatchCable(from: (2, "out"), to: (3, "in")),
            PatchCable(from: (3, "out"), to: (4, "a")),
            PatchCable(from: (5, "out"), to: (4, "b")),
        ],
    )"#;
    let (flat, _) = flatten(text);
    assert!(flat.modules.iter().all(|m| m.path.is_top()));
    assert_eq!(flat.cables.len(), 1, "{:?}", flat.cables);
    assert_eq!(flat.cables[0].from, (1, "out".into()));
    assert_eq!(flat.cables[0].to, (4, "a".into()));
}

/// A pass-through fed by its own output carries nothing: the cable is
/// dropped rather than followed for ever.
#[test]
fn a_loop_of_pass_throughs_is_dropped() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "thru": SubPatch(
                metadata: Metadata(name: "Thru"),
                modules: [
                    PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                    PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
                ],
                cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
            ),
        },
        modules: [
            PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("thru")),
            PatchModule(id: 2, kind: "loom.io.audio-out@1"),
        ],
        cables: [
            PatchCable(from: (1, "out"), to: (1, "in")),
            PatchCable(from: (1, "out"), to: (2, "in")),
        ],
    )"#;
    let (flat, _) = flatten(text);
    assert!(flat.cables.is_empty(), "{:?}", flat.cables);
}

/// Three levels: the bottom module's path names every instance above it,
/// and an exposed name passed up two levels is clamped at each level,
/// outer first.
#[test]
fn three_levels_nest_and_clamp_outer_first() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "filter": SubPatch(
                metadata: Metadata(name: "Filter"),
                modules: [PatchModule(id: 3, kind: "loom.filter.svf@1")],
                exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff", range: Some((200.0, 2000.0)))],
            ),
            "voice": SubPatch(
                metadata: Metadata(name: "Voice"),
                modules: [PatchModule(id: 5, kind: "loom.struct.subpatch@1", subpatch: Some("filter"))],
                exposed: [Exposed(name: "tone", module: 5, param: "cutoff", range: Some((300.0, 400.0)))],
            ),
        },
        modules: [
            PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"tone": 5000.0}),
        ],
    )"#;
    let (flat, _) = flatten(text);
    assert_eq!(flat.modules.len(), 1);
    let svf = module(&flat, &[7, 5, 3]);
    let cutoff = svf.module.descriptor.param_index("cutoff").unwrap();
    assert_eq!(svf.module.params[cutoff], 400.0);
}

/// Rule 4: a path keeps its id across flattens. A module added to a
/// definition gets a fresh id, a removed path's id is never handed out
/// again, and top-level ids stay the file's.
#[test]
fn paths_keep_their_ids_across_flattens() {
    let mut patch = Patch::from_ron(VOICE).unwrap();
    let registry = loom_modules::registry();
    let mut ids = FlatIds::new();
    let first = patch.flatten(&registry, &mut ids).unwrap();
    let sine = id(&ids, &[7, 3]);
    let svf = id(&ids, &[7, 5]);

    // Add a module inside: everything else keeps its id.
    patch.subpatches.get_mut("voice").unwrap().modules.push(PatchModule {
        id: 12,
        kind: "loom.osc.saw@1".into(),
        subpatch: None,
        label: None,
        params: Default::default(),
        position: None,
        size: None,
        ranges: Default::default(),
    });
    let second = patch.flatten(&registry, &mut ids).unwrap();
    assert_eq!(id(&ids, &[7, 3]), sine);
    assert_eq!(id(&ids, &[7, 5]), svf);
    let saw = id(&ids, &[7, 12]);
    assert!(saw > svf, "{saw} {svf}");
    assert_eq!(second.modules.len(), first.modules.len() + 1);

    // Remove it, then add another: the removed id is not reused.
    patch
        .subpatches
        .get_mut("voice")
        .unwrap()
        .modules
        .retain(|m| m.id != 12);
    patch.flatten(&registry, &mut ids).unwrap();
    patch.subpatches.get_mut("voice").unwrap().modules.push(PatchModule {
        id: 13,
        kind: "loom.osc.saw@1".into(),
        subpatch: None,
        label: None,
        params: Default::default(),
        position: None,
        size: None,
        ranges: Default::default(),
    });
    patch.flatten(&registry, &mut ids).unwrap();
    assert!(id(&ids, &[7, 13]) > saw);
    assert_eq!(id(&ids, &[7, 12]), saw, "a removed path keeps its id in the table");
}

/// Rule 6: two flattens of one patch give the same modules, ids, and
/// cables.
#[test]
fn flattening_is_deterministic() {
    let (a, ids_a) = flatten(VOICE);
    let (b, ids_b) = flatten(VOICE);
    assert_eq!(ids_a, ids_b);
    let shape = |flat: &Flat| {
        (
            flat.modules.iter().map(|m| (m.path.clone(), m.id)).collect::<Vec<_>>(),
            flat.cables.clone(),
            flat.modulation.clone(),
        )
    };
    assert_eq!(shape(&a), shape(&b));
}

/// A top-level id above the inner modules' count is kept: ids are reserved
/// before any inner module takes one.
#[test]
fn top_level_ids_are_reserved_before_inner_ones() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "two": SubPatch(
                metadata: Metadata(name: "Two"),
                modules: [
                    PatchModule(id: 1, kind: "loom.osc.sine@1"),
                    PatchModule(id: 2, kind: "loom.osc.sine@1"),
                ],
            ),
        },
        modules: [
            PatchModule(id: 1, kind: "loom.osc.sine@1"),
            PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("two")),
            PatchModule(id: 9, kind: "loom.io.audio-out@1"),
        ],
    )"#;
    let (flat, ids) = flatten(text);
    assert_eq!(id(&ids, &[9]), 9);
    assert_eq!(id(&ids, &[7, 1]), 10);
    assert_eq!(id(&ids, &[7, 2]), 11);
    let order: Vec<u32> = flat.modules.iter().map(|m| m.id.0).collect();
    assert_eq!(order, [1, 10, 11, 9]);
}

/// The id table refuses to give one id to two paths, and never hands out
/// an id it was told about.
#[test]
fn the_id_table_keeps_ids_apart() {
    let mut ids = FlatIds::new();
    ids.reserve(ModulePath::top(3), ModuleId(3)).unwrap();
    assert!(matches!(
        ids.reserve(ModulePath::top(4), ModuleId(3)),
        Err(PatchError::Engine(LoomError::DuplicateModuleId(ModuleId(3))))
    ));
    ids.reserve(ModulePath::top(3), ModuleId(3))
        .expect("the same path again");
    ids.reserve_id(ModuleId(20));
    assert_eq!(ids.id_of(&ModulePath::top(3).child(1)).unwrap(), ModuleId(21));
    assert_eq!(ids.path_of(ModuleId(21)), Some(&ModulePath(vec![3, 1])));
    assert_eq!(ids.path_of(ModuleId(5)), None);
    let mut full = FlatIds::new();
    full.reserve_id(ModuleId::MAX);
    assert!(matches!(
        full.id_of(&ModulePath::top(1)),
        Err(PatchError::Engine(LoomError::ModuleIdOutOfRange(_)))
    ));
}

/// A definition with no ports and no exposed parameters flattens to its
/// modules alone; a cable into or out of its instance is refused.
#[test]
fn a_definition_with_no_ports() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "island": SubPatch(
                metadata: Metadata(name: "Island"),
                modules: [PatchModule(id: 1, kind: "loom.osc.sine@1")],
            ),
        },
        modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("island"))],
    )"#;
    let (flat, _) = flatten(text);
    assert_eq!(flat.modules.len(), 1);
    assert!(flat.cables.is_empty());
    let cabled = text.replace(
        "subpatch: Some(\"island\"))],",
        "subpatch: Some(\"island\")), PatchModule(id: 2, kind: \"loom.io.audio-out@1\")],\n        cables: [PatchCable(from: (1, \"out\"), to: (2, \"in\"))],",
    );
    let error = refuse(&cabled);
    assert!(
        matches!(
            &error,
            PatchError::Engine(LoomError::NoSuchPort {
                module: ModuleId(1),
                direction: "output",
                ..
            })
        ),
        "{error}"
    );
}

/// A placeholder inside a definition takes its ports from the
/// definition's cables and its parameters from the exposed names that
/// reach it, so a patch with an unknown module still opens.
#[test]
fn a_placeholder_inside_a_definition_keeps_its_links() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "fx": SubPatch(
                metadata: Metadata(name: "Fx"),
                modules: [
                    PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                    PatchModule(id: 2, kind: "future.shimmer@1", params: {"size": 0.5}),
                    PatchModule(id: 3, kind: "loom.struct.output@1", label: Some("out")),
                ],
                cables: [
                    PatchCable(from: (1, "out"), to: (2, "input")),
                    PatchCable(from: (2, "output"), to: (3, "in")),
                ],
                exposed: [
                    Exposed(name: "size", module: 2, param: "size"),
                    Exposed(name: "tone", module: 2, param: "tone"),
                ],
            ),
        },
        modules: [
            PatchModule(id: 1, kind: "loom.osc.sine@1"),
            PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("fx"), params: {"size": 0.9}),
            PatchModule(id: 3, kind: "loom.io.audio-out@1"),
            PatchModule(id: 4, kind: "loom.mod.lfo@1"),
        ],
        cables: [
            PatchCable(from: (1, "out"), to: (2, "in")),
            PatchCable(from: (2, "out"), to: (3, "in")),
        ],
        modulation: [PatchModulation(source: (4, "out"), target: (2, "tone"), depth: 0.5)],
    )"#;
    let (flat, ids) = flatten(text);
    let shimmer = module(&flat, &[2, 2]);
    let unknown = shimmer.module.unknown.as_ref().expect("a placeholder");
    assert_eq!(unknown.inputs, ["input"]);
    assert_eq!(unknown.outputs, ["output"]);
    assert_eq!(unknown.params["size"], 0.9);
    assert!(
        unknown.parameters.contains(&"tone".to_string()),
        "{:?}",
        unknown.parameters
    );
    assert_eq!(flat.modulation[0].target, (id(&ids, &[2, 2]), "tone".to_string()));
    assert!(matches!(
        &flat.warnings[..],
        [LoadWarning::UnknownModules { kinds }] if kinds == &["future.shimmer@1"]
    ));
    let patch = Patch::from_ron(text).unwrap();
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    patch.load_into(&mut parts.controller).expect("a placeholder loads");
}

/// A range note from inside a definition says which sub-patch it is in,
/// and keeps the inner module's own id.
#[test]
fn a_note_from_inside_names_the_sub_patch() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "v": SubPatch(
                metadata: Metadata(name: "Voice"),
                modules: [PatchModule(id: 3, kind: "loom.phase.generator@1", ranges: {"tune": (0.0, 2.0)})],
            ),
        },
        modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("v"))],
    )"#;
    let (flat, _) = flatten(text);
    let [LoadWarning::InSubPatch { name, warning }] = &flat.warnings[..] else {
        panic!("{:?}", flat.warnings);
    };
    assert_eq!(name, "Voice");
    assert!(
        matches!(**warning, LoadWarning::RangeIgnored { module: 3, .. }),
        "{warning}"
    );
    assert_eq!(warning.level(), flat.warnings[0].level());
    assert!(
        flat.warnings[0]
            .to_string()
            .starts_with("in sub-patch \"Voice\": module 3:")
    );
}

/// Each refusal names what is wrong, with the definition's name and the
/// inner module's own id for anything inside a definition.
#[test]
fn every_mistake_is_refused_by_name() {
    let wrap = |definition: &str, top: &str| {
        format!(
            r#"Patch(version: 1, subpatches: {{ "d": SubPatch(metadata: Metadata(name: "Def"), {definition}) }},
                modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("d")){top}])"#
        )
    };
    let cases: Vec<(String, &str)> = vec![
        (
            r#"Patch(version: 1, modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("nope"))])"#.into(),
            "module 1: there is no sub-patch `nope`",
        ),
        (
            r#"Patch(version: 1, modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1")])"#.into(),
            "module 1: `loom.struct.subpatch@1` and its `subpatch` field disagree",
        ),
        (
            r#"Patch(version: 1, modules: [PatchModule(id: 1, kind: "loom.osc.sine@1", subpatch: Some("d"))])"#.into(),
            "module 1: `loom.osc.sine@1` and its `subpatch` field disagree",
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.osc.sine@1")]"#, r#", PatchModule(id: 2, kind: "loom.io.audio-out@1")], cables: [PatchCable(from: (1, "out"), to: (2, "in"))"#),
            "module #1 has no output port `out`",
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.struct.input@1"), PatchModule(id: 3, kind: "loom.struct.input@1")]"#, ""),
            r#"in sub-patch "Def": two input ports are called `in`"#,
        ),
        (
            wrap(r#"modules: [], exposed: [Exposed(name: "x", module: 9, param: "cutoff")]"#, ""),
            r#"in sub-patch "Def": exposed parameter `x`: there is no module 9"#,
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.osc.sine@1")], exposed: [Exposed(name: "x", module: 2, param: "cutoff")]"#, ""),
            r#"in sub-patch "Def": exposed parameter `x`: module 2 has no parameter `cutoff`"#,
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.math.mul@1")], exposed: [Exposed(name: "x", module: 2, param: "gain"), Exposed(name: "x", module: 2, param: "gain")]"#, ""),
            r#"in sub-patch "Def": exposed parameter `x`: it is exposed twice"#,
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.math.mul@1")], exposed: [Exposed(name: "x", module: 2, param: "gain", range: Some((1.0, 0.0)))]"#, ""),
            r#"in sub-patch "Def": exposed parameter `x`: its range (1, 0) is not a finite span"#,
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.clock.transport@1")]"#, ""),
            r#"in sub-patch "Def": module 2 is a Transport, and a sub-patch cannot hold one"#,
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.struct.input@1"), PatchModule(id: 3, kind: "loom.math.mul@1")], modulation: [PatchModulation(source: (2, "out"), target: (3, "gain"), depth: 0.5)]"#, ""),
            r#"in sub-patch "Def": module 2 is a Patch input, and an assignment cannot start at a port"#,
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.osc.sine@1")]"#, r#", PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("d"), params: {"gain": 1.0})"#),
            "module 2: unknown parameter `gain`",
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.osc.sine@1")]"#, r#", PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("d"), ranges: {"x": (0.0, 1.0)})"#),
            "module 2: unknown parameter `x`",
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.osc.sine@1"), PatchModule(id: 3, kind: "loom.osc.sine@1")], cables: [PatchCable(from: (2, "out"), to: (3, "inn"))]"#, ""),
            r#"in sub-patch "Def": module #3 has no input port `inn`"#,
        ),
        (
            wrap(r#"modules: [PatchModule(id: 2, kind: "loom.math.mul@1")], midi: [PatchMidiMapping(target: (2, "gain"), channel: 0, controller: 1)]"#, r#", PatchModule(id: 2, kind: "loom.math.mul@1")], midi: [PatchMidiMapping(target: (2, "gain"), channel: 0, controller: 1)"#),
            "two MIDI mappings use controller 1 on channel 0",
        ),
    ];
    for (text, expected) in cases {
        let error = refuse(&text).to_string();
        assert!(error.contains(expected), "expected `{expected}`, got `{error}`\n{text}");
    }
}

/// A definition that contains itself through another is refused, with
/// the loop's names in order.
#[test]
fn a_definition_that_contains_itself_is_refused() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "a": SubPatch(metadata: Metadata(name: "Voice"), modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("b"))]),
            "b": SubPatch(metadata: Metadata(name: "Filter"), modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("a"))]),
        },
        modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("a"))],
    )"#;
    let error = refuse(text);
    let mut inner: &PatchError = &error;
    while let PatchError::InSubPatch { source, .. } = inner {
        inner = source;
    }
    assert!(
        matches!(inner, PatchError::SubPatchLoop { path } if path == &["Voice", "Filter", "Voice"]),
        "{error}"
    );
    assert!(
        error
            .to_string()
            .ends_with("sub-patch Voice › Filter › Voice contains itself"),
        "{error}"
    );
}

/// Two MIDI mappings that collide only after flattening (an outer one on
/// the exposed name, an inner one on the parameter) are refused by path.
#[test]
fn mappings_that_collide_after_flattening_are_refused_by_path() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "v": SubPatch(
                metadata: Metadata(name: "Voice"),
                modules: [PatchModule(id: 3, kind: "loom.filter.svf@1")],
                midi: [PatchMidiMapping(target: (3, "cutoff"), channel: 0, controller: 1)],
                exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff")],
            ),
        },
        modules: [PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("v"))],
        midi: [PatchMidiMapping(target: (7, "cutoff"), channel: 0, controller: 2)],
    )"#;
    let error = refuse(text).to_string();
    assert_eq!(error, "two MIDI mappings use parameter `cutoff` of module 7 › 3");
}

/// At the top level, Patch inputs and outputs are ordinary modules, as
/// when a library file is opened on its own.
#[test]
fn ports_at_the_top_level_are_ordinary_modules() {
    let text = r#"Patch(
        version: 1,
        modules: [
            PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
            PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
        ],
        cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
    )"#;
    let (flat, _) = flatten(text);
    assert_eq!(flat.modules.len(), 2);
    assert_eq!(flat.cables.len(), 1);
}

/// The nested example's id table, as `docs/learn/flattening.md` shows it:
/// top-level ids are kept, and inner modules take the next ids in visit
/// order, depth first.
#[test]
fn the_nested_example_has_the_documented_id_table() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/dev/nested-subtractive.ron");
    let text = std::fs::read_to_string(path).unwrap();
    let (_, ids) = flatten(&text);
    let table: Vec<(String, u32)> = ids.paths().map(|(path, id)| (path.to_string(), id.0)).collect();
    let expected = [
        ("1", 1),
        ("2", 2),
        ("2 › 3", 12),
        ("2 › 4", 13),
        ("2 › 5 › 3", 14),
        ("2 › 5 › 4", 15),
        ("2 › 5 › 5", 16),
        ("2 › 6", 17),
        ("2 › 7", 18),
        ("9", 9),
        ("10", 10),
        ("11", 11),
    ];
    let expected: Vec<(String, u32)> = expected.iter().map(|(p, id)| (p.to_string(), *id)).collect();
    assert_eq!(table, expected);
}
