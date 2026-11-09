//! The overlay for modes (sub-patches.md, Modes; ADR-0040).

use super::*;
use crate::flatten::{ADDED_STEP, COPY_STEP};
use loom_core::{EngineConfig, EngineParts, create_engine};

/// A patch with one sub-patch `voice` (pitch in, out out; a sine through a
/// filter whose cutoff is exposed) as instance 7, and an LFO 11.
const VOICE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Modes"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
                PatchModule(id: 2, kind: "loom.phase.generator@1"),
                PatchModule(id: 3, kind: "loom.osc.sine@1"),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 5, kind: "loom.filter.svf@1", params: {"cutoff": 1000.0}),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "pitch")),
                PatchCable(from: (2, "phase"), to: (3, "phase")),
                PatchCable(from: (3, "out"), to: (5, "in")),
                PatchCable(from: (5, "lowpass"), to: (4, "in")),
            ],
            exposed: [Exposed(name: "cutoff", module: 5, param: "cutoff")],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"cutoff": 1200.0}),
        PatchModule(id: 9, kind: "loom.io.audio-out@1"),
        PatchModule(id: 10, kind: "loom.clock.transport@1"),
        PatchModule(id: 11, kind: "loom.mod.lfo@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (7, "pitch")),
        PatchCable(from: (7, "out"), to: (9, "in")),
    ],
    modulation: [PatchModulation(source: (11, "out"), target: (7, "cutoff"), depth: 0.5)],
)"#;

/// Loads `text` into a fresh engine and reads its hierarchy.
fn load(text: &str) -> (Controller, Hierarchy) {
    let EngineParts { mut controller, .. } = create_engine(loom_modules::registry(), EngineConfig::default());
    let patch = Patch::from_ron(text).unwrap();
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).unwrap();
    let hierarchy = Hierarchy::from_patch(&patch, controller.graph(), ids, controller.registry()).unwrap();
    (controller, hierarchy)
}

/// The patch as the hierarchy writes it, in canonical text.
fn written(hierarchy: &Hierarchy, controller: &Controller) -> String {
    let mut patch = hierarchy.unflatten(controller.graph(), |_| Layout::default());
    patch.metadata = Metadata {
        name: "Modes".to_string(),
        ..Metadata::default()
    };
    patch.to_ron().unwrap()
}

/// A role kind's name, as the app would give it.
fn role_name(kind: &str) -> String {
    kind.to_string()
}

/// Syncs and commits, as the app does after an overlay edit.
fn commit(hierarchy: &mut Hierarchy, controller: &mut Controller) {
    hierarchy.sync(controller).unwrap();
    controller.commit().unwrap();
}

/// The flat id at `path`.
fn id(hierarchy: &Hierarchy, path: &[u32]) -> ModuleId {
    hierarchy.ids().get(&ModulePath(path.to_vec())).unwrap()
}

/// A stacked file loads with its copies implied: not tiles, not written,
/// and the file writes back the text it loaded from.
#[test]
fn a_stack_round_trips_and_its_copies_are_not_tiles() {
    let text = VOICE.replace(
        r#"metadata: Metadata(name: "Voice"),"#,
        r#"metadata: Metadata(name: "Voice"), mode: Stack, copies: 3, spread: {"cutoff": 0.25}, stereo_spread: true,"#,
    );
    let (controller, hierarchy) = load(&text);
    let expected = Patch::from_ron(&text).unwrap().to_ron().unwrap();
    assert_eq!(written(&hierarchy, &controller), expected);
    let copy = id(&hierarchy, &[7, COPY_STEP + 1, 5]);
    assert!(hierarchy.is_hidden(copy) && hierarchy.tile(controller.graph(), copy).is_none());
    assert!(
        hierarchy
            .tile(controller.graph(), id(&hierarchy, &[7, ADDED_STEP]))
            .is_none()
    );
    assert_eq!(hierarchy.copies_of(id(&hierarchy, &[7, 5])).len(), 2);
    assert!(hierarchy.transient_ids().contains(&copy));
    assert_eq!(hierarchy.path_names(&ModulePath(vec![7])), vec!["Voice ×3".to_string()]);
}

