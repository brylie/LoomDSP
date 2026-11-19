//! The value a control shows on a fine range, on the tile and in the
//! inspector, and the inspector's draft against it (control-ranges.md,
//! Showing the value).

use crate::inspector::{Inspector, Message as Inspect};
use crate::ui_test::Ui;
use crate::{App, Message};
use iced::keyboard::{self, Modifiers, key::Named};
use loom_core::{ModuleId, ModuleNames};

/// A sub-patch, Amp, whose Multiply gain of 0.00041 is exposed at -0.01 to
/// 0.01: the only way to get a range that fine until Voice spread is built.
/// Inside, the gain keeps its default range of -1 to 1.
const FINE_AMP: &str = r#"Patch(
    version: 1,
    subpatches: {
        "amp": SubPatch(
            metadata: Metadata(name: "Amp"),
            modules: [PatchModule(id: 1, kind: "loom.math.mul@1", params: {"gain": 0.00041})],
            exposed: [Exposed(name: "gain", module: 1, param: "gain", range: Some((-0.01, 0.01)))],
        ),
    },
    modules: [PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("amp"), position: Some((0.0, 0.0)))],
)"#;

/// The Amp tile's id in [`FINE_AMP`].
const AMP: u32 = 2;

/// [`FINE_AMP`] open, with the inner Multiply's id and its gain's index.
fn fine_amp() -> (App, ModuleId, usize) {
    let app = crate::tests::app_with_text(FINE_AMP);
    let (mul, node) = app
        .session
        .controller
        .graph()
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.math.mul")
        .expect("the Multiply inside Amp");
    let gain = node.parameter_index("gain").unwrap();
    (app, mul, gain)
}

/// The gain's value now.
fn gain_of(app: &App, mul: ModuleId, gain: usize) -> f32 {
    app.session.controller.graph().node(mul).unwrap().params[gain]
}

/// The text the Amp tile's gain knob shows.
fn tile_text(app: &App) -> String {
    let snapshot = app.ui_snapshot();
    let tile = snapshot.tiles.iter().find(|t| t.id == AMP).expect("the Amp tile");
    tile.widgets[0].text.clone()
}

/// The value the inspector's row for the gain shows, with `tile` selected.
fn row_text(app: &mut App, tile: u32, mul: ModuleId, gain: usize) -> String {
    app.session.selection = [tile].into_iter().collect();
    let row = match app.inspector_model() {
        Inspector::SubPatch(rows) => rows.params.into_iter().map(|e| e.row).find(|r| r.index == gain),
        Inspector::Module(rows) if rows.id == mul.0 => rows.params.into_iter().find(|r| r.index == gain),
        other => panic!("{other:?}"),
    };
    let row = row.expect("the gain's row");
    assert_eq!(row.field.as_ref().map(|f| f.text.as_str()), Some(row.value.as_str()));
    row.value
}

/// On the exposed range of -0.01 to 0.01 the tile and the surface row show
/// 0.0004, not "0.000". The inner row, on -1 to 1, keeps today's text.
#[test]
fn a_fine_exposed_range_shows_four_decimals() {
    let (mut app, mul, gain) = fine_amp();
    assert_eq!(tile_text(&app), "0.0004");
    assert_eq!(row_text(&mut app, AMP, mul, gain), "0.0004");
    assert_eq!(row_text(&mut app, mul.0, mul, gain), "0.000");
}

/// The range fields use the visible row's IDs on both faces of a parameter.
#[test]
fn midi_range_fields_follow_the_selected_module_or_surface() {
    use crate::inspector::ids;
    let (mut app, mul, gain) = fine_amp();
    app.session
        .controller
        .map_midi(
            loom_core::MidiControl {
                channel: 0,
                controller: 21,
            },
            mul,
            gain,
        )
        .unwrap();
    app.session.learn.toggle_options((mul.0, gain));

    for tile in [AMP, mul.0] {
        app.session.selection = [tile].into();
        let row = match app.inspector_model() {
            Inspector::SubPatch(rows) => rows.params.into_iter().find(|row| row.module == mul.0).unwrap().row,
            Inspector::Module(rows) => rows.params.into_iter().find(|row| row.index == gain).unwrap(),
            other => panic!("{other:?}"),
        };
        assert_eq!(
            app.midi_range_fields(),
            Some(["midi/low", "midi/high"].map(|part| ids::part(&row.ui, part)))
        );
    }
    app.right_tab = crate::RightTab::Matrix;
    assert_eq!(app.midi_range_fields(), None);
}

