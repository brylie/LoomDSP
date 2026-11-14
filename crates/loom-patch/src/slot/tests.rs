use super::*;
use crate::{Patch, PatchCable};
use loom_core::roles;

/// A patch with a Filter slot inside a Voice sub-patch: the spec's example
/// (`docs/design/sub-patches.md`, Patch file), with a held SVF and a held
/// sub-patch part.
pub(crate) const SLOT_FILE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Slots"),
    subpatches: {
        "comb": SubPatch(
            metadata: Metadata(name: "Comb"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 2, kind: "loom.time.delay@1", params: {"time": 0.002}),
                PatchModule(id: 3, kind: "loom.filter.onepole@1", params: {"cutoff": 500.0}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "in")),
                PatchCable(from: (2, "out"), to: (3, "in")),
                PatchCable(from: (3, "lowpass"), to: (4, "in")),
            ],
            exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff")],
            roles: ["loom.role.filter@1"],
        ),
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
                PatchModule(id: 3, kind: "loom.filter.ladder@1", params: {"cutoff": 800.0, "drive": 0.3}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 9, kind: "loom.struct.slot@1", label: Some("Filter"), slot: Some(Slot(
                    role: "loom.role.filter@1",
                    part: Some(3),
                    held: {
                        Module("loom.filter.svf@1"): Held(params: {"cutoff": 800.0, "resonance": 0.4}),
                        SubPatch("comb"): Held(params: {"cutoff": 800.0}),
                    },
                ))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (9, "in")),
                PatchCable(from: (9, "out"), to: (4, "in")),
            ],
            exposed: [Exposed(name: "cutoff", module: 9, param: "cutoff")],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"cutoff": 1200.0}),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 5, kind: "loom.clock.transport@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)
"#;

/// The spec's slot example parses, keeps its held parts by kind and by
/// definition key, and prints back to the same canonical text.
#[test]
fn the_slot_example_round_trips() {
    let patch = Patch::from_ron(SLOT_FILE).unwrap();
    let voice = &patch.subpatches["voice"];
    let slot = voice.modules.iter().find(|m| m.is_slot()).unwrap();
    let field = slot.slot.as_ref().unwrap();
    assert_eq!(field.role, "loom.role.filter@1");
    assert_eq!(field.part, Some(3));
    assert_eq!(field.held.len(), 2);
    assert_eq!(
        field.held[&PartKey::Module("loom.filter.svf@1".to_string())].params["resonance"],
        0.4
    );
    assert!(field.held.contains_key(&PartKey::SubPatch("comb".to_string())));
    assert_eq!(patch.subpatches["comb"].roles, ["loom.role.filter@1"]);
    let text = patch.to_ron().unwrap();
    assert_eq!(Patch::from_ron(&text).unwrap().to_ron().unwrap(), text);
    assert!(text.contains("Module(\"loom.filter.svf@1\"): Held("), "{text}");
}

/// A definition with the role's ports and exposed items fits; one missing
/// the exposed cutoff, or exposing it on a parameter of another unit, is
/// named (sub-patches.md, Roles).
#[test]
fn a_definition_fits_a_role_by_port_and_exposed_names() {
    let registry = loom_modules::registry();
    let patch = Patch::from_ron(SLOT_FILE).unwrap();
    let comb = &patch.subpatches["comb"];
    assert!(
        comb.role_mismatches(&roles::FILTER, &registry, &patch.subpatches)
            .is_empty()
    );
    assert!(comb.fills(&roles::FILTER, &registry, &patch.subpatches));
    assert!(
        !comb.fills(&roles::EFFECT, &registry, &patch.subpatches),
        "not declared"
    );
    assert!(
        comb.role_mismatches(&roles::EFFECT, &registry, &patch.subpatches)
            .is_empty()
    );

    let mut no_cutoff = comb.clone();
    no_cutoff.exposed.clear();
    let found = no_cutoff.role_mismatches(&roles::FILTER, &registry, &patch.subpatches);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].to_string(), "no exposed `cutoff`");

    let mut wrong_unit = comb.clone();
    wrong_unit.exposed[0] = Exposed {
        name: "cutoff".into(),
        module: 2,
        param: "time".into(),
        range: None,
    };
    let found = wrong_unit.role_mismatches(&roles::FILTER, &registry, &patch.subpatches);
    assert_eq!(found[0].to_string(), "exposed `cutoff` is in s, not Hz");

    let mut no_port = comb.clone();
    no_port.modules.retain(|m| m.id != 4);
    no_port.cables.retain(|c| c.to.0 != 4);
    let found = no_port.role_mismatches(&roles::FILTER, &registry, &patch.subpatches);
    assert_eq!(found[0].to_string(), "no output `out`");
}

