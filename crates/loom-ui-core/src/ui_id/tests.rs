//! Tests for UI IDs and the number table, each rule checked against itself.

use super::*;

/// The ID of a button in a toolbar.
fn save() -> UiId {
    UiId::panel("toolbar").with("save")
}

/// Two IDs built from the same keys are equal, and hash alike.
#[test]
fn ids_built_from_the_same_keys_are_equal() {
    assert_eq!(save(), UiId::panel("toolbar").with("save"));
    let mut table = IdTable::new();
    assert_eq!(
        table.number(&save()),
        table.number(&UiId::panel("toolbar").with("save"))
    );
}

/// A number and a name that read alike are different keys.
#[test]
fn a_number_is_not_the_name_that_reads_like_it() {
    assert_ne!(IdKey::from(1u32), IdKey::from("1"));
    assert_ne!(UiId::panel("files").with(1u32), UiId::panel("files").with("1"));
}

/// Every way to make a key gives the key it says.
#[test]
fn keys_convert_from_numbers_and_names() {
    assert_eq!(IdKey::from(7u32), IdKey::Number(7));
    assert_eq!(IdKey::from(7u64), IdKey::Number(7));
    assert_eq!(IdKey::from("open"), IdKey::Name("open".into()));
    assert_eq!(IdKey::from(String::from("open")), IdKey::Name("open".into()));
}

/// The keys come back in order, from the panel inwards.
#[test]
fn keys_are_kept_in_order() {
    let id = UiId::panel("files").with(12u32).with("rename");
    assert_eq!(
        id.keys(),
        [
            IdKey::Name("files".into()),
            IdKey::Number(12),
            IdKey::Name("rename".into())
        ]
    );
}

/// The order of keys matters: the same keys in another order is another ID.
#[test]
fn key_order_matters() {
    assert_ne!(UiId::panel("a").with("b"), UiId::panel("b").with("a"));
}

/// An ID is under itself and under each ID it was built from, and under
/// nothing else.
#[test]
fn is_under_follows_the_path() {
    let toolbar = UiId::panel("toolbar");
    assert!(toolbar.is_under(&toolbar));
    assert!(save().is_under(&toolbar));
    assert!(save().with("icon").is_under(&save()));
    assert!(!toolbar.is_under(&save()), "an outer ID is not under an inner one");
    assert!(!save().is_under(&UiId::panel("files")));
    assert!(
        !UiId::panel("toolbar2").is_under(&toolbar),
        "a key matches whole, not by prefix"
    );
}

/// A new table is empty, and its default is the same.
#[test]
fn a_new_table_is_empty() {
    assert!(IdTable::new().is_empty());
    assert_eq!(IdTable::default().len(), 0);
}

/// Different IDs get different numbers, and each number leads back to its ID.
#[test]
fn each_id_gets_its_own_number() {
    let mut table = IdTable::new();
    let open = UiId::panel("toolbar").with("open");
    let a = table.number(&save());
    let b = table.number(&open);
    assert_ne!(a, b);
    assert_eq!(table.id(a), Some(&save()));
    assert_eq!(table.id(b), Some(&open));
    assert_eq!(table.len(), 2);
    assert!(!table.is_empty());
}

/// A number nobody was given leads to no ID.
#[test]
fn an_unknown_number_has_no_id() {
    let mut table = IdTable::new();
    let number = table.number(&save());
    assert_eq!(table.id(number + 1), None);
    assert_eq!(table.id(0), None);
}

/// An ID asked for on every build keeps its number, however many builds.
#[test]
fn an_id_asked_for_on_every_build_keeps_its_number() {
    let mut table = IdTable::new();
    let first = table.number(&save());
    for _ in 0..5 {
        table.retain_seen();
        assert_eq!(table.number(&save()), first);
    }
    assert_eq!(table.len(), 1);
}

/// An ID not asked for during a build is removed, with its number.
#[test]
fn an_id_not_asked_for_is_removed() {
    let mut table = IdTable::new();
    let open = UiId::panel("toolbar").with("open");
    let gone = table.number(&save());
    let kept = table.number(&open);
    table.retain_seen();

    // The next build shows only Open.
    table.number(&open);
    table.retain_seen();

    assert_eq!(table.len(), 1);
    assert_eq!(table.id(gone), None);
    assert_eq!(table.id(kept), Some(&open));
}

/// An ID that went away and came back gets a new number, never the old one,
/// and never one another ID has or had.
#[test]
fn an_id_that_comes_back_gets_a_new_number() {
    let mut table = IdTable::new();
    let open = UiId::panel("toolbar").with("open");
    let old = table.number(&save());
    let other = table.number(&open);
    table.retain_seen();
    table.number(&open);
    table.retain_seen();

    let new = table.number(&save());
    assert_ne!(new, old);
    assert_ne!(new, other);
    assert!(new > old.max(other), "the counter only goes up");
    assert_eq!(table.id(old), None, "the old number stays unused");
}

