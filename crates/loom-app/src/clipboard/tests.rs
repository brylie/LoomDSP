use super::*;
use loom_core::{EngineConfig, EngineParts, MidiControl, create_engine};

fn parts() -> EngineParts {
    create_engine(loom_modules::registry(), EngineConfig::with_sample_rate(48_000.0))
}

/// Every module at a place of its own.
fn spread(c: &Controller) -> BTreeMap<u32, Point> {
    c.graph()
        .nodes()
        .map(|(id, _)| (id.0, Point::new(id.0 as f32 * 240.0, 96.0)))
        .collect()
}

fn ids(list: &[ModuleId]) -> BTreeSet<u32> {
    list.iter().map(|id| id.0).collect()
}

/// A voice: phase into a sine into a multiply, whose gain two oscillators
/// modulate. Returns (phase, sine, lfo phase, lfo a, lfo b, multiply).
fn voice(c: &mut Controller) -> [ModuleId; 6] {
    let phase = c.add_module("loom.phase.generator").unwrap();
    let sine = c.add_module("loom.osc.sine").unwrap();
    let lfo_phase = c.add_module("loom.phase.generator").unwrap();
    c.set_param_named(lfo_phase, "tune", 3.0).unwrap();
    let lfo_a = c.add_module("loom.osc.sine").unwrap();
    let lfo_b = c.add_module("loom.osc.saw").unwrap();
    let vca = c.add_module("loom.math.mul").unwrap();
    c.connect_named(phase, "phase", sine, "phase").unwrap();
    c.connect_named(sine, "out", vca, "a").unwrap();
    c.connect_named(lfo_phase, "phase", lfo_a, "phase").unwrap();
    c.connect_named(lfo_phase, "phase", lfo_b, "phase").unwrap();
    c.add_assignment_named(lfo_a, "out", vca, "gain", 0.3).unwrap();
    c.add_assignment_named(lfo_b, "out", vca, "gain", 0.2).unwrap();
    [phase, sine, lfo_phase, lfo_a, lfo_b, vca]
}

#[test]
fn copy_writes_a_versioned_patch_with_inside_cables_and_assignments_only() {
    let mut c = parts().controller;
    let [phase, sine, lfo_phase, lfo_a, _, vca] = voice(&mut c);
    c.map_midi(
        MidiControl {
            channel: 0,
            controller: 74,
        },
        vca,
        0,
    )
    .unwrap();
    let positions = spread(&c);
    // The selection leaves out the voice's phase and the second oscillator.
    let selection = ids(&[sine, lfo_phase, lfo_a, vca]);
    let text = Group::from_selection(c.graph(), &positions, &selection, false).to_text();
    let patch = Patch::from_ron(&text).unwrap();

    assert_eq!(patch.version, loom_patch::CURRENT_VERSION);
    assert!(patch.name.is_empty());
    let copied: Vec<u32> = patch.modules.iter().map(|m| m.id).collect();
    assert_eq!(copied, vec![sine.0, lfo_phase.0, lfo_a.0, vca.0]);
    let lfo = patch.modules.iter().find(|m| m.id == lfo_phase.0).unwrap();
    assert_eq!(lfo.params.get("tune"), Some(&3.0));
    assert_eq!(lfo.position, Some((lfo_phase.0 as f32 * 240.0, 96.0)));
    // The cable from the outside phase and the assignment from the outside
    // oscillator are dropped.
    assert_eq!(patch.cables.len(), 2);
    assert!(patch.cables.iter().all(|cable| cable.from.0 != phase.0));
    assert_eq!(patch.modulation.len(), 1);
    assert_eq!(patch.modulation[0].source.0, lfo_a.0);
    assert!(patch.midi.is_empty(), "MIDI mappings are never copied");
}

