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
    // "trig" starts a word of "Phase Trigger". Nothing else has it.
    assert_eq!(search(&modules(), "trig")[0].id, "loom.phase.trigger");
    // "meter" starts a word of "Level Meter" and is in its ID.
    assert_eq!(search(&modules(), "meter")[0].id, "loom.display.meter");
}

#[test]
fn every_word_must_match_name_id_or_category() {
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
            assert_eq!(at, Point::new(96.0, 24.0));
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

/// The list scrolls by the highlighted row's UI ID, just enough: not at
/// all while the row is in view, down until its bottom shows, back up to
/// its top, and to the end for the last row. Each time, the browser's own
/// offset, which places a row's menu, is the one the list reports: the
/// list does not tell the browser after the app scrolls it.
#[test]
fn the_list_scrolls_just_enough_and_the_browser_knows_where() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.send(crate::Message::OpenBrowser);
    let _ = ui.redraw();
    let press = |ui: &mut crate::ui_test::Ui, key: Named, times: usize| {
        for _ in 0..times {
            let _ = ui.tap(Key::Named(key), Modifiers::empty());
        }
        let _ = ui.redraw();
        let list = ui.scroll_translation(LIST).expect("the list scrolls").y;
        let browser = ui.app().overlays.browser().expect("the browser is open");
        assert!((browser.scroll - list).abs() < 0.5, "{} and {list}", browser.scroll);
        (browser.highlight(), list)
    };
    assert_eq!(press(&mut ui, Named::ArrowDown, 1), (1, 0.0), "in view: no scroll");
    let (row, below) = press(&mut ui, Named::ArrowDown, ROWS - 1);
    assert_eq!(row, ROWS, "the first row past the view");
    assert!((below - ROW_HEIGHT).abs() < 0.5, "its bottom shows: {below}");
    assert_eq!(press(&mut ui, Named::ArrowUp, ROWS), (0, 0.0), "above: back to its top");
    let rows = ui.app().overlays.browser().unwrap().model().rows.len();
    let (last, end) = press(&mut ui, Named::PageDown, rows);
    assert_eq!(last, rows - 1, "the last row");
    let most = rows as f32 * ROW_HEIGHT - LIST_HEIGHT;
    assert!((end - most).abs() < 0.5, "the end of the list: {end}, not {most}");
}

/// After a scroll down, a search that makes the list short, and the search
/// cleared again, the first row shows. The toolkit keeps its own offset
/// while the short list shows it clamped, so the browser always asks the
/// window to show the row, even when its own offset says it shows.
#[test]
fn the_first_row_shows_after_a_short_list_grows_again() {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.send(crate::Message::OpenBrowser);
    let _ = ui.redraw();
    for _ in 0..ROWS + 4 {
        let _ = ui.tap(Key::Named(Named::ArrowDown), Modifiers::empty());
    }
    let _ = ui.redraw();
    assert!(ui.scroll_translation(LIST).unwrap().y > 0.0, "the list scrolled down");
    let query = |ui: &mut crate::ui_test::Ui, text: &str| {
        ui.send(crate::Message::Browser(Message::Query(text.into())));
        let _ = ui.redraw();
    };
    query(&mut ui, "saw");
    let rows = ui.app().overlays.browser().unwrap().model().rows.len();
    assert!(rows < ROWS, "a list short enough to fit: {rows} rows");
    query(&mut ui, "");
    let browser = ui.app().overlays.browser().unwrap();
    assert_eq!(browser.highlight(), 0);
    let first = browser.model().rows[0].id.clone();
    let place = ui.places().of(&first).expect("the window drew the first row");
    let list = place.clip.expect("it is in the list");
    assert!(
        place.rect.y >= list.y - 0.5,
        "the first row shows: {:?} in {list:?}",
        place.rect
    );
    let shown = ui.scroll_translation(LIST).unwrap().y;
    let ours = ui.app().overlays.browser().unwrap().scroll;
    assert!(
        (shown - ours).abs() < 0.5,
        "the browser's offset {ours} is the list's {shown}"
    );
}

