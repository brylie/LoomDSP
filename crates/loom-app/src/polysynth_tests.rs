//! The polysynth example in the app, and the Voice modules a screen reader
//! meets on it (polyphony.md, The shipped example).
//!
//! Hover text is not in the accessibility tree yet (ui-layer.md, section 7,
//! rule 19), so the width check reads the hover text from the scene.

use crate::a11y::tree::tile_id;
use crate::inspector::ids::{Kind, row};
use crate::ui_test::{A11yNode, A11yTree, Ui};
use accesskit::{Action, Role};
use loom_core::ModuleId;

/// Voice Spread's module ID in `polysynth.ron`.
const SPREAD: u32 = 2;

/// The Saw's module ID in `polysynth.ron`, inside the voice chain.
const SAW: u32 = 4;

/// Voice Merge's module ID in `polysynth.ron`.
const MERGE: u32 = 10;

/// The hover text of `module`'s first output.
fn output_hover(ui: &Ui, module: u32) -> String {
    let scene = &ui.app().session.scene;
    scene.module(module).expect("the tile is drawn").outputs[0]
        .hover
        .description
        .clone()
}

/// The value as text the tile shows for `param` on `module`.
fn shown_text(ui: &Ui, module: u32, param: &str) -> String {
    let snapshot = ui.app().ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == module).expect("the tile");
    let node = ui.app().session.controller.graph().node(ModuleId(module)).unwrap();
    let label = node.descriptor.params[node.descriptor.param_index(param).unwrap()].label;
    tile.widgets
        .iter()
        .find(|w| w.label == label)
        .expect("the control is on the tile")
        .text
        .clone()
}

/// The names of the options inside a radio group.
fn options(group: A11yNode<'_>) -> Vec<String> {
    assert_eq!(group.role(), Role::RadioGroup);
    group
        .children()
        .iter()
        .filter(|child| child.role() == Role::RadioButton)
        .filter_map(|child| child.name().map(str::to_string))
        .collect()
}

/// The name of the one checked option inside a radio group.
fn checked(group: A11yNode<'_>) -> String {
    let on: Vec<String> = group
        .children()
        .iter()
        .filter(|child| child.toggled() == Some(accesskit::Toggled::True))
        .filter_map(|child| child.name().map(str::to_string))
        .collect();
    assert_eq!(on.len(), 1, "one option is checked: {on:?}");
    on[0].clone()
}

/// The control for `param` on `module`'s tile.
fn tile_control<'a>(tree: &'a A11yTree, module: u32, label: &str) -> A11yNode<'a> {
    tree.node_by_id(&tile_id(module)).node(label)
}

/// `polysynth.ron` opens in the app. An output in its voice chain says
/// `8 voices` in its hover text, and Voice Merge's output, one voice, does
/// not.
#[test]
fn the_polysynth_shows_its_voice_count() {
    let ui = Ui::open("polysynth.ron");
    let saw = output_hover(&ui, SAW);
    assert!(saw.ends_with(" 8 voices."), "{saw}");
    let merged = output_hover(&ui, MERGE);
    assert!(!merged.contains("voices"), "{merged}");
}

/// A screen reader finds Voice Spread's `amount` on the tile as a slider
/// over the patch's selected range, -0.01 to 0.01, not the descriptor's -10
/// to 10. Its description is the tile's text, "0.0003", and an Increment
/// moves it. The inspector's slider says "0.0003" too. Voice Spread's `mode` is a
/// radio group of Even, Series, and Random, and Voice Merge's `mode` one of
/// its five choices.
#[test]
fn a_screen_reader_finds_the_voice_controls_on_the_polysynth() {
    let mut ui = Ui::open("polysynth.ron");
    let tree = ui.a11y_tree();
    let amount = tile_control(&tree, SPREAD, "Amount");
    assert_eq!(amount.role(), Role::Slider);
    // The range is an `f32` widened to `f64`, as the tree carries it.
    assert_eq!(amount.range(), Some((f64::from(-0.01_f32), f64::from(0.01_f32))));
    assert_eq!(shown_text(&ui, SPREAD, "amount"), "0.0003");
    assert_eq!(amount.description(), Some("0.0003"));
    assert!(amount.offers(Action::Increment));
    assert_eq!(
        options(tile_control(&tree, SPREAD, "Mode")),
        ["Even", "Series", "Random"]
    );
    assert_eq!(
        options(tile_control(&tree, MERGE, "Mode")),
        ["Sum", "Average", "Last", "Lowest", "Highest"]
    );

    ui.a11y_action(&tile_id(SPREAD), Action::Click);
    let tree = ui.a11y_tree();
    let inspector = tree.node_by_id(&row(SPREAD, Kind::Param, "amount")).node("Amount");
    assert_eq!(inspector.role(), Role::Slider);
    assert_eq!(inspector.description(), Some("0.0003"));

    ui.a11y_action(&tile_id(SPREAD).with("param").with("amount"), Action::Increment);
    let node = ui.app().session.controller.graph().node(ModuleId(SPREAD)).unwrap();
    let value = node.params[node.descriptor.param_index("amount").unwrap()];
    assert!(value > 0.0003, "an Increment raises the amount: {value}");
    let shown = shown_text(&ui, SPREAD, "amount");
    assert_ne!(shown, "0.0003", "the tile shows the new value");
    let tree = ui.a11y_tree();
    assert_eq!(
        tile_control(&tree, SPREAD, "Amount").description(),
        Some(shown.as_str())
    );
}

