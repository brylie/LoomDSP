use super::*;
use crate::{help, question};

/// A panel for each kind, with no window behind it.
fn recent() -> Overlay {
    Overlay::RecentList(Vec::new())
}

/// Opening a panel closes the one that was open: only one can be.
#[test]
fn a_new_panel_takes_the_place_of_the_open_one() {
    let mut overlays = Overlays::default();
    overlays.open(recent());
    overlays.open(Overlay::Menu(ContextMenu::Tile {
        module: 1,
        at: Point::ORIGIN,
    }));

    assert!(overlays.recent_rows().is_none());
    assert_eq!(overlays.owner(), InputOwner::TileMenu);
}

/// Closing one panel leaves another alone.
#[test]
fn closing_another_panel_changes_nothing() {
    let mut overlays = Overlays::default();
    overlays.open(recent());

    overlays.close_browser();
    overlays.close_patches();
    overlays.close_preferences();
    overlays.close_tile_menu();
    overlays.close_probe_menu();

    assert_eq!(overlays.owner(), InputOwner::RecentList);
    overlays.close_recent_list();
    assert_eq!(overlays.owner(), InputOwner::Canvas);
}

/// The slots stack over the panel, in the order the view draws them.
#[test]
fn the_top_layer_has_the_input() {
    let mut overlays = Overlays::default();
    overlays.open(recent());
    overlays.chooser = Some(licence_chooser::Chooser::open(licence_chooser::Target::Patch, None));
    assert_eq!(overlays.owner(), InputOwner::Chooser);
    overlays.help = Some(help::Help::shortcuts());
    assert_eq!(overlays.owner(), InputOwner::Help);
    overlays.question = Some(question::Question::error("no".to_string()));
    assert_eq!(overlays.owner(), InputOwner::Question);
}

/// A new patch closes everything, slots included, and a panel other than
/// Preferences.
#[test]
fn a_new_patch_closes_what_is_about_the_patch() {
    let mut overlays = Overlays::default();
    overlays.open(recent());
    overlays.help = Some(help::Help::shortcuts());
    overlays.question = Some(question::Question::error("no".to_string()));

    overlays.close_for_new_patch();

    assert_eq!(overlays.owner(), InputOwner::Canvas);
}
