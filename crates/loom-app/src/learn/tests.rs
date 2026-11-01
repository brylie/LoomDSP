use super::*;
use loom_core::{EngineConfig, MidiControl, create_engine};

const CC74: MidiControl = MidiControl {
    channel: 0,
    controller: 74,
};

#[test]
fn labels_follow_mappings_and_the_armed_control() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let controller = &mut parts.controller;
    let filter = controller.add_module("loom.filter.svf").unwrap();
    let d = controller.graph().node(filter).unwrap().descriptor;
    let cutoff = d.param_index("cutoff").unwrap();
    let resonance = d.param_index("resonance").unwrap();
    let widget_of = |param: usize| {
        d.panel
            .widgets
            .iter()
            .position(|w| d.param_index(w.bind) == Some(param))
            .unwrap()
    };
    controller.map_midi(CC74, filter, cutoff).unwrap();

    let view = view(
        controller,
        Some(Armed {
            param: (filter.0, resonance),
            serial: None,
        }),
    );
    assert_eq!(view.armed, Some((filter.0, widget_of(resonance))));
    assert_eq!(view.labels.len(), 1);
    assert_eq!(view.labels[&(filter.0, widget_of(cutoff))], "CC 74");
}

#[test]
fn the_status_names_the_parameter_a_control_left() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let controller = &mut parts.controller;
    let filter = controller.add_module("loom.filter.svf").unwrap();
    controller.map_midi_named(CC74, filter, "cutoff").unwrap();
    let (_, removed) = controller.map_midi_named(CC74, filter, "resonance").unwrap();
    let (_, mapping) = controller.graph().midi_mapping_on(CC74).unwrap();
    let status = mapped_status(controller, *mapping, &removed);
    assert!(status.starts_with("Mapped CC 74 · ch 1 to "), "{status}");
    assert!(status.contains("no longer moves"), "{status}");
    assert!(status.ends_with("Cutoff"), "{status}");

    // A new mapping on a free control says nothing about a move.
    let (_, removed) = controller
        .map_midi_named(MidiControl { controller: 71, ..CC74 }, filter, "cutoff")
        .unwrap();
    let (_, mapping) = controller.graph().midi_mapping_of(filter, 0).unwrap();
    let status = mapped_status(controller, *mapping, &removed);
    assert!(!status.contains("no longer"), "{status}");
}
