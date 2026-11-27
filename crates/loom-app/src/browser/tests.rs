use super::*;

fn modules() -> Vec<&'static ModuleDescriptor> {
    loom_modules::registry().iter().collect()
}

fn ids(found: &[&'static ModuleDescriptor]) -> Vec<&'static str> {
    found.iter().map(|d| d.id).collect()
}

#[test]
fn empty_query_lists_every_module_by_family_then_name() {
    let all = modules();
    let found = search(&all, "  ");
    assert_eq!(found.len(), all.len());
    let keys: Vec<_> = found.iter().map(|d| (d.family, d.name)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
}

/// A family's name finds its modules, and a category still finds
/// its own (ADR-0029).
#[test]
fn family_and_category_names_both_match() {
    let all = modules();
    let modulation = search(&all, "modulation");
    let family = all.iter().filter(|d| d.family == loom_core::Family::Modulation).count();
    assert!(
        modulation.len() >= family && family >= 3,
        "{} of {family}",
        modulation.len()
    );
    assert!(
        search(&all, "filters and effects")
            .iter()
            .any(|d| d.id == "loom.time.delay")
    );
    assert!(search(&all, "oscillator").iter().any(|d| d.id == "loom.osc.sine"));
}

#[test]
fn a_name_that_starts_with_the_query_comes_first() {
    assert_eq!(search(&modules(), "saw")[0].id, "loom.osc.saw");
    // "Phase" starts with "phase"; so do "Phase Reset", "Phase Reverse", "Phase
    // Scale", "Phase Shift", and "Phase Trigger", and they sort after it by name.
    assert_eq!(
        ids(&search(&modules(), "phase"))[..6],
        [
            "loom.phase.generator",
            "loom.phase.reset",
            "loom.phase.reverse",
            "loom.phase.scale",
            "loom.phase.shift",
            "loom.phase.trigger"
        ]
    );
}

#[test]
fn a_word_in_the_name_ranks_above_a_match_elsewhere() {
    // "trig" starts a word of "Phase Trigger".
    assert_eq!(search(&modules(), "trig")[0].id, "loom.phase.trigger");
    // "meter" starts a word of "Level Meter" and is in its ID.
    assert_eq!(search(&modules(), "meter")[0].id, "loom.display.meter");
}

#[test]
fn every_query_word_must_match() {
    let found = search(&modules(), "osc saw");
    assert_eq!(ids(&found), ["loom.osc.saw"]);
    for d in search(&modules(), "oscillator noise") {
        assert_eq!(d.id, "loom.osc.noise");
    }
}

#[test]
fn case_does_not_matter() {
    let found = search(&modules(), "FILTER");
    assert!(!found.is_empty());
    let filters = modules().into_iter().filter(|d| d.category == "Filter").count();
    assert_eq!(found.iter().filter(|d| d.category == "Filter").count(), filters);
}

#[test]
fn no_match_lists_nothing_and_enter_adds_nothing() {
    let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rect::default());
    let _ = browser.update(Message::Query("zzzz".into()));
    assert!(browser.results.is_empty());
    assert!(matches!(browser.update(Message::Submit).0, Outcome::Stay));
    assert!(matches!(browser.update(Message::Key(Nav::Down)).0, Outcome::Stay));
}

#[test]
fn enter_adds_the_highlighted_module_at_the_snapped_point() {
    let area = Rect::new(Point::ORIGIN, loom_ui_core::Size::new(1000.0, 800.0));
    let mut browser = Browser::open(modules(), Point::new(100.4, 29.0), Point::ORIGIN, area);
    let _ = browser.update(Message::Query("saw".into()));
    match browser.update(Message::Submit).0 {
        Outcome::Add(d, at) => {
            assert_eq!(d.id, "loom.osc.saw");
            assert_eq!(at, Point::new(96.0, 24.0).into());
        }
        other => panic!("expected an add, got {other:?}"),
    }
}

#[test]
fn arrows_move_the_highlight_and_stop_at_the_ends() {
    let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rect::default());
    let last = browser.results.len() - 1;
    let _ = browser.update(Message::Key(Nav::Up));
    assert_eq!(browser.highlight(), 0);
    let _ = browser.update(Message::Key(Nav::Down));
    assert_eq!(browser.highlight(), 1);
    // As many pages as it takes, so a new module does not break this test.
    for _ in 0..browser.results.len() {
        let _ = browser.update(Message::Key(Nav::PageDown));
    }
    assert_eq!(browser.highlight(), last);
    let _ = browser.update(Message::Key(Nav::Down));
    assert_eq!(browser.highlight(), last);
    let _ = browser.update(Message::Key(Nav::PageUp));
    assert_eq!(browser.highlight(), last.saturating_sub(ROWS));
    // A new query highlights the first result again.
    let _ = browser.update(Message::Query("a".into()));
    assert_eq!(browser.highlight(), 0);
}

#[test]
fn escape_and_a_click_outside_close_without_adding() {
    let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rect::default());
    assert!(matches!(browser.update(Message::Key(Nav::Escape)).0, Outcome::Close));
    assert!(matches!(browser.update(Message::Close).0, Outcome::Close));
}