#[test]
fn paste_adds_new_ids_with_values_cables_and_assignments_in_order() {
    let mut c = parts().controller;
    let voice = voice(&mut c);
    c.commit().unwrap();
    let mut positions = spread(&c);
    let text = Group::from_selection(c.graph(), &positions, &ids(&voice), false).to_text();

    let before: BTreeSet<u32> = c.graph().nodes().map(|(id, _)| id.0).collect();
    let group = Group::from_text(&text, c.registry()).unwrap();
    group.check(&c).unwrap();
    let new = group.add(&mut c, &mut positions, Point::new(1200.0, 600.0)).unwrap();
    c.commit().unwrap();

    assert_eq!(new.len(), 6);
    assert!(new.iter().all(|id| !before.contains(id)), "every id is new");
    let graph = c.graph();
    // The new modules follow the old ones' order, so the third new one is
    // the tuned phase.
    assert_eq!(
        graph.node(ModuleId(new[2])).unwrap().params,
        graph.node(voice[2]).unwrap().params
    );
    let new_set: BTreeSet<u32> = new.iter().copied().collect();
    let cables = graph.cables().filter(|(_, to)| new_set.contains(&to.module.0)).count();
    assert_eq!(cables, 4);
    let sources: Vec<u32> = graph
        .assignments()
        .filter(|(_, a)| new_set.contains(&a.target.0))
        .map(|(_, a)| a.source.module.0)
        .collect();
    assert_eq!(sources, vec![new[3], new[4]], "assignments keep their order");
    // The group keeps its layout, with its top-left corner at the origin.
    assert_eq!(positions[&new[0]], Point::new(1200.0, 600.0));
    assert_eq!(positions[&new[1]], Point::new(1440.0, 600.0));
}

#[test]
fn duplicate_keeps_incoming_links_and_drops_outgoing_ones() {
    let mut c = parts().controller;
    let [phase, sine, _, lfo_a, lfo_b, vca] = voice(&mut c);
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(vca, "out", out, "left").unwrap();
    c.commit().unwrap();
    let mut positions = spread(&c);
    let original_cables: Vec<_> = c.graph().cables().collect();
    let original_assignments: Vec<_> = c.graph().assignments().map(|(id, a)| (id, *a)).collect();

    // Duplicate the sine and the multiply. The phase and the oscillators
    // stay outside; the audio output is outgoing.
    let group = Group::from_selection(c.graph(), &positions, &ids(&[sine, vca]), true);
    let at = group.top_left().unwrap();
    let new = group
        .add(
            &mut c,
            &mut positions,
            Point::new(at.x + DUPLICATE_OFFSET, at.y + DUPLICATE_OFFSET),
        )
        .unwrap();
    c.commit().unwrap();
    let (new_sine, new_vca) = (ModuleId(new[0]), ModuleId(new[1]));
    let graph = c.graph();

    let into = |module: ModuleId| -> Vec<OutputRef> {
        let mut list: Vec<_> = graph
            .cables()
            .filter(|(_, to)| to.module == module)
            .map(|(from, _)| from)
            .collect();
        list.sort();
        list
    };
    assert_eq!(
        into(new_sine),
        vec![OutputRef { module: phase, port: 0 }],
        "incoming kept"
    );
    assert_eq!(
        into(new_vca),
        vec![OutputRef {
            module: new_sine,
            port: 0
        }]
    );
    assert!(
        graph.cables().all(|(from, _)| from.module != new_vca),
        "outgoing dropped"
    );
    let sources: Vec<ModuleId> = graph
        .assignments()
        .filter(|(_, a)| a.target == new_vca)
        .map(|(_, a)| a.source.module)
        .collect();
    assert_eq!(sources, vec![lfo_a, lfo_b], "incoming assignments kept, in order");
    // The original is unchanged.
    for cable in &original_cables {
        assert!(graph.cables().any(|c| c == *cable));
    }
    for (id, assignment) in &original_assignments {
        assert_eq!(
            graph.assignments().find(|(i, _)| i == id).map(|(_, a)| *a),
            Some(*assignment)
        );
    }
    assert_eq!(
        positions[&new_sine.0],
        Point::new(positions[&sine.0].x + 24.0, positions[&sine.0].y + 24.0)
    );
}