/// Voice Spread's Seed row in the inspector is named "Seed" while Mode is
/// Random, the one mode that reads it, as `polysynth.ron` saves it. Once a
/// screen reader picks Even on the tile, the row says it is not used: its
/// name becomes "Seed, Not used in Even" (ADR-0030, panels.md).
#[test]
fn spreads_seed_row_is_in_use_only_in_random() {
    let mut ui = Ui::open("polysynth.ron");
    ui.a11y_action(&tile_id(SPREAD), Action::Click);
    let seed = row(SPREAD, Kind::Param, "seed");
    assert_eq!(ui.a11y_tree().node_by_id(&seed).name(), Some("Seed"));

    let tree = ui.a11y_tree();
    let even = tile_control(&tree, SPREAD, "Mode").node("Even");
    let even = even.ui_id().expect("the option has a UI ID").clone();
    ui.a11y_action(&even, Action::Click);
    let node = ui.app().session.controller.graph().node(ModuleId(SPREAD)).unwrap();
    assert_eq!(node.params[node.descriptor.param_index("mode").unwrap()], 0.0);
    assert_eq!(ui.a11y_tree().node_by_id(&seed).name(), Some("Seed, Not used in Even"));
}

/// A Voice Split tile's `first` is a slider from 1 to 16 whose description
/// is "1", a whole number with no decimals.
#[test]
fn a_screen_reader_finds_voice_splits_first_voice() {
    let mut ui = Ui::open("polysynth.ron");
    let split = ui
        .app_mut()
        .session
        .controller
        .add_module("loom.voice.split")
        .expect("a built-in module");
    ui.app_mut().commit();
    ui.redraw();
    let tree = ui.a11y_tree();
    let first = tile_control(&tree, split.0, "First");
    assert_eq!(first.role(), Role::Slider);
    assert_eq!(first.range(), Some((1.0, 16.0)));
    assert_eq!(first.description(), Some("1"));
}

/// Adds a Voice assigner to `polysynth.ron` and returns its module ID.
fn add_assigner(ui: &mut Ui) -> u32 {
    let id = ui
        .app_mut()
        .session
        .controller
        .add_module("loom.voice.assign")
        .expect("a built-in module");
    ui.app_mut().commit();
    ui.redraw();
    id.0
}

/// Sets the assigner's stored `voices` and `mode` directly, as a loaded patch
/// would, without the count edit's mode switch.
fn store(ui: &mut Ui, module: u32, voices: f32, mode: f32) {
    let controller = &mut ui.app_mut().session.controller;
    controller.set_param_named(ModuleId(module), "voices", voices).unwrap();
    controller.set_param_named(ModuleId(module), "mode", mode).unwrap();
    ui.app_mut().commit();
    ui.redraw();
}

/// The names of every node under `node`, at any depth.
fn names_under(node: A11yNode<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for child in node.children() {
        names.extend(child.name().map(str::to_string));
        names.extend(names_under(child));
    }
    names
}

/// Whether `module`'s tile, in the tree and on the canvas, shows a Unison
/// control. Tab moves only over the shown widgets of the scene, so a
/// control hidden there is one Tab skips.
fn unison_on_tile(ui: &mut Ui, module: u32) -> bool {
    let tree = ui.a11y_tree();
    let in_tree = names_under(tree.node_by_id(&tile_id(module)))
        .iter()
        .any(|n| n == "Unison");
    let snapshot = ui.app().ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == module).expect("the tile");
    let in_scene = tile.widgets.iter().any(|w| w.label == "Unison" && !w.hidden);
    assert_eq!(in_tree, in_scene, "the tree and the canvas agree");
    in_tree
}