/// The clamp's arithmetic: an offset past a short list goes to 0. Iced
/// never reports such an offset (its viewport is clamped already); the
/// path users meet is a smaller library list, in
/// `a_smaller_library_list_clamps_the_offset`.
#[test]
fn an_offset_past_a_short_list_is_clamped_to_it() {
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.send(crate::Message::OpenBrowser);
    let _ = ui.redraw();
    ui.send(crate::Message::Browser(Message::Query("saw".into())));
    let rows = ui.app().overlays.browser().unwrap().model().rows.len();
    assert!(rows < ROWS, "a list short enough to fit: {rows} rows");
    ui.send(crate::Message::Browser(Message::Scrolled(500.0)));
    assert_eq!(ui.app().overlays.browser().unwrap().scroll, 0.0);
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

/// A library list read again with fewer items is the one message that
/// changes the rows without a reveal. The offset is clamped to the new
/// list: to its last full view while it is still longer than the view,
/// and to 0 once it fits.
#[test]
fn a_smaller_library_list_clamps_the_offset() {
    let three: Vec<_> = modules().into_iter().take(3).collect();
    let area = loom_ui_core::Rect::new(
        loom_ui_core::Point::new(0.0, 0.0),
        loom_ui_core::Size::new(2000.0, 2000.0),
    );
    let mut browser = Browser::open(
        three,
        loom_ui_core::Point::new(0.0, 0.0),
        loom_ui_core::Point::new(10.0, 10.0),
        area,
    );
    let _ = browser.update(Message::Library(library_items(12)));
    assert_eq!(browser.model().rows.len(), 15);
    let _ = browser.update(Message::Key(Nav::PageDown));
    let _ = browser.update(Message::Key(Nav::PageDown));
    assert_eq!(browser.highlight(), 14, "on the last row");
    assert_eq!(browser.scroll, 5.0 * ROW_HEIGHT);
    let _ = browser.update(Message::Library(library_items(8)));
    assert_eq!(browser.model().rows.len(), 11);
    assert_eq!(browser.scroll, ROW_HEIGHT, "the last full view of 11 rows");
    let _ = browser.update(Message::Library(Vec::new()));
    assert_eq!(browser.scroll, 0.0, "3 rows fit");
}

/// Shrinks the library list until it fits, then grows it back with a
/// `Library` message, which does not reveal, and compares the browser's
/// offset with the one the list shows after a redraw.
fn offsets_after_regrowing(back_to: usize) -> (f32, f32) {
    use iced::keyboard::{Key, Modifiers, key::Named};
    let mut ui = crate::ui_test::Ui::open("subtractive.ron");
    ui.send(crate::Message::OpenBrowser);
    let _ = ui.redraw();
    let library = |ui: &mut crate::ui_test::Ui, count: usize| {
        ui.send(crate::Message::Browser(Message::Library(library_items(count))));
        let _ = ui.redraw();
    };
    library(&mut ui, 12);
    ui.send(crate::Message::Browser(Message::Query("zz-lib".into())));
    let _ = ui.redraw();
    assert_eq!(ui.app().overlays.browser().unwrap().model().rows.len(), 12);
    for _ in 0..11 {
        let _ = ui.tap(Key::Named(Named::ArrowDown), Modifiers::empty());
    }
    let _ = ui.redraw();
    assert_eq!(ui.scroll_translation(LIST).unwrap().y, 2.0 * ROW_HEIGHT);
    library(&mut ui, 3);
    library(&mut ui, back_to);
    let _ = ui.redraw();
    let shown = ui.scroll_translation(LIST).unwrap().y;
    (shown, ui.app().overlays.browser().unwrap().scroll)
}

/// Grown back to a length the list has not reported before, the list
/// reports its offset on the next redraw, and the browser takes it.
#[test]
fn a_regrown_library_list_reports_its_offset() {
    let (shown, ours) = offsets_after_regrowing(13);
    assert_eq!(shown, ours, "the list shows {shown}, the browser holds {ours}");
}

/// Grown back to the very length it last reported, with the same stored
/// offset, the list has nothing new to report, so the browser keeps the
/// clamped offset while the list shows the old one: the known gap in the
/// spec's decision on the browser's offset.
#[test]
fn a_library_list_regrown_to_its_old_length_keeps_the_gap() {
    let (shown, ours) = offsets_after_regrowing(12);
    assert_eq!((shown, ours), (2.0 * ROW_HEIGHT, 0.0));
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
    assert_eq!(filter.id, browser_id().with("role").with("loom.role.filter"));
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
    let _ = browser.update(Message::Key(Nav::Down));
    let _ = browser.update(Message::Key(Nav::Down));
    let _ = browser.update(Message::Key(Nav::Down));
    let _ = browser.update(Message::Key(Nav::Down));
    let before = browser.model().rows[browser.model().highlight.unwrap()].id.clone();
    assert_eq!(before, browser_id().with("role").with("loom.role.source"));
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
