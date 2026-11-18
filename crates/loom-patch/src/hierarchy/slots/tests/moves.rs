//! Moving a slot across an editor boundary or copying it.

use super::*;

/// The slot, its held parts, its cables, and the audio all survive
/// moving it into and back out of another sub-patch.
#[test]
fn collapsing_and_dissolving_a_slot_preserves_sound_and_parts() {
    let (mut controller, mut hierarchy) = load(&Patch::from_ron(SLOT_FILE).unwrap());
    let level = ModulePath::top(2);
    let slot = inside(&hierarchy, 9);
    let part = hierarchy.part_of_slot(slot);
    let held = hierarchy.slot(slot).unwrap().held.clone();
    let graph_cables = cables(&controller);
    let original = written(&hierarchy, &controller);
    let before = render(&original);
    assert!(before.iter().any(|&sample| sample != 0.0));
    let collapsed = hierarchy.collapse(controller.graph(), &level, &[slot.0]).unwrap();
    assert_eq!(collapsed.members, [slot]);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(render(&written(&hierarchy, &controller)), before);
    let dissolved = hierarchy.dissolve(controller.graph(), collapsed.instance).unwrap();
    assert_eq!(dissolved.members, [slot]);
    hierarchy.sync(&mut controller).unwrap();
    assert_eq!(hierarchy.part_of_slot(slot), part);
    assert_eq!(hierarchy.slot(slot).unwrap().held, held);
    assert_eq!(cables(&controller), graph_cables);
    assert_eq!(render(&written(&hierarchy, &controller)), before);
}

/// Renders the patch from reset with no output limiter.
fn render(patch: &Patch) -> Vec<f32> {
    let config = EngineConfig {
        output_safety: loom_core::OutputSafety::Off,
        ..EngineConfig::default()
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    patch.load_into(&mut parts.controller).unwrap();
    let mut left = vec![0.0; 1024];
    let mut right = vec![0.0; 1024];
    for (left, right) in left.chunks_mut(64).zip(right.chunks_mut(64)) {
        parts.engine.process(left, right);
    }
    left
}

/// A copied slot rekeys its held sub-patch, so both slots can swap it in.
#[test]
fn copied_slots_own_their_held_definitions() {
    let patch = Patch::from_ron(SLOT_FILE).unwrap();
    let (mut controller, mut hierarchy) = load(&patch);
    let level = ModulePath::top(2);
    let voice = &patch.subpatches["voice"];
    let get = |id| voice.modules.iter().find(|module| module.id == id).unwrap().clone();
    let definitions = BTreeMap::from([("comb".to_string(), patch.subpatches["comb"].clone())]);
    let copy = hierarchy
        .insert_slot_copy(&mut controller, &level, get(9), Some(get(3)), definitions)
        .unwrap()
        .instance;
    hierarchy.sync(&mut controller).unwrap();
    let held_key = hierarchy
        .slot(copy)
        .unwrap()
        .held
        .keys()
        .find_map(|part| match part {
            PartKey::SubPatch(key) => Some(key.clone()),
            PartKey::Module(_) => None,
        })
        .unwrap();
    assert_ne!(held_key, "comb");
    let saved = written(&hierarchy, &controller);
    assert_eq!(saved.subpatches[&held_key], saved.subpatches["comb"]);
    let (mut controller, mut hierarchy) = load(&saved);
    let copy_id = saved.subpatches["voice"]
        .modules
        .iter()
        .find(|module| {
            module
                .slot
                .as_ref()
                .is_some_and(|slot| slot.held.contains_key(&PartKey::SubPatch(held_key.clone())))
        })
        .unwrap()
        .id;
    let copy = inside(&hierarchy, copy_id);
    let original = inside(&hierarchy, 9);
    let original_part = hierarchy.part_of_slot(original);
    hierarchy
        .swap_to_held_subpatch(&mut controller, &|_| Layout::default(), copy, &held_key)
        .unwrap();
    assert_eq!(hierarchy.part_of_slot(original), original_part);
}
