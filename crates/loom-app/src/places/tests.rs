//! Tests for the names the accessibility layer gives text fields
//! (`places.rs`).

use super::*;

/// A field's widget ID reads back as its part, for UI IDs whose text needs
/// escaping: a name with a slash, a name of digits, a number, quotes and a
/// backslash, and a name in another script.
#[test]
fn a_field_id_reads_back_as_its_part() {
    let parts = [
        UiId::panel("inspector")
            .with(4u64)
            .with("param")
            .with("cutoff")
            .with("field"),
        UiId::panel("canvas").with(3u64).with("port").with("in/out"),
        UiId::panel("browser").with("search"),
        UiId::panel("files").with("12"),
        UiId::panel("tags").with("say \"hi\" \\ there"),
        UiId::panel("patches").with("écho 音"),
    ];
    for part in parts {
        assert_eq!(field_of(&field_widget_id(&part)), Some(part.clone()), "{part}");
    }
}

/// Only an ID the pair made reads as a field: not a named container, even
/// one whose UI ID ends in `text` as the question's sentence does, nor an
/// ID Iced made up, nor a plain string.
#[test]
fn only_a_field_id_reads_as_a_field() {
    let sentence: UiId = "question/text".parse().unwrap();
    assert_eq!(field_of(&Id::from(sentence.to_string())), None);
    assert_eq!(field_of(&Id::unique()), None);
    assert_eq!(field_of(&Id::new("module-browser-search")), None);
    assert_eq!(
        field_of(&Id::new("field:bad\"quote")),
        None,
        "not a character the pair writes"
    );
    assert_eq!(field_of(&Id::new("field:%ZZ")), None, "a broken escape");
}

/// The text between Iced's quotes is the ID itself: a field's ID holds
/// nothing `Debug` would escape. If Iced prints IDs another way, this
/// fails, and so does every focus read.
#[test]
fn iced_prints_a_field_id_as_it_is() {
    let id = field_widget_id(&UiId::panel("tags").with("a \"b\""));
    let printed = format!("{id:?}");
    assert!(printed.starts_with("Id(Custom(\"field:tags/"), "{printed}");
    assert!(!printed.contains('\\'), "{printed}");
}
