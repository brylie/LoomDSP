use super::*;

/// A block's text is one item, and each of its links an item of its own
/// after it, named by its words, that follows its target.
#[test]
fn a_link_is_its_own_item_after_its_block() {
    let blocks = help::markdown::parse("Go [there](a.md).").blocks;
    let items = notes(&help_id().with("notes"), &blocks);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].kind, Kind::Text);
    assert_eq!(items[1].kind, Kind::Link);
    assert_eq!(items[1].label.as_deref(), Some("there"));
    assert!(matches!(&items[1].click, Some(Message::HelpLink(target)) if target == "a.md"));
}