#[test]
fn the_panel_stays_inside_the_canvas() {
    let area = Rect::new(Point::new(0.0, 40.0), loom_ui_core::Size::new(1000.0, 700.0));
    assert_eq!(inside(Point::new(100.0, 100.0), area), Point::new(100.0, 100.0));
    let corner = inside(Point::new(990.0, 730.0), area);
    assert_eq!(corner, Point::new(1000.0 - WIDTH, 740.0 - HEIGHT));
}

/// `count` library items, named so a search for "zz-lib" finds only them.
fn library_items(count: usize) -> Vec<LibraryItem> {
    (0..count)
        .map(|i| LibraryItem {
            path: std::path::PathBuf::from(format!("zz-lib {i:02}.ron")),
            library: format!("zz-lib {i:02}.ron"),
            name: format!("zz-lib {i:02}"),
            roles: Vec::new(),
        })
        .collect()
}

/// A browser over every module and every role, with `library` read.
fn with_roles(library: Vec<LibraryItem>) -> Browser {
    let roles = loom_modules::registry().roles().collect();
    let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rect::default()).with_roles(roles);
    let _ = browser.update(Message::Library(library));
    browser
}

/// A library item that declares `roles`.
fn item_with_roles(name: &str, roles: &[&str]) -> LibraryItem {
    LibraryItem {
        path: std::path::PathBuf::from(format!("{name}.ron")),
        library: format!("{name}.ron"),
        name: name.to_string(),
        roles: roles.iter().map(ToString::to_string).collect(),
    }
}

/// The slot rows' role names, in list order.
fn slot_names(browser: &Browser) -> Vec<&'static str> {
    browser.slot_rows().into_iter().map(|(_, role)| role.name).collect()
}

/// One slot row per role a registered module fills, by name, after the
/// modules and before the library items; Shaper and Voice have no part
/// yet, so no row (sub-patches.md, Empty slots).
#[test]
fn slot_rows_list_the_roles_a_part_fills_after_the_modules() {
    let browser = with_roles(vec![item_with_roles("Pad", &[])]);
    assert_eq!(
        slot_names(&browser),
        ["Effect", "Envelope", "Filter", "Modulator", "Sound source"]
    );
    let modules = browser.results().len();
    let rows: Vec<usize> = browser.slot_rows().into_iter().map(|(row, _)| row).collect();
    assert_eq!(rows, (modules..modules + 5).collect::<Vec<_>>());
    assert_eq!(browser.library_rows()[0].0, modules + 5);
    let filter = &browser.model().rows[modules + 2];
    assert_eq!(filter.name, "Filter slot");
    assert_eq!(filter.mark, SLOT_MARK);
    assert!(!filter.library, "a slot row has no menu");
    assert_eq!(filter.id, list_id().with("role").with("loom.role.filter"));
}

/// A library item adds the slot rows of the roles it declares, once each,
/// when the registry knows them: an unknown role or a newer version adds
/// nothing and does not panic.
#[test]
fn a_library_item_adds_rows_only_for_roles_the_registry_knows() {
    let browser = with_roles(vec![
        item_with_roles("Voice", &["loom.role.voice@1", "loom.role.voice@1"]),
        item_with_roles("Fuzz", &["loom.role.shaper@1"]),
        item_with_roles("Bad", &["loom.role.nonsense@1", "loom.role.filter@99", "not a kind"]),
    ]);
    let names = slot_names(&browser);
    assert_eq!(names.iter().filter(|n| **n == "Voice").count(), 1, "{names:?}");
    assert!(names.contains(&"Shaper"));
    assert_eq!(names.len(), 7, "{names:?}");
}

/// The highlight stays on its row, by UI ID, when the library's items
/// arrive and add a slot row above it.
#[test]
fn the_highlight_stays_on_its_row_when_the_library_arrives() {
    let roles = loom_modules::registry().roles().collect();
    let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rect::default()).with_roles(roles);
    let _ = browser.update(Message::Query("slot".into()));
    let source = browser
        .slot_rows()
        .into_iter()
        .find(|(_, role)| role.id == "loom.role.source")
        .unwrap()
        .0;
    for _ in 0..source {
        let _ = browser.update(Message::Key(Nav::Down));
    }
    let before = browser.model().rows[browser.model().highlight.unwrap()].id.clone();
    assert_eq!(before, list_id().with("role").with("loom.role.source"));
    let _ = browser.update(Message::Library(vec![item_with_roles("Fuzz", &["loom.role.shaper@1"])]));
    let after = browser.model().rows[browser.model().highlight.unwrap()].id.clone();
    assert_eq!(after, before, "the Shaper row came in above it");
    assert_eq!(browser.fresh_model(), *browser.model());
}

