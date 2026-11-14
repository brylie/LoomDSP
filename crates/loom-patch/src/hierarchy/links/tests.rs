use super::*;

/// A link on `level` from `from` to `to`.
fn link(level: &[u32], from: (u32, &str), to: (u32, &str)) -> Link {
    Link {
        level: ModulePath(level.to_vec()),
        from: (from.0, from.1.to_string()),
        to: (to.0, to.1.to_string()),
    }
}

/// The two orders stay in step through insert, remove, and retain, and the
/// range queries find a level's links, the one link into an input, and the
/// links out of an output, never from another level.
#[test]
fn both_orders_stay_in_step_and_find_by_range() {
    let mut set: LinkSet = [
        link(&[], (1, "out"), (2, "in")),
        link(&[], (1, "out"), (3, "in")),
        link(&[2], (1, "out"), (4, "in")),
        link(&[2], (5, "out"), (1, "in")),
    ]
    .into_iter()
    .collect();
    let top = ModulePath(Vec::new());
    let inside = ModulePath(vec![2]);
    assert_eq!(set.len(), 4);
    assert_eq!(set.at_level(&top).count(), 2);
    assert_eq!(set.at_level(&inside).count(), 2);
    assert_eq!(
        set.linked_into(&top, &(3, "in".into())),
        Some(&link(&[], (1, "out"), (3, "in")))
    );
    assert_eq!(
        set.linked_into(&top, &(4, "in".into())),
        None,
        "that input is on another level"
    );
    let out: Vec<Link> = set.linked_from(&top, &(1, "out".into())).collect();
    assert_eq!(
        out,
        [link(&[], (1, "out"), (2, "in")), link(&[], (1, "out"), (3, "in"))]
    );
    set.retain(|l| l.to.0 != 2);
    assert_eq!(
        set.linked_from(&top, &(1, "out".into())).count(),
        1,
        "retain keeps both orders"
    );
    assert!(set.remove(&link(&[2], (5, "out"), (1, "in"))));
    assert_eq!(set.at_level(&inside).count(), 1);
    assert_eq!(set.into_iter().count(), 2);
}