/// Single to stack, then Copies 4: copy 0 keeps its ids, copies appear,
/// an edit inside reaches every copy at the next sync, and Copies 2 removes
/// the last two while copy 1 keeps its id.
#[test]
fn copies_follow_the_mode_the_count_and_edits_inside() {
    let (mut controller, mut hierarchy) = load(VOICE);
    let instance = ModuleId(7);
    let svf = id(&hierarchy, &[7, 5]);
    hierarchy
        .set_mode(controller.graph(), instance, Mode::Stack, &role_name)
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    assert!(hierarchy.implied_ids().is_empty(), "one copy adds nothing");
    hierarchy.set_copies(instance, 4).unwrap();
    commit(&mut hierarchy, &mut controller);
    assert_eq!(id(&hierarchy, &[7, 5]), svf);
    let copy1 = id(&hierarchy, &[7, COPY_STEP + 1, 5]);
    assert!(controller.graph().node(copy1).is_some());
    // The LFO from outside reaches every copy.
    let onto = |controller: &Controller, target: ModuleId| {
        controller
            .graph()
            .assignments()
            .filter(|(_, a)| a.source.module == ModuleId(11) && a.target == target)
            .count()
    };
    assert_eq!(onto(&controller, copy1), 1);
    // A module added inside copy 0 reaches every copy.
    let level = ModulePath(vec![7]);
    let (_, added) = hierarchy.new_module(&level).unwrap();
    controller.add_module_with_id(added, "loom.osc.saw@1").unwrap();
    commit(&mut hierarchy, &mut controller);
    let saws = controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == "loom.osc.saw")
        .count();
    assert_eq!(saws, 4);
    hierarchy.set_copies(instance, 2).unwrap();
    commit(&mut hierarchy, &mut controller);
    assert_eq!(id(&hierarchy, &[7, COPY_STEP + 1, 5]), copy1);
    assert!(hierarchy.ids().get(&ModulePath(vec![7, COPY_STEP + 2, 5])).is_none());
    let text = written(&hierarchy, &controller);
    assert!(text.contains("mode: Stack") && text.contains("copies: 2"), "{text}");
    hierarchy
        .set_mode(controller.graph(), instance, Mode::Single, &role_name)
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    assert!(hierarchy.implied_ids().is_empty());
    assert!(controller.graph().node(copy1).is_none());
}

