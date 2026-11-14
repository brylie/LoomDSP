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

/// `levels` diamonds of empty Effect slots in a row: slot `S_k` feeds two
/// slots `B_k` and `C_k`, and both feed `S_{k+1}`. With `closed`, the last
/// `S` feeds the first, a loop; otherwise it feeds the output. The ids of
/// every slot, in order.
fn diamonds(levels: u32, closed: bool) -> (String, Vec<u32>) {
    let slot = |id: u32| {
        format!("PatchModule(id: {id}, kind: \"loom.struct.slot@1\", slot: Some(Slot(role: \"loom.role.effect@1\"))),")
    };
    let mut modules = String::from("PatchModule(id: 2, kind: \"loom.io.audio-out@1\"),");
    let mut cables = String::new();
    let mut ids = Vec::new();
    let s = |k: u32| 10 + 3 * k;
    for k in 0..levels {
        let (a, b, c) = (s(k), s(k) + 1, s(k) + 2);
        ids.extend([a, b, c]);
        for id in [a, b, c] {
            modules.push_str(&slot(id));
        }
        cables.push_str(&format!(
            "PatchCable(from: ({a}, \"out\"), to: ({b}, \"in\")), PatchCable(from: ({a}, \"out\"), to: ({c}, \"in\")),"
        ));
        let next = if k + 1 < levels {
            s(k + 1)
        } else if closed {
            s(0)
        } else {
            2
        };
        cables.push_str(&format!(
            "PatchCable(from: ({b}, \"out\"), to: ({next}, \"in\")), PatchCable(from: ({c}, \"out\"), to: ({next}, \"in\")),"
        ));
    }
    (
        format!("Patch(version: 1, metadata: Metadata(name: \"Diamonds\"), modules: [{modules}], cables: [{cables}])"),
        ids,
    )
}

/// `slots` empty Effect slots in a ring, each feeding the next. The ids.
fn ring(slots: u32) -> (String, Vec<u32>) {
    let ids: Vec<u32> = (0..slots).map(|n| 10 + n).collect();
    let mut modules = String::new();
    let mut cables = String::new();
    for (n, id) in ids.iter().enumerate() {
        let next = ids[(n + 1) % ids.len()];
        modules.push_str(&format!(
            "PatchModule(id: {id}, kind: \"loom.struct.slot@1\", slot: Some(Slot(role: \"loom.role.effect@1\"))),"
        ));
        cables.push_str(&format!("PatchCable(from: ({id}, \"out\"), to: ({next}, \"in\")),"));
    }
    (
        format!("Patch(version: 1, metadata: Metadata(name: \"Ring\"), modules: [{modules}], cables: [{cables}])"),
        ids,
    )
}

/// Asks `first_input` for every slot's `in` of `text`, in `order`, and
/// checks each against the plain follow.
fn agrees(text: &str, order: &[u32]) {
    let (controller, hierarchy) = load(&Patch::from_ron(text).unwrap());
    let graph = controller.graph();
    let root = ModulePath::root();
    let behind = Behind::new(&hierarchy, graph);
    for id in order {
        let into = (*id, "in".to_string());
        assert_eq!(
            behind.first_input(&root, &into),
            hierarchy.inputs_behind(graph, &root, &into).first().cloned(),
            "{id} in"
        );
    }
}

/// Three empty slots in a loop with two ways out: 10 feeds 11 and an SVF,
/// 11 feeds 12 and the output, and 12 feeds 10 again.
const TWO_EXITS: &str = r#"Patch(version: 1, modules: [
    PatchModule(id: 10, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.effect@1"))),
    PatchModule(id: 11, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.effect@1"))),
    PatchModule(id: 12, kind: "loom.struct.slot@1", slot: Some(Slot(role: "loom.role.effect@1"))),
    PatchModule(id: 20, kind: "loom.io.audio-out@1"),
    PatchModule(id: 30, kind: "loom.filter.svf@1"),
], cables: [
    PatchCable(from: (10, "out"), to: (11, "in")),
    PatchCable(from: (10, "out"), to: (30, "in")),
    PatchCable(from: (11, "out"), to: (12, "in")),
    PatchCable(from: (11, "out"), to: (20, "in")),
    PatchCable(from: (12, "out"), to: (10, "in")),
])"#;