/// One 1% arrow-key step on the tile knob changes the text it shows, on the
/// fine range (control-ranges.md, Which step is visible).
#[test]
fn a_one_percent_key_step_on_a_fine_knob_changes_its_text() {
    let (app, mul, gain) = fine_amp();
    let mut ui = Ui::with_app(app);
    ui.redraw();
    let at = ui.control(AMP, "gain");
    ui.move_to(at);
    ui.press();
    ui.release();
    let before = tile_text(ui.app());
    let value = gain_of(ui.app(), mul, gain);
    ui.tap(keyboard::Key::Named(Named::ArrowUp), Modifiers::default());
    let after = tile_text(ui.app());
    assert!(gain_of(ui.app(), mul, gain) > value, "the step moved the gain");
    assert_ne!(after, before, "{before} stays {after}");
    assert_eq!(after, "0.0006");
}

/// Enter on an untouched draft commits nothing and adds no undo step, on
/// the surface row and on the inner row, each against the text it shows.
/// Compared with the descriptor's text, "0.0004" would differ from "0.000"
/// and commit (control-ranges.md, The inspector's draft).
#[test]
fn an_untouched_draft_on_a_fine_range_commits_nothing() {
    let (mut app, mul, gain) = fine_amp();
    for tile in [AMP, mul.0] {
        let shown = row_text(&mut app, tile, mul, gain);
        let steps = app.session.edits.history.len();
        let _ = app.send(Message::Inspector(Inspect::ValueText {
            module: mul.0,
            index: gain,
            text: shown.clone(),
        }));
        let _ = app.send(Message::Inspector(Inspect::ValueSubmit {
            module: mul.0,
            index: gain,
        }));
        assert!(app.session.value_draft.is_none(), "{shown}: the draft goes");
        assert_eq!(gain_of(&app, mul, gain), 0.00041, "{shown}: the value stays");
        assert_eq!(app.session.edits.history.len(), steps, "{shown}: no undo step");
    }
}

/// Typing "0.000" in the fine surface row and pressing Enter sets the gain
/// to 0, as one undo step: the text differs from the "0.0004" shown.
#[test]
fn typing_zero_on_a_fine_range_sets_zero() {
    let (mut app, mul, gain) = fine_amp();
    app.session.selection = [AMP].into_iter().collect();
    let mut ui = Ui::with_app(app);
    ui.redraw();
    let steps = ui.app().session.edits.history.len();
    ui.type_a_value(mul.0, gain, "0.000");
    assert_eq!(gain_of(ui.app(), mul, gain), 0.0);
    assert_eq!(ui.app().session.edits.history.len(), steps + 1);
    assert_eq!(tile_text(ui.app()), "0.0000");
}

/// A screen reader hears the fine text the screen shows: the Amp tile's
/// knob and the surface row's slider both describe the gain as "0.0004"
/// (control-ranges.md, The screen reader hears the shown text).
#[test]
fn the_screen_reader_hears_the_fine_text() {
    let (mut app, mul, gain) = fine_amp();
    app.session.selection = [AMP].into_iter().collect();
    let row = match app.inspector_model() {
        Inspector::SubPatch(rows) => rows.params.into_iter().map(|e| e.row).find(|r| r.index == gain),
        other => panic!("{other:?}"),
    }
    .expect("the gain's surface row");
    let mut ui = Ui::with_app(app);
    ui.redraw();
    let tree = ui.a11y_tree();
    let knob = tree.node_by_id(&crate::a11y::tree::tile_id(AMP)).node("gain");
    assert_eq!(knob.description(), Some("0.0004"));
    assert_eq!(knob.description(), Some(tile_text(ui.app()).as_str()));
    let slider = tree.node_by_id(&crate::inspector::ids::part(&row.ui, "value"));
    assert_eq!(slider.description(), Some("0.0004"));
    assert_eq!(gain_of(ui.app(), mul, gain), 0.00041);
}
