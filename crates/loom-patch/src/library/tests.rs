use super::*;
use crate::{FlatIds, Hierarchy, SUBPATCH_KIND};
use loom_core::{EngineConfig, EngineParts, create_engine};

/// The registry every test resolves with.
fn registry() -> std::sync::Arc<Registry> {
    loom_modules::registry()
}

/// A library item with a Transport and a Scale and tuning module: the
/// Transport's beat phase drives two sines, a multiply drives the
/// Transport's `run`, and the scale module's pitch feeds a phase
/// generator. The Transport's tempo is also modulated from inside.
const ARP: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Arp", authors: ["Ada"]),
    modules: [
        PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("gate"), position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.clock.transport@1", position: Some((0.0, 200.0))),
        PatchModule(id: 3, kind: "loom.osc.sine@1", position: Some((200.0, 0.0))),
        PatchModule(id: 4, kind: "loom.osc.sine@1", position: Some((200.0, 100.0))),
        PatchModule(id: 5, kind: "loom.math.mul@1", position: Some((400.0, 0.0))),
        PatchModule(id: 6, kind: "loom.struct.output@1", label: Some("out"), position: Some((600.0, 0.0))),
        PatchModule(id: 7, kind: "loom.pitch.scale@1", position: Some((0.0, 300.0))),
        PatchModule(id: 8, kind: "loom.phase.generator@1", position: Some((200.0, 300.0))),
    ],
    cables: [
        PatchCable(from: (2, "beat-phase"), to: (3, "phase")),
        PatchCable(from: (2, "beat-phase"), to: (4, "phase")),
        PatchCable(from: (3, "out"), to: (5, "a")),
        PatchCable(from: (4, "out"), to: (5, "b")),
        PatchCable(from: (5, "out"), to: (2, "run")),
        PatchCable(from: (5, "out"), to: (6, "in")),
        PatchCable(from: (7, "pitch"), to: (8, "pitch")),
    ],
    modulation: [
        PatchModulation(source: (2, "beat"), target: (5, "gain"), depth: 0.5),
        PatchModulation(source: (3, "out"), target: (2, "tempo"), depth: 0.1),
    ],
    midi: [
        PatchMidiMapping(target: (2, "tempo"), channel: 0, controller: 7),
    ],
    exposed: [Exposed(name: "gain", module: 5, param: "gain")],
)"#;

/// A library item with no patch-wide module: a pitch port into a sine,
/// through a multiply whose gain is exposed, out through a port.
const VOICE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Voice"),
    modules: [
        PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
        PatchModule(id: 2, kind: "loom.phase.generator@1"),
        PatchModule(id: 3, kind: "loom.osc.sine@1"),
        PatchModule(id: 4, kind: "loom.math.mul@1", params: {"gain": 0.5}),
        PatchModule(id: 5, kind: "loom.struct.output@1", label: Some("out")),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "pitch")),
        PatchCable(from: (2, "phase"), to: (3, "phase")),
        PatchCable(from: (3, "out"), to: (4, "a")),
        PatchCable(from: (4, "out"), to: (5, "in")),
    ],
    exposed: [Exposed(name: "gain", module: 4, param: "gain")],
)"#;

/// A patch whose Voice is linked to `Voices/Voice.ron`, with a stored
/// copy that differs from [`VOICE`] in its gain and in an exposed name
/// the file does not have.
const LINKED: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Linked"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
                PatchModule(id: 2, kind: "loom.phase.generator@1"),
                PatchModule(id: 3, kind: "loom.osc.sine@1"),
                PatchModule(id: 4, kind: "loom.math.mul@1", params: {"gain": 0.9}),
                PatchModule(id: 5, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "pitch")),
                PatchCable(from: (2, "phase"), to: (3, "phase")),
                PatchCable(from: (3, "out"), to: (4, "a")),
                PatchCable(from: (4, "out"), to: (5, "in")),
            ],
            exposed: [
                Exposed(name: "gain", module: 4, param: "gain"),
                Exposed(name: "tune", module: 2, param: "tune"),
            ],
            library: Some("Voices/Voice.ron"),
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1"),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"gain": 0.25, "tune": 0.2}),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 4, kind: "loom.clock.transport@1"),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