/// A slot row is found by its role's name, by `slot`, and by its kind; the
/// area under the list says what it does, and a thru role that it passes
/// its input through; picking it adds an empty slot.
#[test]
fn a_slot_row_is_found_described_and_picked() {
    let mut browser = with_roles(Vec::new());
    let _ = browser.update(Message::Query("filter slot".into()));
    assert_eq!(slot_names(&browser), ["Filter"]);
    let _ = browser.update(Message::Query("loom.role.envelope".into()));
    assert_eq!(slot_names(&browser), ["Envelope"]);
    let _ = browser.update(Message::Query("slot".into()));
    assert_eq!(slot_names(&browser).len(), 5);
    let _ = browser.update(Message::Query("filter slot".into()));
    let at = browser.slot_rows()[0].0;
    let _ = browser.update(Message::Hover(at));
    let about = browser.model().about.clone().unwrap();
    assert_eq!(
        about.text,
        "Shapes the spectrum of a signal around a cutoff frequency. Add an empty slot, then swap a part in \
         from its menu or the inspector. While empty, it passes its input through."
    );
    assert_eq!(about.detail, "loom.role.filter@1");
    match browser.update(Message::Pick(at)).0 {
        Outcome::AddSlot(role, _) => assert_eq!(role.id, "loom.role.filter"),
        other => panic!("expected an empty slot, got {other:?}"),
    }
}

/// A descriptive word can complete a multi-word query. A direct name
/// match still leads another module found through its description.
#[test]
fn descriptions_match_below_names() {
    let all = modules();
    let found = search(&all, "noise");
    assert_eq!(found[0].id, "loom.osc.noise");
    assert!(found[1..].iter().any(|d| d.description.contains("noise")));
    assert!(search(&all, "all-pass REVERB").iter().any(|d| d.name == "All-Pass"));
}

/// Adding the same module twice through the toolbar gives each tile
/// visible space. Redo restores the position chosen by that add.
#[test]
fn toolbar_adds_two_tiles_without_covering_transport_or_each_other() {
    use loom_ui_core::{Key, Modifiers};
    let mut ui = crate::ui_test::Ui::with_app(crate::tests::app());
    for _ in 0..2 {
        ui.click_text("Add module");
        ui.redraw();
        ui.typewrite("noise");
        ui.tap(Key::Enter, Modifiers::NONE);
    }
    let visible = {
        let canvas = ui.canvas();
        canvas.camera.visible(canvas.bounds.size())
    };
    let tiles = ui.app().session.scene.modules();
    assert_eq!(tiles.iter().filter(|t| t.title.starts_with("Noise ")).count(), 2);
    for (i, tile) in tiles.iter().enumerate() {
        let rect = tile.bounds();
        assert!(tiles[..i].iter().all(|other| !other.bounds().intersects(rect)));
        if tile.title.starts_with("Noise ") {
            assert!(visible.x <= rect.x && visible.y <= rect.y);
            assert!(visible.x + visible.width >= rect.x + rect.width);
            assert!(visible.y + visible.height >= rect.y + rect.height);
        }
    }
    let id = *ui.app().session.selection.first().unwrap();
    let placed = ui.app().session.positions[&id];
    ui.send(crate::Message::Undo);
    assert!(ui.app().session.scene.module(id).is_none());
    ui.send(crate::Message::Redo);
    assert_eq!(ui.app().session.positions[&id], placed);
}

/// Shift+F10 and the menu key, typed in the search field, open the
/// highlighted library row's menu through the window's own key path, and
/// the canvas under the browser opens no tile menu for its selected tile.
#[test]
fn the_menu_keys_open_a_library_rows_menu_from_the_field() {
    use loom_ui_core::{Key, Modifiers};
    for (key, modifiers) in [(Key::F10, Modifiers::SHIFT), (Key::ContextMenu, Modifiers::NONE)] {
        let mut ui = crate::ui_test::Ui::open("subtractive.ron");
        let _ = ui
            .app_mut()
            .send(crate::Message::Canvas(crate::CanvasEvent::Select(vec![4])));
        ui.send(crate::Message::OpenBrowser);
        let _ = ui.redraw();
        ui.send(crate::Message::Browser(Message::Library(library_items(2))));
        ui.send(crate::Message::Browser(Message::Query("zz-lib".into())));
        let _ = ui.redraw();
        let _ = ui.tap(key, modifiers);
        let browser = ui.app().overlays.browser().expect("still open");
        assert_eq!(browser.model().menu, Some(0), "{key:?}: the first row's menu");
        assert_eq!(browser.model().menu_highlight, Some(0), "{key:?}: on its item");
        assert!(
            ui.app().overlays.tile_menu().is_none(),
            "{key:?}: no tile menu under it"
        );
    }
}
