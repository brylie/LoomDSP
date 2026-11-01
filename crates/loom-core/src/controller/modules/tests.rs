use super::*;

/// A module instance that does nothing, to fill a slot.
struct Silent;

impl crate::Module for Silent {
    fn process(&mut self, _ctx: &crate::ProcessContext<'_>, _io: &mut crate::PortIo<'_>) {}
}

/// A new module with id `id`, as `add_module` holds it.
fn fresh(id: u32) -> Fresh {
    Fresh {
        module: Box::new(Silent),
        id,
        displays: 0,
        triggers: 0,
    }
}

/// Slots are taken from 0 up, and a removed module the engine never had
/// frees its slot at once: there is nothing for the engine to remove.
#[test]
fn a_module_never_sent_frees_its_slot_at_once() {
    let mut slots = Slots::new(3);
    let slot = slots.take().unwrap();
    assert_eq!(slot, 0);
    slots.hold(slot, fresh(1));
    slots.release(slot);
    assert_eq!(slots.free_count(), 3);
    assert!(slots.retiring().is_empty());
    assert_eq!(slots.take(), Some(0));
}

/// A module in the running schedule keeps its slot until the swap that
/// stops using it, so the engine never runs a slot that was given away.
#[test]
fn a_running_module_retires_until_the_swap() {
    let mut slots = Slots::new(2);
    let slot = slots.take().unwrap();
    slots.hold(slot, fresh(1));
    assert_eq!(slots.send_fresh(1).len(), 1);
    slots.release(slot);
    assert_eq!(slots.retiring(), &[slot]);
    assert_eq!(slots.free_count(), 1);
    slots.free_retired();
    assert!(slots.retiring().is_empty());
    assert_eq!(slots.free_count(), 2);
}

/// With room for only some new modules, the lowest slots go first and the
/// rest stay held, still editable, for the next commit.
#[test]
fn new_modules_go_lowest_slot_first_and_the_rest_wait() {
    let mut slots = Slots::new(4);
    for id in 1..=3 {
        let slot = slots.take().unwrap();
        slots.hold(slot, fresh(id));
    }
    let sent: Vec<(usize, u32)> = slots.send_fresh(2).into_iter().map(|(slot, f)| (slot, f.id)).collect();
    assert_eq!(sent, [(0, 1), (1, 2)]);
    assert_eq!(slots.waiting(), 1);
    assert!(slots.fresh_mut(2).is_some());
    assert!(slots.fresh_mut(0).is_none());
}
