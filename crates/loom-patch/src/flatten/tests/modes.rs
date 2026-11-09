//! Flattening of sub-patch modes (sub-patches.md, Modes; ADR-0040).

use super::*;

/// [`VOICE`] with `fields` added to the `voice` definition.
fn voice_with(fields: &str) -> String {
    VOICE.replace(
        r#"metadata: Metadata(name: "Voice"),"#,
        &format!(r#"metadata: Metadata(name: "Voice"), {fields}"#),
    )
}

/// The flat cables of `flat`, as (from, to) ends.
fn cables(flat: &Flat) -> Vec<Ends<'_>> {
    flat.cables
        .iter()
        .map(|c| ((c.from.0, c.from.1.as_str()), (c.to.0, c.to.1.as_str())))
        .collect()
}

/// A stack of four copies: copy 0 keeps the paths a single sub-patch has,
/// copy k sits under a copy step, each outer input feeds every copy, and
/// each output is a Sum at 1/√4 that the outside reads.
#[test]
fn a_stack_runs_copies_summed_at_one_over_root_n() {
    let (single, single_ids) = flatten(VOICE);
    let (flat, ids) = flatten(&voice_with("mode: Stack, copies: 4,"));
    // Copy 0 has the ids a single sub-patch has.
    for inner in [2, 3, 5, 6] {
        assert_eq!(id(&ids, &[7, inner]), id(&single_ids, &[7, inner]));
    }
    assert_eq!(flat.modules.len(), single.modules.len() + 3 * 4 + 1);
    let phases: Vec<u32> = (0..4)
        .map(|k| {
            let path = if k == 0 { vec![7, 2] } else { vec![7, COPY_STEP + k, 2] };
            id(&ids, &path)
        })
        .collect();
    for phase in &phases {
        assert!(cables(&flat).contains(&((1, "pitch"), (*phase, "pitch"))), "{phase}");
    }
    let sum = module(&flat, &[7, ADDED_STEP]);
    assert_eq!(sum.module.descriptor.kind(), SUM_KIND);
    assert_eq!(sum.module.params[0], 0.5);
    assert!(cables(&flat).contains(&((sum.id.0, "out"), (9, "in"))));
    for k in 0..4_u32 {
        let path = if k == 0 { vec![7, 6] } else { vec![7, COPY_STEP + k, 6] };
        let input = format!("in{}", k + 1);
        assert!(cables(&flat).contains(&((id(&ids, &path), "out"), (sum.id.0, input.as_str()))));
    }
    assert_eq!(
        flat.added.get(&sum.id),
        Some(&Added::Output {
            instance: ModulePath(vec![7]),
            port: "out".to_string()
        })
    );
    // Every module of copy 0 knows its copies.
    assert_eq!(flat.copies[&ModuleId(id(&ids, &[7, 5]))].len(), 3);
    assert!(ModulePath(vec![7, COPY_STEP + 1, 5]).is_implied());
    assert!(!ModulePath(vec![7, 5]).is_implied());
}

/// One copy with no stereo spread is the single sub-patch, module for
/// module: a mode change alone never changes the sound.
#[test]
fn a_stack_of_one_is_the_single_sub_patch() {
    let (single, _) = flatten(VOICE);
    let (stack, _) = flatten(&voice_with("mode: Stack,"));
    assert_eq!(single.modules.len(), stack.modules.len());
    assert_eq!(cables(&single), cables(&stack));
    assert!(stack.added.is_empty() && stack.copies.is_empty());
}