/// Spread and stereo spread are set on the stack and reach the file; a
/// spread of 0 leaves the file.
#[test]
fn spread_settings_reach_the_file() {
    let (mut controller, mut hierarchy) = load(VOICE);
    let instance = ModuleId(7);
    hierarchy
        .set_mode(controller.graph(), instance, Mode::Stack, &role_name)
        .unwrap();
    hierarchy.set_copies(instance, 3).unwrap();
    hierarchy.set_spread(instance, "cutoff", 0.4);
    hierarchy.set_spread(instance, "nothing", 0.4);
    hierarchy.set_stereo_spread(instance, true);
    commit(&mut hierarchy, &mut controller);
    let text = written(&hierarchy, &controller);
    assert!(
        text.contains(r#""cutoff": 0.4"#) && text.contains("stereo_spread: true"),
        "{text}"
    );
    let spread = controller
        .graph()
        .assignments()
        .filter(|(_, a)| hierarchy.added_module(a.source.module).is_some())
        .count();
    assert_eq!(spread, 2, "copies 0 and 2; copy 1 sits at offset 0");
    hierarchy.set_spread(instance, "cutoff", 0.0);
    commit(&mut hierarchy, &mut controller);
    assert!(!written(&hierarchy, &controller).contains("spread: {"));
}

/// Single to layer makes the inside member 1 with every flat id kept and
/// the cables unchanged; back to single gives the file it started from.
#[test]
fn single_to_layer_and_back_keeps_ids_and_the_file() {
    let (mut controller, mut hierarchy) = load(VOICE);
    let before = written(&hierarchy, &controller);
    let graph_cables_before = graph_cables(controller.graph());
    let svf = id(&hierarchy, &[7, 5]);
    hierarchy
        .set_mode(controller.graph(), ModuleId(7), Mode::Layer, &role_name)
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    let member = hierarchy.members_of(ModuleId(7));
    assert_eq!(member.len(), 1);
    let member_path = hierarchy.ids().path_of(member[0]).unwrap().clone();
    assert_eq!(hierarchy.ids().get(&member_path.child(5)), Some(svf));
    assert_eq!(graph_cables(controller.graph()), graph_cables_before);
    let layered = written(&hierarchy, &controller);
    assert!(
        layered.contains("mode: Layer") && layered.contains("members: ["),
        "{layered}"
    );
    // It loads back the same.
    let (again_controller, again) = load(&layered);
    assert_eq!(written(&again, &again_controller), layered);
    hierarchy
        .set_mode(controller.graph(), ModuleId(7), Mode::Single, &role_name)
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    assert_eq!(written(&hierarchy, &controller), before);
    assert_eq!(id(&hierarchy, &[7, 5]), svf);
}

/// A select's Member value, an assignment onto it, and a mapping on it are
/// written on the instance, and load back.
#[test]
fn select_member_round_trips_on_the_instance() {
    let (mut controller, mut hierarchy) = load(VOICE);
    let instance = ModuleId(7);
    hierarchy
        .set_mode(controller.graph(), instance, Mode::Select, &role_name)
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    let member = hierarchy.member_module(&ModulePath(vec![7])).unwrap();
    controller.set_param(member, 0, 1.0).unwrap();
    controller
        .add_assignment_named(ModuleId(11), "out", member, MEMBER_PARAM, 0.25)
        .unwrap();
    controller
        .map_midi_named(
            loom_core::MidiControl {
                channel: 0,
                controller: 5,
            },
            member,
            MEMBER_PARAM,
        )
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    let text = written(&hierarchy, &controller);
    assert!(text.contains(r#""member": 1.0"#), "{text}");
    assert!(text.contains(r#"target: (7, "member")"#), "{text}");
    let (again_controller, again) = load(&text);
    assert_eq!(written(&again, &again_controller), text);
}

/// The refusals of mode changes and of removing the last member.
#[test]
fn mode_changes_refuse_what_the_spec_refuses() {
    let (mut controller, mut hierarchy) = load(VOICE);
    let instance = ModuleId(7);
    hierarchy
        .set_mode(controller.graph(), instance, Mode::Layer, &role_name)
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    let member = hierarchy.members_of(instance)[0];
    assert_eq!(hierarchy.remove_refusal(member), Some(modes::last_member(Mode::Layer)));
    assert_eq!(
        hierarchy.level_refusal(&ModulePath(vec![7])),
        Some(modes::only_members(Mode::Layer))
    );
    assert_eq!(hierarchy.level_refusal(&ModulePath::root()), None);
    // A second member, then single is refused.
    let file = hierarchy
        .definition_file(controller.graph(), |_| Layout::default(), member)
        .unwrap();
    hierarchy
        .insert(&mut controller, &ModulePath(vec![7]), &file, None)
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    assert_eq!(hierarchy.members_of(instance).len(), 2);
    assert_eq!(
        hierarchy.set_mode(controller.graph(), instance, Mode::Single, &role_name),
        Err(modes::several_members("Voice", Mode::Single))
    );
    assert!(hierarchy.move_member(hierarchy.members_of(instance)[1], true));
    assert!(!hierarchy.move_member(hierarchy.members_of(instance)[0], true));
    // A role declared refuses a mode change.
    let key = hierarchy.instances[&ModulePath(vec![7])].key.clone();
    hierarchy.definitions.get_mut(&key).unwrap().roles = vec!["loom.role.filter@1".to_string()];
    assert_eq!(
        hierarchy.mode_refusal(instance, Mode::Select, &|_| "Filter".to_string()),
        Some(modes::fills_a_role("Voice", "Filter"))
    );
}

/// A layer whose one member is a stack cannot become single: the stack
/// would be lost when its inside moves up. The file stays as it was.
#[test]
fn leaving_a_layer_whose_member_has_a_mode_is_refused() {
    let (mut controller, mut hierarchy) = load(VOICE);
    let instance = ModuleId(7);
    hierarchy
        .set_mode(controller.graph(), instance, Mode::Layer, &role_name)
        .unwrap();
    commit(&mut hierarchy, &mut controller);
    let member = hierarchy.members_of(instance)[0];
    hierarchy
        .set_mode(controller.graph(), member, Mode::Stack, &role_name)
        .unwrap();
    hierarchy.set_copies(member, 4).unwrap();
    commit(&mut hierarchy, &mut controller);
    let before = written(&hierarchy, &controller);
    assert_eq!(
        hierarchy.set_mode(controller.graph(), instance, Mode::Single, &role_name),
        Err(modes::member_has_mode("Voice", "Voice", Mode::Single))
    );
    assert_eq!(written(&hierarchy, &controller), before);
}

/// An undo snapshot leaves implied modules out, and restoring one keeps
/// the implied ids, so copies keep their state.
#[test]
fn restoring_a_snapshot_keeps_implied_ids() {
    let (mut controller, mut hierarchy) = load(VOICE);
    let snapshot = hierarchy.clone();
    hierarchy
        .set_mode(controller.graph(), ModuleId(7), Mode::Stack, &role_name)
        .unwrap();
    hierarchy.set_copies(ModuleId(7), 2).unwrap();
    commit(&mut hierarchy, &mut controller);
    let copy = id(&hierarchy, &[7, COPY_STEP + 1, 5]);
    let mut restored = hierarchy.clone();
    restored.restore(&snapshot);
    assert_eq!(restored.ids().path_of(copy), hierarchy.ids().path_of(copy));
}
