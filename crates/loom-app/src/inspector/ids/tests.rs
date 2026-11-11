use super::*;

/// A row's ID is the tile, the kind of item, and its name, and the parts
/// of a row are named under it. Every ID reads back from its text.
#[test]
fn rows_and_their_parts_are_keyed_by_what_they_show() {
    let own = row(4, Kind::Param, "cutoff");
    assert_eq!(own.to_string(), "inspector/4/param/cutoff");
    let exposed = row(7, Kind::Exposed, "Cutoff");
    assert_eq!(exposed.to_string(), "inspector/7/exposed/Cutoff");
    assert_ne!(own, exposed);
    assert!(own.is_under(&panel(4)));

    assert_eq!(part(&own, "value").to_string(), "inspector/4/param/cutoff/value");
    assert_eq!(part(&own, "midi/low").to_string(), "inspector/4/param/cutoff/midi/low");
    assert_eq!(part(&panel(4), "help").to_string(), "inspector/4/help");
    assert_eq!(
        assignment(&own, 12).to_string(),
        "inspector/4/param/cutoff/assignment/12"
    );

    for id in [
        own,
        exposed,
        part(&panel(4), "reset_clip"),
        assignment(&row(4, Kind::Param, "mix"), 3),
    ] {
        let text = id.to_string();
        assert_eq!(text.parse::<UiId>().unwrap(), id, "{text}");
    }
    // A name with a slash stays one key.
    let odd = row(4, Kind::Exposed, "in/out");
    assert_eq!(odd.keys().len(), 4);
    assert_eq!(odd.to_string().parse::<UiId>().unwrap(), odd);
}
