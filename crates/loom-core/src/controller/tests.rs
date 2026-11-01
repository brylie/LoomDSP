use super::*;
use crate::module::{Module, PortIo, ProcessContext};
use crate::{Panel, ParamOptions, ParamSpec, Widget};
use std::sync::atomic::Ordering;

struct Silent;

impl Module for Silent {
    fn process(&mut self, _ctx: &ProcessContext<'_>, _io: &mut PortIo<'_>) {}
}

const fn parameter(name: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        label: name,
        description: "A test parameter.",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer()
}

static GATE: ModuleDescriptor = ModuleDescriptor {
    id: "test.gate",
    version: 1,
    name: "Gate",
    category: "Test",
    family: crate::Family::Interface,
    description: "A button for the queue test.",
    inputs: &[],
    outputs: &[],
    params: &[parameter("mode"), parameter("on"), parameter("hold").transient()],
    role: ModuleRole::Dsp(|_| Box::new(Silent)),
    panel: Panel::new(6, 3, &[Widget::button(0, 0, "on", "hold", "mode")]),
    displays: &[],
};

fn controller(capacity: usize) -> Controller {
    let mut registry = Registry::new();
    registry.register(&GATE).expect("test module is valid");
    let config = EngineConfig {
        command_capacity: capacity,
        ..EngineConfig::default()
    };
    create_engine(Arc::new(registry), config).controller
}

#[test]
fn set_params_writes_both_or_neither() {
    let mut room = controller(8);
    let id = room.add_module(GATE.id).unwrap();
    room.commit().unwrap();
    room.set_param_named(id, "on", 1.0).unwrap();
    room.set_params(id, &[(0, 1.0), (1, 0.0)]).unwrap();
    assert_eq!(room.graph().node(id).unwrap().params, vec![1.0, 0.0, 0.0]);

    // Inserting the module and swapping in its schedule fill a queue of
    // two. Two parameter commands do not fit, so neither value changes.
    let mut full = controller(2);
    let id = full.add_module(GATE.id).unwrap();
    full.commit().unwrap();
    let error = full.set_params(id, &[(0, 1.0), (1, 0.0)]).unwrap_err();
    assert!(matches!(error, LoomError::QueueFull), "{error}");
    assert_eq!(full.graph().node(id).unwrap().params, vec![0.0, 0.0, 0.0]);
}

/// A transient parameter is sent to the engine like any other, but it
/// cannot be a modulation target or a MIDI mapping target.
#[test]
fn a_transient_parameter_cannot_be_modulated_or_mapped() {
    let mut room = controller(16);
    let id = room.add_module(GATE.id).unwrap();
    room.commit().unwrap();
    assert_eq!(room.set_param_named(id, "hold", 1.0).unwrap(), 1.0);
    let control = MidiControl {
        channel: 0,
        controller: 1,
    };
    let error = room.map_midi(control, id, 2).unwrap_err();
    assert!(matches!(error, LoomError::TransientParam { .. }), "{error}");
    assert!(room.map_midi(control, id, 1).is_ok());
}

/// The largest module ID is refused with an error, not an overflow
/// panic, and an ID just below it still leaves room for the next free
/// one (#44).
#[test]
fn module_ids_past_the_largest_are_refused() {
    let mut room = controller(8);
    let error = room.add_module_with_id(ModuleId(u32::MAX), GATE.id).unwrap_err();
    assert!(matches!(error, LoomError::ModuleIdOutOfRange(_)), "{error}");
    room.add_module_with_id(ModuleId::MAX, GATE.id).unwrap();
    let error = room.add_module(GATE.id).unwrap_err();
    assert!(matches!(error, LoomError::ModuleIdOutOfRange(_)), "{error}");
}

/// An engine that no device plays takes edits for as long as someone
/// calls `Engine::apply_pending` (#220). It renders nothing: no buffer
/// is counted, and nothing leaks, because replaced schedules come back
/// to the controller.
#[test]
fn apply_pending_drains_the_queue_without_rendering() {
    let mut registry = Registry::new();
    registry.register(&GATE).expect("test module is valid");
    let config = EngineConfig {
        command_capacity: 4,
        ..EngineConfig::default()
    };
    let parts = create_engine(Arc::new(registry), config);
    let (mut room, mut engine) = (parts.controller, parts.engine);
    let id = room.add_module(GATE.id).unwrap();
    room.commit().unwrap();
    engine.apply_pending();
    let setup_swaps = room.stats().schedule_swaps.load(Ordering::Relaxed);
    let mut extra = None;
    for i in 0..1000 {
        room.set_param_named(id, "on", (i % 2) as f32).unwrap();
        // Adding or removing a module makes the commit swap in a new
        // schedule, so the old one comes back as garbage.
        match extra.take() {
            None => extra = Some(room.add_module(GATE.id).unwrap()),
            Some(module) => room.remove_module(module).unwrap(),
        }
        room.commit().unwrap();
        engine.apply_pending();
        room.collect_garbage();
    }
    let stats = room.stats();
    assert_eq!(stats.buffers.load(Ordering::Relaxed), 0, "nothing rendered");
    assert_eq!(stats.schedule_swaps.load(Ordering::Relaxed), setup_swaps + 1000);
    assert_eq!(stats.leaked.load(Ordering::Relaxed), 0, "nothing leaked");
}