/// Being seen lasts for one build only: an ID seen in the last build but not
/// in this one is removed.
#[test]
fn seen_is_cleared_by_each_sweep() {
    let mut table = IdTable::new();
    table.number(&save());
    table.retain_seen();
    assert_eq!(table.len(), 1, "seen in the first build");
    table.retain_seen();
    assert_eq!(table.len(), 0, "not seen in the second");
}

/// Forgetting a part drops it and everything inside it, even if just seen,
/// and leaves the rest.
#[test]
fn forget_drops_a_part_and_what_is_inside_it() {
    let mut table = IdTable::new();
    let files = UiId::panel("files");
    let row = files.clone().with(12u32);
    let old_files = table.number(&files);
    let old_row = table.number(&row);
    let toolbar = table.number(&save());

    table.forget(&files);

    assert_eq!(table.len(), 1);
    assert_eq!(table.id(old_files), None);
    assert_eq!(table.id(old_row), None);
    assert_eq!(table.number(&save()), toolbar);
    assert_ne!(table.number(&row), old_row, "the same keys now name a new element");
}

/// The text form joins the keys with `/`, numbers as digits.
#[test]
fn an_id_is_written_as_a_slash_path() {
    assert_eq!(save().to_string(), "toolbar/save");
    assert_eq!(
        UiId::panel("files").with(12u32).with("rename").to_string(),
        "files/12/rename"
    );
    assert_eq!(UiId::panel(7u32).to_string(), "7");
}

/// A `/` or a `%` inside a name is escaped, so the name stays one key.
#[test]
fn a_slash_or_percent_in_a_name_is_escaped() {
    let id = UiId::panel("files").with("docs/notes 100%.txt");
    assert_eq!(id.to_string(), "files/docs%2Fnotes 100%25.txt");
    assert_eq!(id.to_string().parse::<UiId>(), Ok(id));
}

/// A name made only of digits is written so that it does not read back as
/// a number, and a number stays a number.
#[test]
fn a_name_of_digits_does_not_come_back_as_a_number() {
    let name = UiId::panel("files").with("12");
    let number = UiId::panel("files").with(12u32);
    assert_eq!(name.to_string(), "files/%312");
    assert_eq!(number.to_string(), "files/12");
    assert_eq!(name.to_string().parse::<UiId>(), Ok(name));
    assert_eq!(number.to_string().parse::<UiId>(), Ok(number));
}

/// Writing an ID and reading the text gives the same ID, for names that are
/// empty, hold other scripts, look like escapes, or are long digit strings.
#[test]
fn every_id_round_trips_through_text() {
    let names = [
        "",
        "a",
        "0",
        "007",
        "1a",
        "a1",
        "%",
        "%2F",
        "%31",
        "/",
        "//",
        "a/b/c",
        " ",
        "é",
        "音",
        "🎛",
        "a%",
        "99999999999999999999999999",
    ];
    for name in names {
        for id in [
            UiId::panel(name),
            UiId::panel("p").with(name),
            UiId::panel(name).with(u64::MAX).with(name).with(0u32),
        ] {
            let text = id.to_string();
            assert_eq!(text.parse::<UiId>(), Ok(id.clone()), "{id:?} as `{text}`");
        }
    }
}

/// Reading takes any escaped byte, and leading zeros in a number.
#[test]
fn reading_accepts_any_escape_and_leading_zeros() {
    assert_eq!("%61b".parse::<UiId>(), Ok(UiId::panel("ab")));
    assert_eq!(
        "a%2fb".parse::<UiId>(),
        Ok(UiId::panel("a/b")),
        "hex digits in either case"
    );
    assert_eq!("p/007".parse::<UiId>(), Ok(UiId::panel("p").with(7u32)));
    assert_eq!("p//q".parse::<UiId>(), Ok(UiId::panel("p").with("").with("q")));
}

/// Text that is not an ID says why, and names the key at fault.
#[test]
fn bad_text_is_refused_with_a_reason() {
    assert_eq!("a/b%".parse::<UiId>(), Err(ParseUiIdError::BadEscape("b%".into())));
    assert_eq!("a%2".parse::<UiId>(), Err(ParseUiIdError::BadEscape("a%2".into())));
    assert_eq!("a%zz".parse::<UiId>(), Err(ParseUiIdError::BadEscape("a%zz".into())));
    assert_eq!("a%FF".parse::<UiId>(), Err(ParseUiIdError::NotText("a%FF".into())));
    let huge = "99999999999999999999999999";
    assert_eq!(huge.parse::<UiId>(), Err(ParseUiIdError::NumberTooLarge(huge.into())));
    for error in [
        ParseUiIdError::BadEscape("b%".into()),
        ParseUiIdError::NotText("a%FF".into()),
        ParseUiIdError::NumberTooLarge(huge.into()),
    ] {
        assert!(error.to_string().contains('`'), "the message quotes the key");
    }
}
