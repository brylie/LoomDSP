use super::*;
use crate::Patch;

/// The spec's file example (`docs/design/sub-patches.md`, Patch file),
/// without the fields later slices add.
const VOICE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Voices"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch")),
                PatchModule(id: 2, kind: "loom.osc.saw@1"),
                PatchModule(id: 3, kind: "loom.filter.svf@1", params: {"cutoff": 800.0}),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [
                PatchCable(from: (2, "out"), to: (3, "in")),
                PatchCable(from: (3, "lowpass"), to: (4, "in")),
            ],
            exposed: [Exposed(name: "cutoff", module: 3, param: "cutoff", range: Some((200.0, 2000.0)))],
        ),
    },
    modules: [
        PatchModule(id: 7, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), params: {"cutoff": 1200.0}),
    ],
)
"#;

/// The spec's example parses, and its canonical text is stable across a
/// parse and print cycle.
#[test]
fn the_spec_example_round_trips() {
    let patch = Patch::from_ron(VOICE).unwrap();
    let voice = &patch.subpatches["voice"];
    assert_eq!(voice.metadata.name, "Voice");
    assert_eq!(voice.modules.len(), 4);
    assert_eq!(voice.exposed[0].range, Some((200.0, 2000.0)));
    let instance = &patch.modules[0];
    assert!(instance.is_instance());
    assert_eq!(instance.subpatch.as_deref(), Some("voice"));
    assert_eq!(instance.params["cutoff"], 1200.0);

    let text = patch.to_ron().unwrap();
    assert_eq!(Patch::from_ron(&text).unwrap(), patch);
    assert_eq!(Patch::from_ron(&text).unwrap().to_ron().unwrap(), text);
    assert!(text.contains(r#"subpatch: Some("voice")"#), "{text}");
    assert!(text.contains("range: Some((200.0, 2000.0))"), "{text}");
}

/// A nested file prints each module and cable of a definition on its own
/// line, so a diff shows one change per line.
#[test]
fn a_definition_prints_one_module_per_line() {
    let text = Patch::from_ron(VOICE).unwrap().to_ron().unwrap();
    let module_lines = text
        .lines()
        .filter(|line| line.trim_start().starts_with("PatchModule(id:"))
        .count();
    assert_eq!(module_lines, 4, "{text}");
    assert!(
        text.contains("\n                PatchCable(from: (2, \"out\"), to: (3, \"in\")),\n"),
        "{text}"
    );
}

/// A flat patch's text does not change: no `subpatches` table, no
/// `subpatch` field, and still one module per line.
#[test]
fn a_flat_patch_writes_nothing_new() {
    let mut patch = Patch::new("Flat");
    patch.modules.push(PatchModule {
        id: 1,
        kind: "loom.osc.sine@1".into(),
        subpatch: None,
        label: None,
        params: Default::default(),
        position: None,
        size: None,
        ranges: Default::default(),
    });
    let text = patch.to_ron().unwrap();
    assert!(!text.contains("subpatch"), "{text}");
    assert!(
        text.contains("\n        PatchModule(id: 1, kind: \"loom.osc.sine@1\"),\n"),
        "{text}"
    );
}

/// Canonical order reaches into each definition.
#[test]
fn canonicalise_sorts_inside_definitions() {
    let mut patch = Patch::from_ron(VOICE).unwrap();
    let voice = patch.subpatches.get_mut("voice").unwrap();
    voice.modules.reverse();
    voice.cables.reverse();
    patch.canonicalise();
    let voice = &patch.subpatches["voice"];
    assert_eq!(voice.modules.iter().map(|m| m.id).collect::<Vec<_>>(), [1, 2, 3, 4]);
    assert_eq!(voice.cables[0].to.0, 3);
}

/// A new definition is empty and named.
#[test]
fn a_new_definition_is_empty() {
    let voice = SubPatch::new("Voice");
    assert_eq!(voice.metadata.name, "Voice");
    assert!(voice.modules.is_empty() && voice.exposed.is_empty());
}