/// An assignment from outside onto an exposed parameter reaches every copy,
/// and a MIDI mapping reaches copy 0 with the other copies as followers,
/// since a control has one mapping.
#[test]
fn assignments_and_mappings_into_a_stack_reach_every_copy() {
    let (flat, ids) = flatten(&voice_with("mode: Stack, copies: 3,"));
    let targets: Vec<u32> = flat
        .modulation
        .iter()
        .filter(|a| a.source == (11, "out".to_string()))
        .map(|a| a.target.0)
        .collect();
    let svfs = [
        id(&ids, &[7, 5]),
        id(&ids, &[7, COPY_STEP + 1, 5]),
        id(&ids, &[7, COPY_STEP + 2, 5]),
    ];
    assert_eq!(targets, svfs);
    assert_eq!(flat.midi.len(), 1);
    let cutoff = (ModuleId(svfs[0]), "cutoff".to_string());
    assert_eq!(flat.midi[0].target, (svfs[0], "cutoff".to_string()));
    assert_eq!(
        flat.followers[&cutoff],
        vec![
            (ModuleId(svfs[1]), "cutoff".to_string()),
            (ModuleId(svfs[2]), "cutoff".to_string())
        ]
    );
}

/// A mapping inside a stack becomes copy 0's, with followers, so the file
/// loads into a controller, which allows one mapping per control.
#[test]
fn a_mapping_inside_a_stack_loads_as_one_mapping_with_followers() {
    let text = voice_with(
        r#"mode: Stack, copies: 2, midi: [PatchMidiMapping(target: (6, "gain"), channel: 0, controller: 9)],"#,
    );
    let (flat, ids) = flatten(&text);
    let mul = (ModuleId(id(&ids, &[7, 6])), "gain".to_string());
    assert_eq!(
        flat.followers[&mul],
        vec![(ModuleId(id(&ids, &[7, COPY_STEP + 1, 6])), "gain".to_string())]
    );
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let controller = &mut parts.controller;
    Patch::from_ron(&text).unwrap().load_into(controller).unwrap();
    // The inside mapping and the outer one on the exposed cutoff.
    assert_eq!(controller.graph().midi_mappings().count(), 2);
    let index = controller.graph().node(mul.0).unwrap().parameter_index("gain").unwrap();
    assert_eq!(controller.graph().midi_followers(mul.0, index).len(), 1);
}

