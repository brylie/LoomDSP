//! Tests for the harness view of the accessibility tree: reading nodes by
//! name, scoping a name that repeats, and acting on a node as a screen
//! reader does.

use super::*;
use loom_core::ModuleId;

/// The value of a parameter in the patch, by name.
fn param(ui: &Ui, module: u32, name: &str) -> f32 {
    let node = ui.app().session.controller.graph().node(ModuleId(module)).unwrap();
    node.params[node.descriptor.param_index(name).unwrap()]
}

/// A knob reads by its name, inside the tile named above it, with its role
/// and number; the same knob is found by its UI ID.
#[test]
fn a_knob_reads_by_name_role_and_number() {
    let mut ui = Ui::open("subtractive.ron");
    let tree = ui.a11y_tree();
    let cutoff = tree.path(&["SVF", "Cutoff"]);
    assert_eq!(cutoff.role(), Role::Slider);
    assert_eq!(cutoff.number(), Some(300.0));
    assert!(cutoff.offers(Action::Increment));
    assert_eq!(tree.node("Cutoff").ui_id(), cutoff.ui_id(), "one Cutoff in the patch");
    let id = cutoff.ui_id().expect("every node has a UI ID").clone();
    assert_eq!(tree.node_by_id(&id).name(), Some("Cutoff"));

    // Two Envelope tiles: one found by its UI ID scopes the name inside it.
    let second = tree.node_by_id(&crate::a11y::tree::tile_id(7));
    assert_eq!(second.name(), Some("Envelope"));
    let attack = second.node("Attack").number().expect("a knob has a number");
    assert!((attack - 0.005).abs() < 1e-6, "the second envelope's Attack: {attack}");
    assert!(tree.names().iter().filter(|name| *name == "Attack").count() == 2);
}

/// A name that repeats panics, saying how many matched and where each is,
/// so the test can scope it.
#[test]
#[should_panic(expected = r#"2 shown nodes are named "Attack" inside "subtractive.ron - LoomDSP""#)]
fn a_name_that_repeats_panics_and_lists_the_matches() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.a11y_tree().node("Attack");
}

/// A name no shown node has panics, listing the names there are.
#[test]
#[should_panic(expected = r#"no shown node named "Wobble""#)]
fn a_missing_name_panics() {
    let mut ui = Ui::open("subtractive.ron");
    let _ = ui.a11y_tree().path(&["SVF", "Wobble"]);
}

/// Increment on a knob moves its parameter, and the tree reads the new
/// value, as a screen reader hears it after its request.
#[test]
fn increment_moves_a_knob_and_the_tree_follows() {
    let mut ui = Ui::open("subtractive.ron");
    let before = param(&ui, 4, "cutoff");
    ui.a11y_action(["SVF", "Cutoff"], Action::Increment);
    let after = param(&ui, 4, "cutoff");
    assert!(after > before, "{before} -> {after}");
    assert_eq!(ui.a11y_tree().path(&["SVF", "Cutoff"]).number(), Some(f64::from(after)));
}

/// SetValue with a number sets a knob on its tile, and with text sets the
/// inspector's value field, once a Click on the tile has selected it.
#[test]
fn set_value_sets_a_knob_and_a_field() {
    let mut ui = Ui::open("subtractive.ron");
    ui.a11y_set_value(["SVF", "Cutoff"], 1000.0);
    assert_eq!(param(&ui, 4, "cutoff"), 1000.0);
    ui.a11y_action("SVF", Action::Click);
    ui.a11y_set_value("Cutoff value", "2000");
    assert_eq!(param(&ui, 4, "cutoff"), 2000.0);
}

/// Click on a toolbar button opens the module browser, which owns the
/// input: the tiles below it are hidden, so their names are not found.
#[test]
fn click_opens_the_browser_and_hides_the_tiles() {
    let mut ui = Ui::open("subtractive.ron");
    assert!(ui.a11y_tree().names().iter().any(|name| name == "Cutoff"));
    ui.a11y_action("Add module", Action::Click);
    let names = ui.a11y_tree().names();
    assert!(!names.iter().any(|name| name == "Cutoff"), "{names:?}");
    assert!(ui.app().ui_snapshot().browser.is_some(), "the browser is open");
}
