use super::*;

/// A flag keeps any trigger until it is taken, once.
#[test]
fn a_trigger_flag_is_taken_once() {
    let flag = TriggerFlag::default();
    assert_eq!(flag.take(), 0.0);
    flag.record(true);
    flag.record(false);
    assert_eq!(flag.take(), 1.0);
    assert_eq!(flag.take(), 0.0);
    flag.record(true);
    flag.clear();
    assert_eq!(flag.take(), 0.0);
}

#[test]
fn values_belong_to_their_owner() {
    let table = DisplayTable::new(2);
    table.claim(1, 7, 0);
    table.write(1, &[0.5, 0.25]);
    assert_eq!(table.read(1, 7, 0, false), 0.5);
    assert_eq!(table.read(1, 7, 1, false), 0.25);
    // Another module, or an empty slot, reads 0.
    assert_eq!(table.read(1, 8, 0, false), 0.0);
    assert_eq!(table.read(0, 7, 0, false), 0.0);
}

#[test]
fn a_new_owner_starts_at_zero() {
    let table = DisplayTable::new(1);
    table.claim(0, 7, 0);
    table.write(0, &[1.0, 1.0, 1.0, 1.0]);
    table.release(0);
    assert_eq!(table.read(0, 7, 0, false), 0.0);
    table.claim(0, 9, 0);
    assert!((0..MAX_DISPLAYS).all(|i| table.read(0, 9, i, false) == 0.0));
}

/// A trigger written in one block stays until it is read, even when
/// later blocks write 0, and a read takes it.
#[test]
fn a_trigger_is_latched_until_read() {
    let table = DisplayTable::new(1);
    // Value 1 is a trigger, value 0 a level.
    table.claim(0, 7, 0b10);
    table.write(0, &[0.5, 1.0]);
    table.write(0, &[0.25, 0.0]);
    table.write(0, &[0.125, 0.0]);
    // The level shows the latest block, the trigger any block since the read.
    assert_eq!(table.read(0, 7, 0, false), 0.125);
    assert_eq!(table.read(0, 7, 1, true), 1.0);
    assert_eq!(table.read(0, 7, 1, true), 0.0);
    // Values out of range and NaN are kept to 0 to 1.
    table.write(0, &[0.0, f32::NAN]);
    assert_eq!(table.read(0, 7, 1, true), 0.0);
    table.write(0, &[0.0, -3.0]);
    assert_eq!(table.read(0, 7, 1, true), 0.0);
    table.write(0, &[0.0, 7.0]);
    assert_eq!(table.read(0, 7, 1, true), 1.0);
}
