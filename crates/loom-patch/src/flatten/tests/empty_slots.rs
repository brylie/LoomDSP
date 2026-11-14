//! Empty slots in the flattener: a thru role passes its input to its
//! output, any other role is a pulled cable, and an assignment from an
//! empty slot moves nothing (ADR-0043, sub-patches.md, Empty slots).

use super::*;

/// Noise into an empty slot of `role`, whose `out` feeds the output and,
/// through an assignment, a Mixer's level.
fn empty_slot_patch(role: &str) -> String {
    format!(
        r#"Patch(
    version: 1,
    metadata: Metadata(name: "Empty"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.noise@1"),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 5, kind: "loom.mix.mixer@1"),
        PatchModule(id: 9, kind: "loom.struct.slot@1", slot: Some(Slot(role: "{role}"))),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (9, "in")),
        PatchCable(from: (9, "out"), to: (3, "in")),
    ],
    modulation: [
        PatchModulation(source: (9, "out"), target: (5, "level1"), depth: 0.5),
    ],
)
"#
    )
}

/// An empty Filter slot is a thru: the cable from its `out` continues from
/// what feeds its `in`. An assignment from it still moves nothing, since
/// the thru applies to cables only.
#[test]
fn an_empty_thru_slot_passes_its_input_on_for_cables_only() {
    let (flat, _) = flatten(&empty_slot_patch("loom.role.filter@1"));
    let cables: Vec<Ends> = flat
        .cables
        .iter()
        .map(|c| ((c.from.0, c.from.1.as_str()), (c.to.0, c.to.1.as_str())))
        .collect();
    assert_eq!(cables, [((1, "out"), (3, "in"))]);
    assert!(flat.modulation.is_empty(), "{:?}", flat.modulation);
}

/// An empty Envelope slot is a pulled cable: nothing comes out, so the
/// output reads its normal.
#[test]
fn an_empty_slot_of_another_role_is_a_pulled_cable() {
    let text = empty_slot_patch("loom.role.envelope@1").replace(r#"(9, "in")"#, r#"(9, "gate")"#);
    let (flat, _) = flatten(&text);
    assert!(flat.cables.is_empty(), "{:?}", flat.cables);
    assert!(flat.modulation.is_empty());
}

/// An empty thru slot with nothing on its `in` passes nothing on.
#[test]
fn an_empty_thru_slot_with_nothing_on_its_input_passes_nothing() {
    let text = empty_slot_patch("loom.role.effect@1").replace(
        r#"        PatchCable(from: (1, "out"), to: (9, "in")),
"#,
        "",
    );
    let (flat, _) = flatten(&text);
    assert!(flat.cables.is_empty(), "{:?}", flat.cables);
}

/// A chain of 10,000 empty Effect slots in series flattens without
/// overflowing the stack: the follow is a loop (a crafted file must not
/// crash the app), and the noise reaches the output through all of them.
#[test]
fn a_long_chain_of_empty_thru_slots_flattens() {
    const SLOTS: u32 = 10_000;
    let mut modules = String::from(
        "        PatchModule(id: 1, kind: \"loom.osc.noise@1\"),\n        PatchModule(id: 2, kind: \"loom.io.audio-out@1\"),\n",
    );
    let mut cables = String::new();
    let mut from = (1, "out");
    for n in 0..SLOTS {
        let id = 10 + n;
        modules.push_str(&format!(
            "        PatchModule(id: {id}, kind: \"loom.struct.slot@1\", slot: Some(Slot(role: \"loom.role.effect@1\"))),\n"
        ));
        cables.push_str(&format!(
            "        PatchCable(from: ({}, \"{}\"), to: ({id}, \"in\")),\n",
            from.0, from.1
        ));
        from = (id, "out");
    }
    cables.push_str(&format!(
        "        PatchCable(from: ({}, \"out\"), to: (2, \"in\")),\n",
        from.0
    ));
    let text =
        format!("Patch(version: 1, metadata: Metadata(name: \"Chain\"), modules: [\n{modules}], cables: [\n{cables}])");
    // Run on a thread with a small stack, as the app's background work
    // would, so a recursive follow would overflow here.
    let flat = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || flatten(&text).0)
        .unwrap()
        .join()
        .expect("no stack overflow");
    let cables: Vec<Ends> = flat
        .cables
        .iter()
        .map(|c| ((c.from.0, c.from.1.as_str()), (c.to.0, c.to.1.as_str())))
        .collect();
    assert_eq!(cables, [((1, "out"), (2, "in"))]);
}