/// Inside a loop with two ways out, an end gets an input the loop
/// reaches, though not always the plain follow's first: asked after 10,
/// slot 11 keeps 10's answer, the output, where the plain follow goes on
/// round the loop to the SVF. Asked first, each end gets the plain
/// follow's answer.
#[test]
fn a_loop_with_two_ways_out_keeps_an_input_it_reaches() {
    let (controller, hierarchy) = load(&Patch::from_ron(TWO_EXITS).unwrap());
    let graph = controller.graph();
    let root = ModulePath::root();
    let node = |kind: &str| graph.nodes().find(|(_, n)| n.descriptor.id == kind).unwrap().0;
    let (output, svf) = (
        (node("loom.io.audio-out"), "in".to_string()),
        (node("loom.filter.svf"), "in".to_string()),
    );
    let plain = |id: u32| {
        hierarchy
            .inputs_behind(graph, &root, &(id, "in".to_string()))
            .first()
            .cloned()
    };
    assert_eq!(plain(10), Some(output.clone()));
    assert_eq!(plain(11), Some(svf.clone()));

    let behind = Behind::new(&hierarchy, graph);
    assert_eq!(behind.first_input(&root, &(10, "in".to_string())), Some(output.clone()));
    assert_eq!(behind.first_input(&root, &(11, "in".to_string())), Some(output));
    let fresh = Behind::new(&hierarchy, graph);
    assert_eq!(fresh.first_input(&root, &(11, "in".to_string())), Some(svf));
}

/// On small diamonds, open and closed into a loop, and on a ring, the memo
/// gives what the plain follow gives, asked in either order.
#[test]
fn the_memo_agrees_on_diamonds_and_rings() {
    for (text, ids) in [diamonds(3, false), diamonds(3, true), ring(5)] {
        agrees(&text, &ids);
        let backwards: Vec<u32> = ids.iter().rev().copied().collect();
        agrees(&text, &backwards);
    }
}

/// A crafted loop of 20 diamonds (60 empty slots, each reached by two
/// routes) and a ring of 10,000 empty slots are answered for every end
/// within a second: each walk visits an end once, and a walk that finds
/// nothing keeps that for every end it reached. Without both, the
/// diamonds take time that doubles with each one and the ring its square.
#[test]
fn loops_of_empty_slots_are_answered_quickly() {
    for (text, ids) in [diamonds(20, true), ring(10_000)] {
        let (controller, hierarchy) = load(&Patch::from_ron(&text).unwrap());
        let root = ModulePath::root();
        let behind = Behind::new(&hierarchy, controller.graph());
        let started = std::time::Instant::now();
        for id in &ids {
            assert_eq!(behind.first_input(&root, &(*id, "in".to_string())), None);
            assert_eq!(behind.source(&root, &(*id, "out".to_string())), None);
        }
        let took = started.elapsed();
        assert!(
            took < std::time::Duration::from_secs(1),
            "{} slots took {took:?}",
            ids.len()
        );
    }
}

/// A ring of 10,000 empty slots, one of which also feeds the output, is
/// answered for every end within a second: each walk keeps the output for
/// every end on its path.
#[test]
fn a_ring_with_one_way_out_is_answered_quickly() {
    let (text, ids) = ring(10_000);
    let exit = ids[ids.len() / 2];
    let text = text.replacen(
        "modules: [",
        "modules: [PatchModule(id: 2, kind: \"loom.io.audio-out@1\"),",
        1,
    );
    let text = text.replacen(
        "cables: [",
        &format!("cables: [PatchCable(from: ({exit}, \"out\"), to: (2, \"in\")),"),
        1,
    );
    let (controller, hierarchy) = load(&Patch::from_ron(&text).unwrap());
    let graph = controller.graph();
    let output = graph
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.io.audio-out")
        .unwrap()
        .0;
    let root = ModulePath::root();
    let behind = Behind::new(&hierarchy, graph);
    let started = std::time::Instant::now();
    for id in &ids {
        assert_eq!(
            behind.first_input(&root, &(*id, "in".to_string())),
            Some((output, "in".to_string()))
        );
    }
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(1), "took {took:?}");
}
