use super::*;
use std::cell::Cell;

/// It computes once per change of key, and not for a repeated key.
#[test]
fn computes_only_when_the_key_changes() {
    let count = Cell::new(0);
    let compute = |key: f32| {
        count.set(count.get() + 1);
        key * 2.0
    };
    let mut cached = Cached::new(1.0, compute);
    assert_eq!(cached.get(1.0, compute), 2.0);
    assert_eq!(count.get(), 1);
    assert_eq!(cached.get(3.0, compute), 6.0);
    assert_eq!(cached.get(3.0, compute), 6.0);
    assert_eq!(count.get(), 2);
}
