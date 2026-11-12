use super::*;

/// Notes split into one block per paragraph, heading, list entry, and code
/// block, with a paragraph's lines joined.
#[test]
fn notes_split_into_paragraphs_entries_headings_and_code() {
    let text = "## Use\n\nA first\nparagraph.\n\n- one\n- two\n1. three\n\n```ron\nPatch()\n```\nLast.";
    assert_eq!(
        blocks(text),
        ["Use", "A first paragraph.", "one", "two", "three", "Patch()", "Last."]
    );
}

/// A link reads as its label in the text, and is an item of its own that
/// follows its target. Emphasis and code marks are dropped. A bracket that
/// is not a link stays as it is.
#[test]
fn a_link_reads_as_its_label_and_is_its_own_item() {
    let (words, links) = read_links("See [the **clock**](loom.clock.clock) and [x] `here`.");
    assert_eq!(words, "See the clock and [x] here.");
    assert_eq!(links, [("the clock".to_string(), "loom.clock.clock".to_string())]);
    // A bracket before a link is not joined to it.
    let (words, links) = read_links("See [x] then [there](a.b).");
    assert_eq!(words, "See [x] then there.");
    assert_eq!(links, [("there".to_string(), "a.b".to_string())]);
    let items = notes(&help_id().with("notes"), "Go [there](a.b).");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, Kind::Text);
    assert_eq!(items[1].kind, Kind::Link);
    assert_eq!(items[1].label.as_deref(), Some("there"));
    assert!(matches!(&items[1].click, Some(Message::HelpLink(target)) if target == "a.b"));
}