/// Spread is hidden modulation from one constant: copy k's parameter gets
/// depth `amount × offset(k)`, offsets even from -1 to 1, the middle copy of
/// an odd count none.
#[test]
fn spread_is_modulation_from_a_hidden_constant() {
    let (flat, ids) = flatten(&voice_with(r#"mode: Stack, copies: 3, spread: {"cutoff": 0.3},"#));
    let source = module(&flat, &[7, ADDED_STEP + SPREAD_STEP]);
    assert_eq!(source.module.descriptor.kind(), SPREAD_SOURCE_KIND);
    assert_eq!(source.module.params[0], 1.0);
    let spread: Vec<(u32, f32)> = flat
        .modulation
        .iter()
        .filter(|a| a.source.0 == source.id.0)
        .map(|a| (a.target.0, a.depth))
        .collect();
    assert_eq!(
        spread,
        vec![(id(&ids, &[7, 5]), -0.3), (id(&ids, &[7, COPY_STEP + 2, 5]), 0.3)]
    );
    assert_eq!(
        flat.added[&source.id],
        Added::Spread {
            instance: ModulePath(vec![7])
        }
    );
}

/// Stereo spread puts a stereo spread sum on each output, even for one copy.
#[test]
fn stereo_spread_adds_a_stereo_sum() {
    let (flat, _) = flatten(&voice_with("mode: Stack, copies: 4, stereo_spread: true,"));
    let sum = module(&flat, &[7, ADDED_STEP]);
    assert_eq!(sum.module.descriptor.kind(), SPREAD_SUM_KIND);
    assert_eq!(sum.module.params[0], 4.0);
    let (one, _) = flatten(&voice_with("mode: Stack, stereo_spread: true,"));
    assert_eq!(module(&one, &[7, ADDED_STEP]).module.params[0], 1.0);
}

/// Copy k of a seeded module plays seed + k, wrapped in the seed's range.
#[test]
fn seeds_differ_per_copy() {
    let text = voice_with(r#"mode: Stack, copies: 3,"#).replace(
        r#"PatchModule(id: 5, kind: "loom.filter.svf@1", params: {"cutoff": 1000.0}),"#,
        r#"PatchModule(id: 5, kind: "loom.filter.svf@1", params: {"cutoff": 1000.0}), PatchModule(id: 8, kind: "loom.osc.noise@1", params: {"seed": 65535.0}),"#,
    );
    let (flat, ids) = flatten(&text);
    let seeds: Vec<f32> = [vec![7, 8], vec![7, COPY_STEP + 1, 8], vec![7, COPY_STEP + 2, 8]]
        .iter()
        .map(|path| module(&flat, path).module.params[0])
        .collect();
    assert_eq!(seeds, vec![65535.0, 1.0, 2.0]);
    let copies = &flat.copies[&ModuleId(id(&ids, &[7, 8]))];
    assert_eq!(copies.iter().map(|(_, offset)| *offset).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(seeded(1.0, 65_535.0, 7.0, 3), 10.0);
    assert_eq!(seeded(0.0, 0.0, 0.0, 5), 0.0);
}

/// Raising Copies keeps every existing copy's ids, so their state survives
/// (rule 4).
#[test]
fn raising_copies_keeps_the_ids_of_existing_copies() {
    let patch = |copies: u32| Patch::from_ron(&voice_with(&format!("mode: Stack, copies: {copies},"))).unwrap();
    let registry = loom_modules::registry();
    let mut ids = FlatIds::new();
    patch(2).flatten(&registry, &mut ids).unwrap();
    let before = (
        id(&ids, &[7, 5]),
        id(&ids, &[7, COPY_STEP + 1, 5]),
        id(&ids, &[7, ADDED_STEP]),
    );
    patch(4).flatten(&registry, &mut ids).unwrap();
    assert_eq!(
        before,
        (
            id(&ids, &[7, 5]),
            id(&ids, &[7, COPY_STEP + 1, 5]),
            id(&ids, &[7, ADDED_STEP])
        )
    );
}

/// Two member definitions for a layer or select: `a` with ports `pitch` and
/// `out`, `b` with `out` and a gate output `trig`.
const MEMBERS: &str = r#"
        "a": SubPatch(
            metadata: Metadata(name: "Saw"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
                PatchModule(id: 2, kind: "loom.phase.generator@1"),
                PatchModule(id: 3, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 4, kind: "loom.osc.sine@1"),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "pitch")),
                PatchCable(from: (2, "phase"), to: (4, "phase")),
                PatchCable(from: (4, "out"), to: (3, "in")),
            ],
        ),
        "b": SubPatch(
            metadata: Metadata(name: "Clock"),
            modules: [
                PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 0.25}),
                PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
                PatchModule(id: 3, kind: "loom.struct.output@1", label: Some("trig")),
                PatchModule(id: 4, kind: "loom.clock.clock@1"),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "in")),
                PatchCable(from: (4, "gate"), to: (3, "in")),
            ],
        ),"#;

/// A patch with a layer or select instance 7 of members `a` and `b`, fed
/// from a note input and read by the audio output.
fn members_patch(mode: &str, instance: &str) -> String {
    format!(
        r#"Patch(
    version: 1,
    subpatches: {{
        "layer": SubPatch(
            metadata: Metadata(name: "Layer"),
            mode: {mode},
            members: [1, 2],
            modules: [
                PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("a")),
                PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("b")),
            ],
        ),{MEMBERS}
    }},
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("layer"){instance}),
        PatchModule(id: 9, kind: "loom.io.audio-out@1"),
        PatchModule(id: 11, kind: "loom.mod.lfo@1"),
        PatchModule(id: 12, kind: "loom.math.mul@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (7, "pitch")),
        PatchCable(from: (7, "out"), to: (9, "in")),
        PatchCable(from: (7, "trig"), to: (12, "a")),
    ],
)"#
    )
}