/// A screen reader finds Voice assigner's controls by name. At its default
/// of one voice, Mode is a radio group of Legato and Retrigger, Voices a
/// slider from 1 to 16, and the Gate light is on the tile. Unison is hidden,
/// so the tile has no Unison control, while the inspector row is still read,
/// named "Unison, Not used in Legato" (panels.md, rule 5). After Voices is set
/// to 8, Mode offers Oldest and Quietest, and Unison is a slider from 1 to 4.
#[test]
fn a_screen_reader_finds_voice_assigners_controls() {
    let mut ui = Ui::open("polysynth.ron");
    let assign = add_assigner(&mut ui);
    let tree = ui.a11y_tree();
    assert_eq!(options(tile_control(&tree, assign, "Mode")), ["Legato", "Retrigger"]);
    assert_eq!(checked(tile_control(&tree, assign, "Mode")), "Legato");
    let voices = tile_control(&tree, assign, "Voices");
    assert_eq!(voices.role(), Role::Slider);
    assert_eq!(voices.range(), Some((1.0, 16.0)));
    tile_control(&tree, assign, "Gate");
    assert!(!unison_on_tile(&mut ui, assign));

    ui.a11y_action(&tile_id(assign), Action::Click);
    let unison = row(assign, Kind::Param, "unison");
    assert_eq!(
        ui.a11y_tree().node_by_id(&unison).name(),
        Some("Unison, Not used in Legato")
    );

    ui.a11y_set_value(&tile_id(assign).with("param").with("voices"), 8.0);
    let tree = ui.a11y_tree();
    assert_eq!(options(tile_control(&tree, assign, "Mode")), ["Oldest", "Quietest"]);
    assert_eq!(checked(tile_control(&tree, assign, "Mode")), "Oldest");
    let control = tile_control(&tree, assign, "Unison");
    assert_eq!(control.role(), Role::Slider);
    assert_eq!(control.range(), Some((1.0, 4.0)));
    assert_eq!(ui.a11y_tree().node_by_id(&unison).name(), Some("Unison"));
}

/// Unison hides by the mode the assigner plays, which the Mode control
/// shows, not by the stored value: at 8 voices a stored Legato plays Oldest,
/// so Unison shows; at 1 voice a stored Oldest plays Legato, so it hides,
/// and its inspector row names Legato.
#[test]
fn voice_assigners_unison_follows_the_mode_that_plays() {
    let mut ui = Ui::open("polysynth.ron");
    let assign = add_assigner(&mut ui);
    store(&mut ui, assign, 8.0, 2.0);
    assert!(unison_on_tile(&mut ui, assign));
    // A stored Legato plays Oldest, and Mode says so.
    let tree = ui.a11y_tree();
    assert_eq!(checked(tile_control(&tree, assign, "Mode")), "Oldest");
    store(&mut ui, assign, 8.0, 0.0);
    assert!(unison_on_tile(&mut ui, assign));
    store(&mut ui, assign, 1.0, 0.0);
    assert!(!unison_on_tile(&mut ui, assign));
    ui.a11y_action(&tile_id(assign), Action::Click);
    assert_eq!(
        ui.a11y_tree().node_by_id(&row(assign, Kind::Param, "unison")).name(),
        Some("Unison, Not used in Legato")
    );
}

/// The Saw of the first layer in `polysynth-large.ron`, inside the voice chain.
const LARGE_SAW: u32 = 6;

/// Voice Merge's module ID in `polysynth-large.ron`.
const LARGE_MERGE: u32 = 69;

/// `polysynth-large.ron` opens in the app with all 96 modules. An
/// output in its voice chain says `8 voices` in its hover text, and Voice
/// Merge's output, one voice, does not (polyphony.md, The large reference
/// patch).
#[test]
fn the_large_reference_patch_opens_and_shows_its_voice_count() {
    let ui = Ui::open("polysynth-large.ron");
    let graph = ui.app().session.controller.graph();
    assert_eq!(graph.nodes().count(), 96);
    let saw = output_hover(&ui, LARGE_SAW);
    assert!(saw.ends_with(" 8 voices."), "{saw}");
    let merged = output_hover(&ui, LARGE_MERGE);
    assert!(!merged.contains("voices"), "{merged}");
}

/// MIDI Note's Mode reads the choice it plays as checked, through the same
/// tile code as Voice assigner: Oldest at the polysynth's 8 voices, and
/// Legato on a new MIDI Note at its default of one voice.
#[test]
fn midi_notes_mode_reads_its_checked_choice() {
    let mut ui = Ui::open("polysynth.ron");
    let tree = ui.a11y_tree();
    assert_eq!(checked(tile_control(&tree, 1, "Mode")), "Oldest");
    let note = ui
        .app_mut()
        .session
        .controller
        .add_module("loom.midi.note")
        .expect("a built-in module");
    ui.app_mut().commit();
    ui.redraw();
    let tree = ui.a11y_tree();
    assert_eq!(checked(tile_control(&tree, note.0, "Mode")), "Legato");
}
