//! Tests for the kept follows: they give what the plain follows give, on
//! chains, fan-outs, and loops, and a long chain of empty thru slots is
//! answered for every end in a time proportional to its length.

use super::*;
use crate::Patch;
use crate::hierarchy::tests::load;

/// A sine into `slots` empty Effect slots in a row, into the output, and
/// the id of the last slot.
fn chain(slots: u32) -> (String, u32) {
    let mut modules = String::from(
        "PatchModule(id: 1, kind: \"loom.osc.sine@1\"), PatchModule(id: 2, kind: \"loom.io.audio-out@1\"),",
    );
    let mut cables = String::new();
    let mut from = 1;
    for n in 0..slots {
        let id = 10 + n;
        modules.push_str(&format!(
            "PatchModule(id: {id}, kind: \"loom.struct.slot@1\", slot: Some(Slot(role: \"loom.role.effect@1\"))),"
        ));
        cables.push_str(&format!("PatchCable(from: ({from}, \"out\"), to: ({id}, \"in\")),"));
        from = id;
    }
    cables.push_str(&format!("PatchCable(from: ({from}, \"out\"), to: (2, \"in\")),"));
    (
        format!("Patch(version: 1, metadata: Metadata(name: \"Chain\"), modules: [{modules}], cables: [{cables}])"),
        from,
    )
}

/// On a short chain every end gets the same answer from the memo as from
/// the plain follow, asked in any order.
#[test]
fn the_memo_agrees_with_the_plain_follows() {
    let (text, last) = chain(6);
    let (controller, hierarchy) = load(&Patch::from_ron(&text).unwrap());
    let graph = controller.graph();
    let root = ModulePath::root();
    let behind = Behind::new(&hierarchy, graph);
    for id in [last, 12, 10, 14, 11, 13] {
        let out = (id, "out".to_string());
        assert_eq!(
            behind.source(&root, &out),
            hierarchy.source_behind(graph, &root, &out),
            "{id} out"
        );
        let into = (id, "in".to_string());
        assert_eq!(
            behind.first_input(&root, &into),
            hierarchy.inputs_behind(graph, &root, &into).first().cloned(),
            "{id} in"
        );
    }
    let missing = (99, "out".to_string());
    assert_eq!(behind.source(&root, &missing), None);
    assert_eq!(behind.first_input(&root, &(99, "in".to_string())), None);
}

/// Two empty slots that feed each other have no source and no input
/// behind them, from either end, and asking again gives the same.
#[test]
fn a_loop_of_empty_slots_has_nothing_behind_it() {
    let text = r#"Patch(version: 1, modules: [
        PatchModule(id: 10, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.effect@1"))),
        PatchModule(id: 11, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.effect@1"))),
    ], cables: [
        PatchCable(from: (10, "out"), to: (11, "in")),
        PatchCable(from: (11, "out"), to: (10, "in")),
    ])"#;
    let (controller, hierarchy) = load(&Patch::from_ron(text).unwrap());
    let behind = Behind::new(&hierarchy, controller.graph());
    let root = ModulePath::root();
    for _ in 0..2 {
        for id in [10, 11] {
            assert_eq!(behind.source(&root, &(id, "out".to_string())), None);
            assert_eq!(behind.first_input(&root, &(id, "in".to_string())), None);
        }
    }
}

/// Every end of a chain of 10,000 empty Effect slots is answered within a
/// second, both ways: the plain follows would walk the chain once per end,
/// some 50 million steps (sub-patches.md, Empty slots).
#[test]
fn a_long_chain_is_answered_for_every_end_in_linear_time() {
    let (text, last) = chain(10_000);
    let (controller, hierarchy) = load(&Patch::from_ron(&text).unwrap());
    let graph = controller.graph();
    let root = ModulePath::root();
    let sine = controller
        .graph()
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.osc.sine")
        .unwrap()
        .0;
    let out = controller
        .graph()
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.io.audio-out")
        .unwrap()
        .0;
    let behind = Behind::new(&hierarchy, graph);
    let started = std::time::Instant::now();
    for id in (10..=last).rev() {
        assert_eq!(
            behind.source(&root, &(id, "out".into())),
            Some((sine, "out".to_string()))
        );
    }
    for id in 10..=last {
        assert_eq!(
            behind.first_input(&root, &(id, "in".into())),
            Some((out, "in".to_string()))
        );
    }
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(1), "took {took:?}");
}