/// Renders both channels at a block size.
fn render(parts: &mut EngineParts, block: usize) -> (Vec<f32>, Vec<f32>) {
    let frames = 4_096;
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    let mut at = 0;
    while at < frames {
        let n = block.min(frames - at);
        parts.engine.process(&mut left[at..at + n], &mut right[at..at + n]);
        at += n;
    }
    (left, right)
}

#[test]
fn a_duplicated_voice_sounds_like_one_built_by_hand() {
    for block in [1, 77, 512] {
        let mut by_hand = parts();
        let c = &mut by_hand.controller;
        let first = voice(c);
        let second = voice(c);
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(first[5], "out", out, "left").unwrap();
        c.connect_named(second[5], "out", out, "right").unwrap();
        c.commit().unwrap();
        let expected = render(&mut by_hand, block);

        let mut duplicated = parts();
        let c = &mut duplicated.controller;
        let first = voice(c);
        let mut positions = spread(c);
        let group = Group::from_selection(c.graph(), &positions, &ids(&first), true);
        let new = group.add(c, &mut positions, Point::ORIGIN).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(first[5], "out", out, "left").unwrap();
        c.connect_named(ModuleId(new[5]), "out", out, "right").unwrap();
        c.commit().unwrap();
        assert_eq!(render(&mut duplicated, block), expected, "block size {block}");
    }
}

#[test]
fn paste_reads_hand_edited_text_and_ignores_midi_mappings() {
    let text = r#"Patch(
        version: 1,
        name: "Somebody's patch",
        modules: [
            (id: 10, kind: "loom.math.mul", params: {"gain": 0.25}),
            (id: 20, kind: "loom.osc.sine"),
        ],
        cables: [(from: (20, "out"), to: (10, "a"))],
        midi: [(target: (10, "gain"), channel: 0, controller: 7)],
    )"#;
    let mut c = parts().controller;
    let mut positions = BTreeMap::new();
    let group = Group::from_text(text, c.registry()).unwrap();
    assert_eq!(group.describe(), "2 modules");
    let new = group.add(&mut c, &mut positions, Point::new(48.0, 48.0)).unwrap();
    c.commit().unwrap();
    let gain = c.graph().node(ModuleId(new[0])).unwrap().params[0];
    assert_eq!(gain, 0.25);
    assert_eq!(c.graph().cables().count(), 1);
    assert_eq!(c.graph().midi_mappings().count(), 0);
    // Modules with no position go in a row at the paste point.
    assert_eq!(positions[&new[0]].x, 48.0);
    assert!(positions[&new[1]].x > positions[&new[0]].x);
}

#[test]
fn paste_refuses_text_it_cannot_add_whole() {
    let c = parts().controller;
    let registry = c.registry();
    let refuse = |text: &str| Group::from_text(text, registry).unwrap_err();
    assert_eq!(refuse("hello"), NOTHING_TO_PASTE);
    assert_eq!(refuse("Patch(version: 1)"), NOTHING_TO_PASTE);
    assert!(refuse("Patch(version: 99, modules: [(id: 1, kind: \"loom.osc.sine\")])").contains("newer"));
    let unknown = refuse("Patch(version: 1, modules: [(id: 1, kind: \"loom.none.such\")])");
    assert_eq!(unknown, "Cannot paste: unknown module `loom.none.such`");
    let param = refuse("Patch(version: 1, modules: [(id: 1, kind: \"loom.osc.sine\", params: {\"nope\": 1.0})])");
    assert!(param.contains("no parameter `nope`"), "{param}");
    let missing = refuse(
        "Patch(version: 1, modules: [(id: 1, kind: \"loom.osc.sine\")], cables: [(from: (2, \"out\"), to: (1, \"phase\"))])",
    );
    assert!(missing.contains("no module has id 2"), "{missing}");
}

