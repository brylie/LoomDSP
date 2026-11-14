use super::*;
use crate::{Exposed, PatchCable, PatchModule, SUBPATCH_KIND};

/// A member instance with id `id` and definition key `key`.
fn member(id: u32, key: &str) -> PatchModule {
    PatchModule {
        subpatch: Some(key.to_string()),
        ..PatchModule::new(id, SUBPATCH_KIND)
    }
}

/// A layer of two members.
fn layer() -> SubPatch {
    SubPatch {
        mode: Mode::Layer,
        members: vec![2, 1],
        modules: vec![member(1, "a"), member(2, "b")],
        ..SubPatch::new("Layer")
    }
}

/// Every mode field is omitted at its default, so a single definition
/// writes the same text as before modes existed, and a set field round
/// trips.
#[test]
fn mode_fields_are_omitted_at_their_defaults_and_round_trip() {
    let single = SubPatch::new("Voice");
    let text = ron::to_string(&single).unwrap();
    for field in ["mode", "members", "copies", "spread", "stereo_spread"] {
        assert!(!text.contains(field), "{field} in {text}");
    }
    let mut stack = SubPatch::new("Unison");
    stack.mode = Mode::Stack;
    stack.copies = 4;
    stack.spread.insert("detune".to_string(), 0.2);
    stack.stereo_spread = true;
    let text = ron::to_string(&stack).unwrap();
    assert!(text.contains("mode:Stack") && text.contains("copies:4") && text.contains("stereo_spread:true"));
    let back: SubPatch = ron::from_str(&text).unwrap();
    assert_eq!(back, stack);
    assert_eq!(back.copy_count(), 4);
    let back: SubPatch = ron::from_str(&ron::to_string(&layer()).unwrap()).unwrap();
    assert_eq!(back.members, vec![2, 1]);
}

/// A copy count of 0 or 1 is one copy, and only a stack has more.
#[test]
fn copy_count_is_one_outside_a_stack() {
    let mut definition = SubPatch::new("Voice");
    assert_eq!(definition.copy_count(), 1);
    definition.copies = 5;
    assert_eq!(definition.copy_count(), 1);
    definition.mode = Mode::Stack;
    assert_eq!(definition.copy_count(), 5);
    definition.copies = 0;
    assert_eq!(definition.copy_count(), 1);
    let settings = ModeSettings::of(&definition);
    assert_eq!(settings.copy_count(), 1);
}

/// The settings the editor keeps write back what the file had, with a
/// copy count of one as 0.
#[test]
fn mode_settings_write_what_they_read() {
    let mut stack = SubPatch::new("Unison");
    stack.mode = Mode::Stack;
    stack.copies = 3;
    let mut written = SubPatch::new("Unison");
    ModeSettings::of(&stack).write(&mut written);
    assert_eq!(written, stack);
    let mut one = ModeSettings::of(&stack);
    one.copies = 1;
    one.write(&mut written);
    assert_eq!(written.copies, 0);
}

/// What the file shape refuses, each with a reason (sub-patches.md, Patch
/// file).
#[test]
fn mode_problems_are_named() {
    assert_eq!(layer().mode_problem(), None);
    assert_eq!(SubPatch::new("Voice").mode_problem(), None);
    let cases: Vec<(SubPatch, &str)> = vec![
        (
            SubPatch {
                members: vec![],
                modules: vec![],
                ..layer()
            },
            "needs at least one member",
        ),
        (
            SubPatch {
                members: vec![1, 1],
                ..layer()
            },
            "twice",
        ),
        (
            SubPatch {
                members: vec![1],
                ..layer()
            },
            "not exactly its modules",
        ),
        (
            SubPatch {
                members: (1..=17).collect(),
                ..layer()
            },
            "more than 16",
        ),
        (
            SubPatch {
                modules: vec![member(1, "a"), PatchModule::new(2, "loom.osc.saw@1")],
                ..layer()
            },
            "not a sub-patch",
        ),
        (
            SubPatch {
                cables: vec![PatchCable {
                    from: (1, "out".into()),
                    to: (2, "in".into()),
                }],
                ..layer()
            },
            "holds only its members",
        ),
        (
            SubPatch {
                mode: Mode::Select,
                exposed: vec![Exposed {
                    name: MEMBER_PARAM.to_string(),
                    module: 1,
                    param: "x".into(),
                    range: None,
                }],
                ..layer()
            },
            "Member control",
        ),
        (
            SubPatch {
                members: vec![1],
                ..SubPatch::new("Voice")
            },
            "has no members",
        ),
        (
            SubPatch {
                copies: 4,
                ..SubPatch::new("Voice")
            },
            "no copies",
        ),
        (
            SubPatch {
                mode: Mode::Stack,
                copies: 17,
                ..SubPatch::new("Voice")
            },
            "above 16",
        ),
        (
            SubPatch {
                mode: Mode::Stack,
                spread: [("cutoff".to_string(), 0.5)].into(),
                ..SubPatch::new("Voice")
            },
            "does not expose",
        ),
        (
            SubPatch {
                mode: Mode::Stack,
                spread: [("cutoff".to_string(), 1.5)].into(),
                exposed: vec![Exposed {
                    name: "cutoff".into(),
                    module: 1,
                    param: "cutoff".into(),
                    range: None,
                }],
                ..SubPatch::new("Voice")
            },
            "outside 0 to 1",
        ),
    ];
    for (definition, words) in cases {
        let problem = definition.mode_problem().expect(words);
        assert!(problem.contains(words), "{problem} lacks {words}");
    }
}

/// The status texts read as the spec names them.
#[test]
fn status_texts_read_as_the_spec_names_them() {
    assert_eq!(
        several_members("Voice", Mode::Single),
        "Remove members until one is left to make Voice single"
    );
    assert_eq!(last_member(Mode::Layer), "A layer needs at least one member");
    assert_eq!(too_many_members(Mode::Select), "A select holds at most 16 members");
    assert_eq!(
        only_members(Mode::Layer),
        "A layer holds only its members; open a member to add modules"
    );
    assert_eq!(dissolve_single_only("Voice"), "Make Voice single before dissolving it");
    assert_eq!(no_library(Mode::Stack), "Cannot save a stack to the library yet");
    assert_eq!(no_role(Mode::Layer), "A layer cannot fill a role yet");
    assert_eq!(
        fills_a_role("Voice", "Filter"),
        "Voice fills Filter; turn that off before changing its mode"
    );
    assert_eq!(stack_holds_slot("Voice"), "Cannot stack Voice: it holds a slot");
    assert_eq!(Mode::ALL.map(Mode::label), ["Single", "Layer", "Select", "Stack"]);
    assert!(Mode::Layer.has_members() && Mode::Select.has_members() && !Mode::Stack.has_members());
}

/// The kinds of the mixers the flattener adds name the module IDs that live
/// patching rule 9 treats as mixers (`loom_core::cable_fade`).
#[test]
fn mixer_kinds_name_the_core_mixer_ids() {
    use loom_core::cable_fade::{MEMBER_MODULE, SELECT_MODULE, SPREAD_SUM_MODULE, SUM_MODULE};
    assert_eq!(SUM_KIND, format!("{SUM_MODULE}@1"));
    assert_eq!(SPREAD_SUM_KIND, format!("{SPREAD_SUM_MODULE}@1"));
    assert_eq!(SELECT_KIND, format!("{SELECT_MODULE}@1"));
    assert_eq!(MEMBER_KIND, format!("{MEMBER_MODULE}@1"));
}
