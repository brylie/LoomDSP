use super::*;
use crate::live::LiveTable;

/// A port of module `module`.
fn port(module: u32) -> OutputRef {
    OutputRef {
        module: ModuleId(module),
        port: 0,
    }
}

/// Probe ids count up, and removing a module removes only its probes.
#[test]
fn removing_a_module_removes_only_its_probes() {
    let mut probes = Probes::new();
    let first = probes.add(port(1), ProbeKind::Meter);
    let second = probes.add(port(2), ProbeKind::Meter);
    assert!(second.0 > first.0);
    probes.remove_on(ModuleId(1));
    assert_eq!(probes.get(first), None);
    assert_eq!(probes.get(second), Some((port(2), ProbeKind::Meter)));
    assert_eq!(probes.iter().count(), 1);
    assert_eq!(probes.remove(first), None, "already gone");
    assert!(probes.remove(second).is_some());
    assert!(probes.as_map().is_empty());
}

/// A modulated parameter reads its live value only once it has a cell
/// and the engine has written to it.
#[test]
fn a_live_value_needs_a_cell_and_a_write() {
    let live = Arc::new(LiveTable::new(2));
    let mut values = LiveValues::new(Arc::new(DisplayTable::new(1)), live.clone(), 2);
    let key = live_key(ModuleId(3), 1);
    assert_eq!(values.modulated(ModuleId(3), 1), None, "no cell yet");

    let (released, missing) = values.assign(&[key]);
    assert!(released.is_empty());
    assert_eq!(missing, 0);
    let cell = values.cell(key).unwrap();
    live.claim(cell, key);
    assert_eq!(values.modulated(ModuleId(3), 1), None, "no value yet");
    live.write(cell, 0.75);
    assert_eq!(values.modulated(ModuleId(3), 1), Some(0.75));

    let (released, _) = values.assign(&[]);
    assert_eq!(released, [cell]);
    assert_eq!(values.modulated(ModuleId(3), 1), None);
}