/// A layer's ports are its members' by name; an input feeds every member
/// that has it, and each output is a Sum of the members that have it.
#[test]
fn a_layer_sums_its_members() {
    let (flat, ids) = flatten(&members_patch("Layer", ""));
    let phase = id(&ids, &[7, 1, 2]);
    let sine = id(&ids, &[7, 1, 4]);
    assert!(cables(&flat).contains(&((1, "pitch"), (phase, "pitch"))));
    let out = module(&flat, &[7, ADDED_STEP]);
    assert_eq!(out.module.descriptor.kind(), SUM_KIND);
    assert_eq!(out.module.params[0], 1.0);
    let constant = id(&ids, &[7, 2, 1]);
    assert!(cables(&flat).contains(&((sine, "out"), (out.id.0, "in1"))));
    assert!(cables(&flat).contains(&((constant, "out"), (out.id.0, "in2"))));
    assert!(cables(&flat).contains(&((out.id.0, "out"), (9, "in"))));
    // `trig` only `b` has: it comes second in the port order, and the Sum
    // has nothing on its first input.
    let trig = module(&flat, &[7, ADDED_STEP + 1]);
    assert!(cables(&flat).contains(&((trig.id.0, "out"), (12, "a"))));
    assert!(!cables(&flat).iter().any(|(_, to)| *to == (trig.id.0, "in1")));
}

/// A select adds a Member module holding the instance's value, clamped to
/// the member count, one Select per output reading it, and fade off for a
/// gate output. An assignment or mapping on `member` reaches the Member
/// module, and MIDI sweeps 1 to the member count.
#[test]
fn a_select_reads_one_member_module() {
    let text = members_patch("Select", r#", params: {"member": 9.0}"#).replace(
        "    ],\n)",
        r#"    ],
    modulation: [PatchModulation(source: (11, "out"), target: (7, "member"), depth: 0.5)],
    midi: [PatchMidiMapping(target: (7, "member"), channel: 0, controller: 3)],
)"#,
    );
    let (flat, _) = flatten(&text);
    let member = module(&flat, &[7, ADDED_STEP + MEMBER_STEP]);
    assert_eq!(member.module.descriptor.kind(), MEMBER_KIND);
    assert_eq!(member.module.params[0], 2.0, "clamped to two members");
    let out = module(&flat, &[7, ADDED_STEP]);
    let trig = module(&flat, &[7, ADDED_STEP + 1]);
    assert_eq!(out.module.descriptor.kind(), SELECT_KIND);
    let fade = out.module.descriptor.param_index("fade").unwrap();
    assert_eq!(out.module.params[fade], 1.0);
    assert_eq!(trig.module.params[fade], 0.0, "a gate switches on the frame");
    for select in [out.id, trig.id] {
        assert!(cables(&flat).contains(&((member.id.0, "out"), (select.0, "member"))));
    }
    assert!(
        flat.modulation
            .iter()
            .any(|a| a.target == (member.id.0, "member".to_string()))
    );
    assert_eq!(flat.midi[0].target, (member.id.0, "member".to_string()));
    assert_eq!(flat.limits[&(member.id, "member".to_string())], (1.0, 2.0));
    assert_eq!(
        flat.added[&member.id],
        Added::Member {
            instance: ModulePath(vec![7])
        }
    );
}

/// A layer of one member is the member, with nothing added.
#[test]
fn a_layer_of_one_member_adds_nothing() {
    let text = members_patch("Layer", "")
        .replace("members: [1, 2],", "members: [1],")
        .replace(
            r#"                PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("b")),
"#,
            "",
        )
        .replace(r#"        PatchCable(from: (7, "trig"), to: (12, "a")),"#, "");
    let (flat, _) = flatten(&text);
    assert!(flat.added.is_empty());
}

/// A mode whose fields do not fit is refused, naming the sub-patch, and so
/// is a level id in the reserved range.
#[test]
fn bad_mode_fields_and_reserved_ids_are_refused() {
    let error = refuse(&voice_with("copies: 4,"));
    assert!(error.to_string().contains("Voice"), "{error}");
    assert!(error.to_string().contains("no copies"), "{error}");
    let error = refuse(
        &VOICE
            .replace("PatchModule(id: 6,", "PatchModule(id: 3758096384,")
            .replace("(6, ", "(3758096384, "),
    );
    assert!(error.to_string().contains("3758096384"), "{error}");
}
