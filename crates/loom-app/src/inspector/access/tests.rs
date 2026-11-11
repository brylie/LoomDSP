use super::*;

/// What shows when no one module is selected: the number selected, or
/// nothing at all.
#[test]
fn several_and_none() {
    let several = items(&Inspector::Several(3));
    assert_eq!(several.len(), 1);
    assert_eq!(several[0].kind, Kind::Text);
    assert_eq!(several[0].value.as_deref(), Some("3 modules selected"));
    assert!(items(&Inspector::Empty).is_empty());
}

/// With nothing selected the panel is the patch's own: its name, the
/// authors field (read only), the tag field, the licence, and the help.
#[test]
fn the_patch_inspector() {
    let app = crate::tests::app();
    let model = app.inspector_model();
    let panel = items(&model);
    assert_eq!(panel.len(), 1);
    let all = panel[0].walk();
    let authors = all
        .iter()
        .find(|item| item.id == ids::part(&ids::patch(), "authors"))
        .unwrap();
    assert_eq!(authors.kind, Kind::Field);
    assert!(authors.set_text.is_none(), "read only until phase 3");
    assert!(authors.focus.is_some());
    let tags = all
        .iter()
        .find(|item| item.id == ids::part(&ids::patch(), "tags"))
        .unwrap();
    assert!(matches!(tags.set_text, Some(SetText::Tag)));
    for name in ["choose_licence", "shortcuts"] {
        let button = all
            .iter()
            .find(|item| item.id == ids::part(&ids::patch(), name))
            .unwrap();
        assert_eq!(button.kind, Kind::Button);
        assert!(button.click.is_some());
    }
}

/// A sub-patch tile's port tile shows its name, direction, and what it is.
#[test]
fn a_port_tile() {
    let rows = super::super::PortRows {
        id: 12,
        direction: "Patch input".into(),
        name: "pitch".into(),
    };
    let panel = items(&Inspector::Port(rows));
    assert_eq!(panel[0].label.as_deref(), Some("pitch"));
    assert_eq!(panel[0].children.len(), 2);
    assert!(panel[0].children.iter().all(|item| item.kind == Kind::Text));
}
