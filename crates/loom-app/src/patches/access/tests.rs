use super::super::{PatchBrowser, Row, patch_files};
use super::*;
use loom_ui_core::UiId;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// A library folder with a patch in a subfolder, one at the top, and a
/// file that does not read.
fn library(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-patches-access-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("live")).unwrap();
    let write = |file: &str, text: &str| std::fs::write(dir.join(file), text).unwrap();
    write(
        "live/bass.ron",
        r#"Patch(version: 1, metadata: Metadata(name: "Bass", authors: ["Sam Rivera"], tags: ["drone", "808"], license: "MIT"))"#,
    );
    write("pad.ron", r#"Patch(version: 1, metadata: Metadata(name: "Pad"))"#);
    write("broken.ron", "not a patch");
    dir
}

/// A browser that has read every file in `dir`, one file per batch.
fn browser(dir: &Path) -> PatchBrowser {
    let mut browser = PatchBrowser::new(1, dir.to_path_buf());
    for path in patch_files(dir, &AtomicBool::new(false)) {
        let _ = browser.update(Message::Found {
            open: 1,
            rows: vec![Row::read(&path)],
        });
    }
    browser
}

/// The option of the row named `name`.
fn option<'a>(dialog: &'a Item, name: &str) -> &'a Item {
    let list = dialog.children.iter().find(|item| item.kind == Kind::ListBox).unwrap();
    list.children
        .iter()
        .find(|option| option.label.as_deref().is_some_and(|label| label.starts_with(name)))
        .unwrap_or_else(|| panic!("a row for {name}"))
}

/// Your patches is a dialog with Close, its two fields, a check box per
/// licence filter keyed by its fixed key, a toggle button per tag, and a
/// list box of the rows, keyed by path in the library folder.
#[test]
fn your_patches_is_a_dialog_with_its_parts() {
    let dir = library("parts");
    let browser = browser(&dir);
    let dialog = items(browser.model());
    assert_eq!(dialog.kind, Kind::Dialog);
    assert_eq!(dialog.label.as_deref(), Some("Your patches"));
    let find = |id: &str| {
        dialog
            .walk()
            .into_iter()
            .find(|item| item.id.to_string() == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    assert_eq!(find("patches/close").kind, Kind::Button);
    assert!(find("patches/search").focus.is_some());
    assert!(find("patches/author").focus.is_some());
    for key in ["lets-me-change", "allows-commercial-use", "no-licence-stated"] {
        let check = find(&format!("patches/licence/{key}"));
        assert_eq!(check.kind, Kind::Check);
        assert_eq!(check.on, Some(false));
    }
    // A tag of digits alone stays a name, not a number.
    let tag = find("patches/tag/%3808");
    assert_eq!(tag.kind, Kind::Button);
    assert_eq!(tag.on, Some(false));
    let bass = option(&dialog, "Bass");
    assert_eq!(bass.id.to_string(), "patches/row/live%2Fbass.ron");
    assert!(
        bass.description
            .as_deref()
            .is_some_and(|text| text.contains("Sam Rivera"))
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A file that does not read says so in its name, stays enabled so a
/// reader does not skip it, and offers no Click, since it cannot be opened.
#[test]
fn an_unread_row_says_so_and_opens_nothing() {
    let dir = library("unread");
    let browser = browser(&dir);
    let dialog = items(browser.model());
    let broken = option(&dialog, "broken");
    assert_eq!(broken.label.as_deref(), Some("broken.ron, could not read"));
    assert!(broken.enabled);
    assert!(broken.click.is_none());
    assert!(option(&dialog, "Pad").click.is_some());
    let _ = std::fs::remove_dir_all(dir);
}

/// A licence check box and a tag toggle say their state, and a Click
/// sends what a press sends.
#[test]
fn a_licence_box_and_a_tag_toggle() {
    let dir = library("toggles");
    let mut browser = browser(&dir);
    let _ = browser.update(Message::Tag("drone".into()));
    let _ = browser.update(Message::Licence(
        loom_patch::metadata::LicenceFilter::LetsMeChange,
        true,
    ));
    let dialog = items(browser.model());
    let walk = dialog.walk();
    let tag = walk.iter().find(|item| item.id == tag_id("drone")).unwrap();
    assert_eq!(tag.on, Some(true));
    assert!(matches!(&tag.click, Some(AppMessage::Patches(Message::Tag(t))) if t == "drone"));
    let check = walk
        .iter()
        .find(|item| item.id == licence_id("lets-me-change"))
        .unwrap();
    assert_eq!(check.on, Some(true));
    assert!(matches!(
        check.click,
        Some(AppMessage::Patches(Message::Licence(
            loom_patch::metadata::LicenceFilter::LetsMeChange,
            false
        )))
    ));
    let _ = std::fs::remove_dir_all(dir);
}

/// As rows stream in, a row keeps its UI ID, the list's size is the rows
/// shown so far, and the highlight stays on the same patch.
#[test]
fn streamed_rows_keep_their_ids_and_the_list_grows() {
    let dir = library("stream");
    let paths = patch_files(&dir, &AtomicBool::new(false));
    let mut browser = PatchBrowser::new(1, dir.clone());
    let mut seen: Vec<UiId> = Vec::new();
    for path in &paths {
        let highlighted = browser.model().highlight.map(|at| browser.model().rows[at].id.clone());
        let _ = browser.update(Message::Found {
            open: 1,
            rows: vec![Row::read(path)],
        });
        let model = browser.model();
        let ids: Vec<UiId> = model.rows.iter().map(|row| row.id.clone()).collect();
        assert!(seen.iter().all(|id| ids.contains(id)), "rows keep their IDs");
        let dialog = items(model);
        let list = dialog.children.iter().find(|item| item.kind == Kind::ListBox).unwrap();
        assert_eq!(list.children.len(), model.rows.len());
        assert_eq!(model.loaded, model.rows.len());
        if let Some(before) = highlighted {
            let now = model.highlight.map(|at| &model.rows[at].id);
            assert_eq!(now, Some(&before), "the highlight stays on its patch");
        }
        seen = ids;
    }
    assert_eq!(seen.len(), paths.len());
    let _ = std::fs::remove_dir_all(dir);
}