#[test]
fn a_group_too_big_for_the_patch_is_refused_before_anything_is_added() {
    let mut config = EngineConfig::with_sample_rate(48_000.0);
    config.max_modules = 3;
    let mut c = create_engine(loom_modules::registry(), config).controller;
    let a = c.add_module("loom.osc.sine").unwrap();
    let b = c.add_module("loom.osc.saw").unwrap();
    c.commit().unwrap();
    let group = Group::from_selection(c.graph(), &BTreeMap::new(), &ids(&[a, b]), false);
    let why = group.check(&c).unwrap_err();
    assert_eq!(why, "the patch has room for 1 more module");
    assert_eq!(c.graph().nodes().count(), 2);
}

#[test]
fn a_paste_that_fills_the_command_queue_is_taken_back_and_frees_its_slots() {
    let mut config = EngineConfig::with_sample_rate(48_000.0);
    config.command_capacity = 4;
    let mut parts = create_engine(loom_modules::registry(), config);
    let mut positions = BTreeMap::new();
    let mut voice_ids = Vec::new();
    // Build the voice a little at a time, so the small queue never fills.
    for kind in ["loom.phase.generator", "loom.osc.sine", "loom.math.mul"] {
        voice_ids.push(parts.controller.add_module(kind).unwrap());
        parts.controller.commit().unwrap();
        parts.engine.process(&mut [0.0; 16], &mut [0.0; 16]);
    }
    let c = &mut parts.controller;
    c.connect_named(voice_ids[0], "phase", voice_ids[1], "phase").unwrap();
    c.connect_named(voice_ids[1], "out", voice_ids[2], "a").unwrap();
    c.commit().unwrap();
    parts.engine.process(&mut [0.0; 16], &mut [0.0; 16]);
    parts.controller.commit().ok();
    let c = &mut parts.controller;
    let before: Vec<u32> = c.graph().nodes().map(|(id, _)| id.0).collect();
    let free = c.free_module_slots();

    // Two copies of three modules do not fit in a queue of four.
    let group = Group::from_selection(c.graph(), &positions, &ids(&voice_ids), false);
    group.add(c, &mut positions, Point::ORIGIN).unwrap();
    group.check(c).unwrap();
    assert!(matches!(
        group.add(c, &mut positions, Point::ORIGIN),
        Err(LoomError::QueueFull)
    ));
    assert_eq!(
        c.graph().nodes().count(),
        before.len() + 3,
        "the failed paste left nothing"
    );

    for _ in 0..3 {
        parts.engine.process(&mut [0.0; 16], &mut [0.0; 16]);
        let _ = parts.controller.commit();
    }
    assert_eq!(
        parts.controller.free_module_slots(),
        free - 3,
        "the commit hands the failed paste's slots back"
    );
}

/// A copied module keeps its control range and its value, even a value
/// outside the default range (control-ranges.md).
#[test]
fn copy_and_paste_keep_control_ranges() {
    let mut c = parts().controller;
    let mul = c.add_module("loom.math.mul").unwrap();
    c.set_range(mul, 0, ParamRange::new(-5.0, 5.0)).unwrap();
    c.set_param(mul, 0, 4.0).unwrap();
    let mut positions = spread(&c);
    let text = Group::from_selection(c.graph(), &positions, &ids(&[mul]), false).to_text();
    assert!(text.contains(r#"ranges: {"gain": (-5.0, 5.0)}"#), "{text}");
    let group = Group::from_text(&text, c.registry()).unwrap();
    let new = group.add(&mut c, &mut positions, Point::new(1200.0, 600.0)).unwrap();
    let node = c.graph().node(ModuleId(new[0])).unwrap();
    assert_eq!((node.params[0], node.range(0)), (4.0, Some(ParamRange::new(-5.0, 5.0))));
}
