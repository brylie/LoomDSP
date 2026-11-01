use super::*;
use crate::{ModuleRole, Panel, ParamOptions, ParamSpec};

/// The hardware control every test maps.
const KNOB: MidiControl = MidiControl {
    channel: 0,
    controller: 7,
};

/// A module with one parameter from 0 to 1, for a mapping to target.
static LEVEL: ModuleDescriptor = ModuleDescriptor {
    id: "test.level",
    version: 1,
    name: "Level",
    category: "Test",
    family: crate::Family::Interface,
    description: "One parameter for the MIDI sync tests.",
    inputs: &[],
    outputs: &[],
    params: &[ParamSpec {
        name: "level",
        label: "Level",
        description: "A test parameter.",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit: "",
        options: ParamOptions::DEFAULT,
    }],
    role: ModuleRole::UnitDelay,
    panel: Panel::EMPTY,
    displays: &[],
};

/// A graph with module 1 and `id` mapping KNOB to its level.
fn mapped(id: MidiMappingId) -> Graph {
    let mut graph = Graph::default();
    graph.nodes.insert(ModuleId(1), Node::new(&LEVEL, None));
    graph.midi_mappings.insert(
        id,
        MidiMapping {
            control: KNOB,
            target: ModuleId(1),
            param: 0,
        },
    );
    graph
}

/// The level of module 1.
fn level(graph: &Graph) -> f32 {
    graph.nodes[&ModuleId(1)].params[0]
}

/// Mapping ids count up and are never reused, so feedback for a removed
/// mapping can never be taken for a new one.
#[test]
fn mapping_ids_count_up() {
    let mut midi = MidiSync::new(Arc::new(MidiFeedback::new()));
    let first = midi.new_mapping();
    let second = midi.new_mapping();
    assert!(second.0 > first.0);
}

/// Edit tokens count up from 1 and skip 0, which means "no edit", when
/// they wrap.
#[test]
fn edit_tokens_never_use_zero() {
    let mut midi = MidiSync::new(Arc::new(MidiFeedback::new()));
    let id = midi.new_mapping();
    assert_eq!(midi.next_token(), 1);
    assert_eq!(midi.record_edit(id), 1);
    assert_eq!(midi.next_token(), 2);
    midi.next_edit = u32::MAX;
    assert_eq!(midi.record_edit(id), u32::MAX);
    assert_eq!(midi.next_token(), 1);
}

/// A control change that reached the engine before a screen edit never
/// overwrites the edit. Once the engine confirms the edit, a newer control
/// change applies (MIDI learn, rule 2).
#[test]
fn a_control_change_before_a_screen_edit_is_ignored() {
    let feedback = Arc::new(MidiFeedback::new());
    let mut midi = MidiSync::new(feedback.clone());
    let id = midi.new_mapping();
    let mut graph = mapped(id);
    feedback.write(KNOB, id, 5, 127);

    // The screen sets 0.25 and sends the edit with its token.
    graph.nodes.get_mut(&ModuleId(1)).unwrap().params[0] = 0.25;
    let token = midi.record_edit(id);
    assert!(midi.sync(&mut graph).is_empty(), "the edit is still on its way");
    assert_eq!(level(&graph), 0.25);

    // The engine applies the edit after control change 5: still older.
    feedback.write_edit(KNOB, token, 5);
    assert!(midi.sync(&mut graph).is_empty());
    assert_eq!(level(&graph), 0.25);

    // A control change after the edit applies.
    feedback.write(KNOB, id, 6, 0);
    assert_eq!(midi.sync(&mut graph), [(ModuleId(1), 0)]);
    assert_eq!(level(&graph), 0.0);
}

/// The compiler gets, per mapping, the feedback the graph already holds:
/// every control change so far while a screen edit is on its way, and
/// otherwise the last one applied.
#[test]
fn synced_counts_pending_edits_as_newer() {
    let feedback = Arc::new(MidiFeedback::new());
    let mut midi = MidiSync::new(feedback.clone());
    let id = midi.new_mapping();
    let mut graph = mapped(id);
    assert!(midi.synced(&graph).is_empty(), "nothing applied yet");

    feedback.write(KNOB, id, 3, 64);
    midi.sync(&mut graph);
    assert_eq!(midi.synced(&graph), BTreeMap::from([(id, 3)]));

    midi.record_edit(id);
    feedback.write(KNOB, id, 4, 10);
    assert_eq!(midi.synced(&graph), BTreeMap::from([(id, 4)]));

    midi.forget(id);
    assert!(midi.synced(&graph).is_empty(), "a forgotten mapping holds nothing");
}