/// Pruning drops a declared role the definition no longer fits, saying
/// why, and an unknown role, and keeps the rest.
#[test]
fn prune_drops_roles_that_no_longer_fit() {
    let registry = loom_modules::registry();
    let patch = Patch::from_ron(SLOT_FILE).unwrap();
    let mut comb = patch.subpatches["comb"].clone();
    comb.roles.push("loom.role.envelope@1".to_string());
    comb.roles.push("loom.role.future@1".to_string());
    let dropped = comb.prune_roles(&registry, &patch.subpatches);
    assert_eq!(comb.roles, ["loom.role.filter@1"]);
    assert_eq!(
        dropped,
        [
            (
                "loom.role.envelope@1".to_string(),
                "no input `gate`, no exposed `attack`, no exposed `release`".to_string()
            ),
            (
                "loom.role.future@1".to_string(),
                "this build does not know the role".to_string()
            ),
        ]
    );
}

/// An exposed item that reaches a slot has the role's kind, so a
/// definition that exposes a slot's cutoff fills Filter through it.
#[test]
fn an_exposed_slot_parameter_has_the_roles_kind() {
    let registry = loom_modules::registry();
    let mut patch = Patch::from_ron(SLOT_FILE).unwrap();
    let voice = patch.subpatches.get_mut("voice").unwrap();
    voice.roles.push("loom.role.filter@1".to_string());
    let voice = &patch.subpatches["voice"];
    assert!(
        voice
            .role_mismatches(&roles::FILTER, &registry, &patch.subpatches)
            .is_empty()
    );
    // Through a nested instance, the same.
    let mut outer = SubPatch::new("Outer");
    outer.modules.push(crate::PatchModule {
        id: 1,
        kind: crate::SUBPATCH_KIND.to_string(),
        subpatch: Some("voice".to_string()),
        slot: None,
        label: None,
        params: Default::default(),
        position: None,
        size: None,
        ranges: Default::default(),
    });
    outer.modules.push(crate::PatchModule {
        id: 2,
        kind: crate::INPUT_KIND.to_string(),
        subpatch: None,
        slot: None,
        label: Some("in".into()),
        params: Default::default(),
        position: None,
        size: None,
        ranges: Default::default(),
    });
    outer.modules.push(crate::PatchModule {
        id: 3,
        kind: crate::OUTPUT_KIND.to_string(),
        subpatch: None,
        slot: None,
        label: Some("out".into()),
        params: Default::default(),
        position: None,
        size: None,
        ranges: Default::default(),
    });
    outer.cables.push(PatchCable {
        from: (2, "out".into()),
        to: (1, "in".into()),
    });
    outer.exposed.push(Exposed {
        name: "cutoff".into(),
        module: 1,
        param: "cutoff".into(),
        range: None,
    });
    assert!(
        outer
            .role_mismatches(&roles::FILTER, &registry, &patch.subpatches)
            .is_empty()
    );
}

/// The swap fade's kind names the module ID that live patching rule 9
/// treats as a mixer (`loom_core::cable_fade`).
#[test]
fn the_fade_kind_names_the_core_mixer_id() {
    assert_eq!(FADE_KIND, format!("{}@1", loom_core::cable_fade::SWAP_FADE_MODULE));
}