fn patch(text: &str) -> Patch {
    Patch::from_ron(text).unwrap()
}

/// A library holding `items` by path.
fn library(items: &[(&str, &str)]) -> BTreeMap<String, Patch> {
    items
        .iter()
        .map(|(path, text)| (path.to_string(), patch(text)))
        .collect()
}

/// Loads `patch` into a fresh engine, proving it flattens and plays.
fn loads(patch: &Patch) -> (loom_core::Controller, Hierarchy) {
    let EngineParts { mut controller, .. } = create_engine(registry(), EngineConfig::default());
    let mut ids = FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).expect("the patch loads");
    let hierarchy = Hierarchy::from_patch(patch, controller.graph(), ids).expect("the hierarchy reads");
    (controller, hierarchy)
}

/// The modules of `kind` in a definition.
fn of_kind<'a>(definition: &'a SubPatch, kind: &str) -> Vec<&'a PatchModule> {
    definition.modules.iter().filter(|m| m.kind == kind).collect()
}

/// A file with a Transport and a Scale and tuning module becomes a
/// definition without them: each cable from one is a Patch input named
/// after the inner input it fed, shared by the two sines, each cable into
/// one is a Patch output, an assignment from one exposes its target, an
/// assignment into one gets a Patch output, and its MIDI mapping goes
/// (sub-patches.md, Library; rule 8).
#[test]
fn from_patch_leaves_patch_wide_modules_out_and_turns_their_cables_into_ports() {
    let file = patch(ARP);
    let (definition, left_out) = SubPatch::from_patch(&file, &registry(), Some("Arp.ron"));
    assert_eq!(definition.metadata, file.metadata);
    assert_eq!(definition.library.as_deref(), Some("Arp.ron"));
    assert!(!definition.modules.iter().any(|m| is_patch_wide(&m.kind)));
    assert!(definition.midi.is_empty(), "the mapping on the Transport goes");
    assert!(
        definition.modulation.is_empty(),
        "both assignments touched a patch-wide module"
    );

    let inputs = of_kind(&definition, INPUT_KIND);
    let names: Vec<&str> = inputs.iter().map(|m| m.label.as_deref().unwrap()).collect();
    // The gate port the file had, the beat phase shared by both sines
    // (named after the sine's input), and the scale's pitch.
    assert_eq!(names, ["gate", "Phase", "Pitch"]);
    let phase = inputs[1].id;
    assert!(inputs.iter().all(|m| m.id == 1 || m.id > 8), "new ports take fresh ids");
    let fed_by_phase: Vec<u32> = definition
        .cables
        .iter()
        .filter(|c| c.from == (phase, "out".to_string()))
        .map(|c| c.to.0)
        .collect();
    assert_eq!(fed_by_phase, [3, 4]);
    assert!(
        inputs[1].position.is_some_and(|(x, _)| x < 0.0),
        "a new input port stands left of the modules"
    );

    let outputs = of_kind(&definition, OUTPUT_KIND);
    let names: Vec<&str> = outputs.iter().map(|m| m.label.as_deref().unwrap()).collect();
    // The out port the file had, one for the multiply's output into `run`,
    // and one for the sine's output whose assignment moved `tempo`.
    assert_eq!(names, ["out", "Out", "Out 2"]);
    assert_eq!(
        definition
            .cables
            .iter()
            .filter(|c| c.to == (outputs[1].id, "in".to_string()))
            .count(),
        1
    );
    // The port made for the assignment's source is fed by that source,
    // once, as collapsing would feed it.
    let into_out_2: Vec<&PatchCable> = definition
        .cables
        .iter()
        .filter(|c| c.to == (outputs[2].id, "in".to_string()))
        .collect();
    assert_eq!(into_out_2.len(), 1);
    assert_eq!(into_out_2[0].from, (3, "out".to_string()));
    assert!(
        !definition
            .cables
            .iter()
            .any(|c| c.from.0 == 2 || c.to.0 == 2 || c.from.0 == 7),
        "no cable touches a dropped module"
    );
    // The assignment from the Transport's beat onto the gain exposes the
    // gain, which the file exposed already, so nothing is added twice.
    assert_eq!(definition.exposed.len(), 1);

    assert_eq!(left_out.len(), 2);
    assert_eq!(left_out[0].kind, TRANSPORT_MODULE);
    assert_eq!(left_out[0].ports, ["Phase", "Out", "Out 2"]);
    assert_eq!(
        left_out[0].warning("Arp").to_string(),
        "Arp: its Transport was left out; connect its clock ports."
    );
    assert_eq!(left_out[1].kind, SCALE_MODULE);
    assert_eq!(
        left_out[1].warning("Arp").to_string(),
        "Arp: its Scale and tuning was left out; connect its pitch ports."
    );
    // The definition flattens: a patch with one instance of it loads.
    let mut nested = Patch::new("Test");
    nested.subpatches.insert("arp".to_string(), definition);
    nested.modules.push(PatchModule {
        id: 1,
        kind: SUBPATCH_KIND.to_string(),
        subpatch: Some("arp".to_string()),
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    loads(&nested);
}

/// A patch-wide module with no cable leaves no port behind, and its note
/// says only that it was left out.
#[test]
fn a_patch_wide_module_with_no_cables_leaves_no_port() {
    let mut file = patch(VOICE);
    file.modules.push(PatchModule {
        id: 9,
        kind: "loom.clock.transport@1".to_string(),
        subpatch: None,
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    let (definition, left_out) = SubPatch::from_patch(&file, &registry(), None);
    assert_eq!(definition.modules.len(), 5);
    assert_eq!(left_out.len(), 1);
    assert!(left_out[0].ports.is_empty());
    assert_eq!(
        left_out[0].warning("Voice").to_string(),
        "Voice: its Transport was left out."
    );
}

/// A file without patch-wide modules becomes the definition as it is,
/// and written back as a file it is the same text: Save to library and
/// Insert are inverses.
#[test]
fn from_patch_and_from_definition_are_inverses() {
    let file = patch(VOICE);
    let (definition, left_out) = SubPatch::from_patch(&file, &registry(), None);
    assert!(left_out.is_empty());
    assert_eq!(definition.modules, file.modules);
    assert_eq!(definition.cables, file.cables);
    assert_eq!(definition.exposed, file.exposed);
    assert_eq!(definition.library, None);
    let back = Patch::from_definition(&definition, &BTreeMap::new());
    assert_eq!(back.to_ron().unwrap(), file.to_ron().unwrap());
}

/// A file written from a definition carries the definitions nested in it,
/// and only those.
#[test]
fn from_definition_carries_the_nested_definitions() {
    let mut inner = SubPatch::new("Inner");
    inner.library = Some("Inner.ron".to_string());
    let mut outer = SubPatch::new("Outer");
    outer.modules.push(PatchModule {
        id: 1,
        kind: SUBPATCH_KIND.to_string(),
        subpatch: Some("inner".to_string()),
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    let table: BTreeMap<String, SubPatch> = [
        ("inner".to_string(), inner.clone()),
        ("other".to_string(), SubPatch::new("Other")),
    ]
    .into_iter()
    .collect();
    let file = Patch::from_definition(&outer, &table);
    assert_eq!(file.metadata.name, "Outer");
    assert_eq!(file.subpatches.len(), 1);
    assert_eq!(file.subpatches["inner"], inner, "a nested link travels with the file");
    assert_eq!(reachable_keys(&outer, &table), ["inner"]);
}

/// On load, a linked definition whose library file differs is replaced:
/// the instance keeps its exposed values by name, drops the one whose
/// name is gone, and the note names the sub-patch (sub-patches.md, Linked
/// library items). The refreshed patch loads.
#[test]
fn refresh_replaces_a_differing_copy_and_keeps_exposed_values_by_name() {
    let mut linked = patch(LINKED);
    let library = library(&[("Voices/Voice.ron", VOICE)]);
    let notes = linked.refresh_linked(&library, &registry()).unwrap();
    assert_eq!(
        notes.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["Updated from the library: Voice."]
    );
    let voice = &linked.subpatches["voice"];
    assert_eq!(voice.library.as_deref(), Some("Voices/Voice.ron"));
    assert_eq!(
        voice.modules[3].params["gain"], 0.5,
        "the file's copy replaced the stored one"
    );
    assert_eq!(voice.exposed.len(), 1);
    let instance = linked.modules.iter().find(|m| m.id == 2).unwrap();
    assert_eq!(
        instance.params,
        [("gain".to_string(), 0.25)].into_iter().collect(),
        "the value kept by name, the one whose name is gone dropped"
    );
    let (controller, hierarchy) = loads(&linked);
    assert_eq!(hierarchy.library_of(loom_core::ModuleId(2)), Some("Voices/Voice.ron"));
    // The instance value reached the inner gain.
    let mul = hierarchy.id_at(&crate::ModulePath::top(2), 4).unwrap();
    let node = controller.graph().node(mul).unwrap();
    use loom_core::ModuleNames;
    let gain = node.parameter_index("gain").unwrap();
    assert_eq!(node.descriptor.saved_values(&node.params)[gain], 0.25);
}

/// A library file equal to the stored copy changes nothing and says
/// nothing, whatever keys its nested definitions have.
#[test]
fn refresh_leaves_an_equal_copy_alone() {
    let mut linked = patch(LINKED);
    let (same, _) = SubPatch::from_patch(&patch(VOICE), &registry(), Some("Voices/Voice.ron"));
    linked.subpatches.insert("voice".to_string(), same.clone());
    let library = library(&[("Voices/Voice.ron", VOICE)]);
    let notes = linked.refresh_linked(&library, &registry()).unwrap();
    assert!(notes.is_empty());
    assert_eq!(linked.subpatches["voice"], same);
}

/// Two copies of one file compare equal under any nested keys, and differ
/// when a nested definition differs.
#[test]
fn fingerprints_ignore_nested_keys_and_see_nested_changes() {
    let mut inner = SubPatch::new("Inner");
    let mut outer = SubPatch::new("Outer");
    outer.modules.push(PatchModule {
        id: 1,
        kind: SUBPATCH_KIND.to_string(),
        subpatch: Some("a".to_string()),
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    let table_a: BTreeMap<String, SubPatch> = [("a".to_string(), inner.clone())].into_iter().collect();
    let mut outer_b = outer.clone();
    outer_b.modules[0].subpatch = Some("b".to_string());
    let table_b: BTreeMap<String, SubPatch> = [("b".to_string(), inner.clone())].into_iter().collect();
    assert_eq!(fingerprint(&outer, &table_a), fingerprint(&outer_b, &table_b));
    inner.metadata.name = "Changed".to_string();
    let table_c: BTreeMap<String, SubPatch> = [("b".to_string(), inner)].into_iter().collect();
    assert_ne!(fingerprint(&outer, &table_a), fingerprint(&outer_b, &table_c));
}

/// A missing library file, or one that does not load, leaves the stored
/// copy and says so (rule 5: a patch always opens).
#[test]
fn a_missing_or_unreadable_library_file_keeps_the_stored_copy_with_a_note() {
    let mut linked = patch(LINKED);
    let before = linked.clone();
    let notes = linked.refresh_linked(&NoLibrary, &registry()).unwrap();
    assert_eq!(linked, before);
    assert_eq!(
        notes.iter().map(ToString::to_string).collect::<Vec<_>>(),
        ["Voice: the library file \"Voices/Voice.ron\" is missing, so its stored copy is used."]
    );

    /// A library whose every file is unreadable.
    struct Broken;
    impl Library for Broken {
        fn read(&self, _: &str) -> Result<Patch, LibraryError> {
            Err(LibraryError::Unreadable("invalid patch".to_string()))
        }
    }
    let notes = linked.refresh_linked(&Broken, &registry()).unwrap();
    assert_eq!(linked, before);
    assert_eq!(
        notes[0].to_string(),
        "Voice: the library file \"Voices/Voice.ron\" does not load (invalid patch), so its stored copy is used."
    );
    assert!(!loads(&linked).1.is_flat());
}

/// A library file that gained a Transport is refreshed without it: its
/// cables become ports, and the note says so (Linked library items; rule 8).
#[test]
fn refresh_drops_a_transport_the_file_gained_with_a_note() {
    let mut linked = patch(LINKED);
    let mut voice = patch(VOICE);
    voice.modules.push(PatchModule {
        id: 9,
        kind: "loom.clock.transport@1".to_string(),
        subpatch: None,
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    voice.cables.push(PatchCable {
        from: (9, "beat-phase".to_string()),
        to: (3, "phase".to_string()),
    });
    let library: BTreeMap<String, Patch> = [("Voices/Voice.ron".to_string(), voice)].into_iter().collect();
    let notes = linked.refresh_linked(&library, &registry()).unwrap();
    assert_eq!(
        notes.iter().map(ToString::to_string).collect::<Vec<_>>(),
        [
            "Voice: its Transport was left out; connect its clock ports.",
            "Updated from the library: Voice.",
        ]
    );
    let voice = &linked.subpatches["voice"];
    assert_eq!(of_kind(voice, INPUT_KIND).len(), 2);
    // The file listed two cables into the sine's phase; as a flat patch it
    // played the later, from the Transport, so the port's cable is the one
    // kept and the phase generator's is gone.
    let into_phase: Vec<&PatchCable> = voice
        .cables
        .iter()
        .filter(|c| c.to == (3, "phase".to_string()))
        .collect();
    assert_eq!(into_phase.len(), 1);
    assert_ne!(into_phase[0].from.0, 2);
    let (controller, _) = loads(&linked);
    assert_eq!(
        controller
            .graph()
            .nodes()
            .filter(|(_, n)| n.descriptor.id == TRANSPORT_MODULE)
            .count(),
        1,
        "the patch still has one Transport"
    );
}

/// A file whose nested definition has a key the patch already uses comes
/// in under a free key, with its instance pointing at it.
#[test]
fn refresh_gives_the_file_s_nested_definitions_free_keys() {
    let mut linked = patch(LINKED);
    let mut other = SubPatch::new("Other");
    other.modules.push(PatchModule {
        id: 1,
        kind: "loom.osc.sine@1".to_string(),
        subpatch: None,
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    linked.subpatches.insert("filter".to_string(), other.clone());
    linked.modules.push(PatchModule {
        id: 5,
        kind: SUBPATCH_KIND.to_string(),
        subpatch: Some("filter".to_string()),
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    let mut voice = patch(VOICE);
    voice.subpatches.insert("filter".to_string(), SubPatch::new("Filter"));
    voice.modules.push(PatchModule {
        id: 6,
        kind: SUBPATCH_KIND.to_string(),
        subpatch: Some("filter".to_string()),
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    let library: BTreeMap<String, Patch> = [("Voices/Voice.ron".to_string(), voice)].into_iter().collect();
    linked.refresh_linked(&library, &registry()).unwrap();
    assert_eq!(linked.subpatches["filter"], other, "the patch's own stays");
    assert_eq!(linked.subpatches["filter 2"].metadata.name, "Filter");
    let nested = linked.subpatches["voice"].modules.iter().find(|m| m.id == 6).unwrap();
    assert_eq!(nested.subpatch.as_deref(), Some("filter 2"));
    loads(&linked);
}

/// A chain of links that comes back to a file on the way in is refused,
/// naming the sub-patches along the loop (sub-patches.md, Patch file).
#[test]
fn a_link_loop_is_refused_naming_the_loop() {
    let mut a = patch(VOICE);
    a.metadata.name = "A".to_string();
    let mut b_def = SubPatch::new("B");
    b_def.library = Some("B.ron".to_string());
    a.subpatches.insert("b".to_string(), b_def);
    a.modules.push(PatchModule {
        id: 9,
        kind: SUBPATCH_KIND.to_string(),
        subpatch: Some("b".to_string()),
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    let mut b = Patch::new("B");
    let mut a_def = SubPatch::new("A");
    a_def.library = Some("A.ron".to_string());
    b.subpatches.insert("a".to_string(), a_def);
    b.modules.push(PatchModule {
        id: 1,
        kind: SUBPATCH_KIND.to_string(),
        subpatch: Some("a".to_string()),
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    let library: BTreeMap<String, Patch> = [("A.ron".to_string(), a), ("B.ron".to_string(), b)]
        .into_iter()
        .collect();
    let mut linked = patch(LINKED);
    linked.subpatches.get_mut("voice").unwrap().library = Some("A.ron".to_string());
    let error = linked.refresh_linked(&library, &registry()).unwrap_err();
    assert_eq!(error.to_string(), "sub-patch Voice › B › A contains itself");
    // A file that links to itself is the shortest loop.
    let mut selfish = patch(VOICE);
    let mut me = SubPatch::new("Me");
    me.library = Some("Voices/Voice.ron".to_string());
    selfish.subpatches.insert("me".to_string(), me);
    selfish.modules.push(PatchModule {
        id: 9,
        kind: SUBPATCH_KIND.to_string(),
        subpatch: Some("me".to_string()),
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    let library: BTreeMap<String, Patch> = [("Voices/Voice.ron".to_string(), selfish)].into_iter().collect();
    let error = patch(LINKED).refresh_linked(&library, &registry()).unwrap_err();
    assert_eq!(error.to_string(), "sub-patch Voice › Me contains itself");
}

/// The Transport and the Scale and tuning module are patch-wide at any
/// version; nothing else is.
#[test]
fn patch_wide_kinds() {
    assert!(is_patch_wide("loom.clock.transport@1"));
    assert!(is_patch_wide("loom.clock.transport@7"));
    assert!(is_patch_wide(SCALE_MODULE));
    assert!(!is_patch_wide("loom.osc.sine@1"));
    assert!(!is_patch_wide(INPUT_KIND));
    assert!(!patch(VOICE).has_links());
    assert!(patch(LINKED).has_links());
}

/// A library file that renamed its output port and its exposed parameter
/// is taken, and everything in the patch that named the old port or name
/// goes with a note: the cable out of the tile, the assignment and the
/// MIDI mapping onto the exposed name, and the instance's value. The
/// patch still loads (sub-patches.md, Linked library items; rule 5).
#[test]
fn refresh_drops_what_names_a_port_or_exposed_name_the_file_no_longer_has() {
    let mut linked = patch(LINKED);
    linked.modules.push(PatchModule {
        id: 5,
        kind: "loom.mod.lfo@1".to_string(),
        subpatch: None,
        label: None,
        params: BTreeMap::new(),
        position: None,
        size: None,
        ranges: BTreeMap::new(),
    });
    linked.modulation.push(crate::PatchModulation {
        source: (5, "out".to_string()),
        target: (2, "gain".to_string()),
        depth: 0.5,
        polarity: None,
        enabled: true,
    });
    linked.midi.push(crate::PatchMidiMapping {
        target: (2, "gain".to_string()),
        channel: 0,
        controller: 7,
        mode: crate::PatchMidiMode::Absolute,
        range: (0.0, 1.0),
        invert: false,
    });
    loads(&linked);
    let renamed = VOICE
        .replace("label: Some(\"out\")", "label: Some(\"audio\")")
        .replace("Exposed(name: \"gain\"", "Exposed(name: \"level\"");
    let library = library(&[("Voices/Voice.ron", &renamed)]);
    let notes = linked.refresh_linked(&library, &registry()).unwrap();
    assert_eq!(
        notes.iter().map(ToString::to_string).collect::<Vec<_>>(),
        [
            "Voice: the library file no longer has \"out\"; its cable was removed.",
            "Voice: the library file no longer has \"gain\"; its assignment was removed.",
            "Voice: the library file no longer has \"gain\"; its MIDI mapping was removed.",
            "Updated from the library: Voice.",
        ]
    );
    assert!(
        !linked.cables.iter().any(|c| c.from.0 == 2),
        "the cable out of the tile"
    );
    assert!(
        linked.cables.iter().any(|c| c.to == (2, "pitch".to_string())),
        "the cable into a port the file still has stays"
    );
    assert!(linked.modulation.is_empty());
    assert!(linked.midi.is_empty());
    let instance = linked.modules.iter().find(|m| m.id == 2).unwrap();
    assert!(instance.params.is_empty(), "no value for a name that is gone");
    let (controller, hierarchy) = loads(&linked);
    let (_, outputs) = hierarchy.instance_ports(loom_core::ModuleId(2));
    assert_eq!(outputs[0].1, "audio");
    assert_eq!(
        controller.graph().cables().count(),
        3,
        "the note into the generator through the port, and the two inside; nothing out"
    );
}

/// An exposed name gone from a refreshed file goes up the levels: an
/// enclosing definition's entry that re-exposed it, and the value on that
/// definition's instance, go too, and the patch loads.
#[test]
fn refresh_drops_an_enclosing_exposure_of_a_name_the_file_no_longer_has() {
    let text = r#"Patch(
        version: 1,
        subpatches: {
            "rack": SubPatch(
                metadata: Metadata(name: "Rack"),
                modules: [
                    PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"gain": 0.3}),
                    PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
                ],
                cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
                exposed: [Exposed(name: "gain", module: 1, param: "gain")],
            ),
            "voice": SubPatch(
                metadata: Metadata(name: "Voice"),
                modules: [
                    PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
                    PatchModule(id: 2, kind: "loom.phase.generator@1"),
                    PatchModule(id: 3, kind: "loom.osc.sine@1"),
                    PatchModule(id: 4, kind: "loom.math.mul@1", params: {"gain": 0.9}),
                    PatchModule(id: 5, kind: "loom.struct.output@1", label: Some("out")),
                ],
                cables: [
                    PatchCable(from: (1, "out"), to: (2, "pitch")),
                    PatchCable(from: (2, "phase"), to: (3, "phase")),
                    PatchCable(from: (3, "out"), to: (4, "a")),
                    PatchCable(from: (4, "out"), to: (5, "in")),
                ],
                exposed: [Exposed(name: "gain", module: 4, param: "gain")],
                library: Some("Voices/Voice.ron"),
            ),
        },
        modules: [
            PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("rack"), params: {"gain": 0.1}),
            PatchModule(id: 2, kind: "loom.io.audio-out@1"),
            PatchModule(id: 3, kind: "loom.clock.transport@1"),
        ],
        cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
        midi: [PatchMidiMapping(target: (1, "gain"), channel: 0, controller: 7)],
    )"#;
    let mut nested = patch(text);
    loads(&nested);
    let renamed = VOICE.replace("Exposed(name: \"gain\"", "Exposed(name: \"level\"");
    let library = library(&[("Voices/Voice.ron", &renamed)]);
    let notes = nested.refresh_linked(&library, &registry()).unwrap();
    assert!(
        notes
            .iter()
            .any(|n| n.to_string()
                == "Voice: the library file no longer has \"gain\"; its exposed parameter was removed."),
        "{notes:?}"
    );
    assert!(nested.subpatches["rack"].exposed.is_empty());
    assert!(nested.subpatches["rack"].modules[0].params.is_empty());
    assert!(nested.modules[0].params.is_empty(), "the value on the Rack's instance");
    assert!(nested.midi.is_empty(), "the mapping on the Rack's exposed name");
    loads(&nested);
}
