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
    // "Phase" starts with "phase"; so do "Phase Scale" and "Phase
    // Trigger", and they sort after it by name.
    assert_eq!(
        ids(&search(&modules(), "phase"))[..3],
        ["loom.phase.generator", "loom.phase.scale", "loom.phase.trigger"]
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
    let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rectangle::default());
    let _ = browser.update(Message::Query("zzzz".into()));
    assert!(browser.results.is_empty());
    assert!(matches!(browser.update(Message::Submit).0, Outcome::Stay));
    assert!(matches!(browser.update(Message::Key(Nav::Down)).0, Outcome::Stay));
}

#[test]
fn enter_adds_the_highlighted_module_at_the_snapped_point() {
    let area = Rectangle::new(Point::ORIGIN, iced::Size::new(1000.0, 800.0));
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
    let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rectangle::default());
    let last = browser.results.len() - 1;
    let _ = browser.update(Message::Key(Nav::Up));
    assert_eq!(browser.highlight, 0);
    let _ = browser.update(Message::Key(Nav::Down));
    assert_eq!(browser.highlight, 1);
    for _ in 0..5 {
        let _ = browser.update(Message::Key(Nav::PageDown));
    }
    assert_eq!(browser.highlight, last);
    let _ = browser.update(Message::Key(Nav::Down));
    assert_eq!(browser.highlight, last);
    let _ = browser.update(Message::Key(Nav::PageUp));
    assert_eq!(browser.highlight, last.saturating_sub(ROWS));
    // A new query highlights the first result again.
    let _ = browser.update(Message::Query("a".into()));
    assert_eq!(browser.highlight, 0);
}

#[test]
fn escape_and_a_click_outside_close_without_adding() {
    let mut browser = Browser::open(modules(), Point::ORIGIN, Point::ORIGIN, Rectangle::default());
    assert!(matches!(browser.update(Message::Key(Nav::Escape)).0, Outcome::Close));
    assert!(matches!(browser.update(Message::Close).0, Outcome::Close));
}

#[test]
fn the_panel_stays_inside_the_canvas() {
    let area = Rectangle::new(Point::new(0.0, 40.0), iced::Size::new(1000.0, 700.0));
    assert_eq!(inside(Point::new(100.0, 100.0), area), Point::new(100.0, 100.0));
    let corner = inside(Point::new(990.0, 730.0), area);
    assert_eq!(corner, Point::new(1000.0 - WIDTH, 740.0 - HEIGHT));
}
