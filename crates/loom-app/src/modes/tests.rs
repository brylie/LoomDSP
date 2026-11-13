//! The mode gestures in the app (sub-patches.md, The editor for modes;
//! ADR-0040).

use super::*;
use crate::inspector::Inspector;
use crate::{Loaded, Message as AppMessage, Source};
use loom_core::{EngineConfig, ModuleNames};
use loom_patch::flatten::COPY_STEP;
use loom_patch::{ModulePath, modes};

/// A note into a Voice sub-patch (a phase into a sine into a filter, the
/// filter's cutoff exposed), out to the audio output.
const VOICE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Modes"),
    subpatches: {
        "voice": SubPatch(
            metadata: Metadata(name: "Voice"),
            modules: [
                PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("pitch"), position: Some((0.0, 0.0))),
                PatchModule(id: 2, kind: "loom.phase.generator@1", position: Some((200.0, 0.0))),
                PatchModule(id: 3, kind: "loom.osc.sine@1", position: Some((400.0, 0.0))),
                PatchModule(id: 4, kind: "loom.struct.output@1", label: Some("out"), position: Some((800.0, 0.0))),
                PatchModule(id: 5, kind: "loom.filter.svf@1", params: {"cutoff": 1000.0}, position: Some((600.0, 0.0))),
                PatchModule(id: 6, kind: "loom.math.mul@1", position: Some((600.0, 200.0))),
            ],
            cables: [
                PatchCable(from: (1, "out"), to: (2, "pitch")),
                PatchCable(from: (2, "phase"), to: (3, "phase")),
                PatchCable(from: (3, "out"), to: (5, "in")),
                PatchCable(from: (5, "lowpass"), to: (4, "in")),
            ],
            exposed: [Exposed(name: "Cutoff", module: 5, param: "cutoff")],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.midi.note@1", position: Some((0.0, 0.0))),
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("voice"), position: Some((200.0, 0.0))),
        PatchModule(id: 3, kind: "loom.io.audio-out@1", position: Some((400.0, 0.0))),
        PatchModule(id: 4, kind: "loom.clock.transport@1", position: Some((0.0, 200.0))),
    ],
    cables: [
        PatchCable(from: (1, "pitch"), to: (2, "pitch")),
        PatchCable(from: (2, "out"), to: (3, "in")),
    ],
)"#;

/// The app with `text` open, no audio.
fn app_with(text: &str) -> App {
    let patch = loom_patch::Patch::from_ron(text).unwrap();
    let loaded = Loaded::read(
        EngineConfig::default(),
        false,
        Source::Recovered(Box::new(patch)),
        &loom_patch::NoLibrary,
    )
    .unwrap();
    App::assemble(
        Err("test".to_string()),
        48_000.0,
        loaded,
        1,
        &crate::tests::app_args(),
        crate::settings::Rig::empty(),
        false,
    )
}

/// Sends a mode gesture.
fn mode(app: &mut App, message: Message) {
    let _ = app.send(AppMessage::Mode(message));
}

/// The flat id at `path`, if any.
fn at(app: &App, path: &[u32]) -> Option<ModuleId> {
    app.session.hierarchy.ids().get(&ModulePath(path.to_vec()))
}

/// The inspector rows of the Voice tile.
fn voice_rows(app: &mut App) -> crate::inspector::SubPatchRows {
    app.session.selection = [2].into();
    match app.ui_snapshot().inspector {
        Inspector::SubPatch(rows) => rows,
        other => panic!("not a sub-patch inspector: {other:?}"),
    }
}

/// How many flat modules of `id` (such as `loom.filter.svf`) the graph holds.
fn count(app: &App, id: &str) -> usize {
    app.session
        .controller
        .graph()
        .nodes()
        .filter(|(_, n)| n.descriptor.id == id)
        .count()
}

/// Mode and Copies from the inspector: Stack, then 4 copies, each one undo
/// step; a turn of the inner cutoff reaches every copy at once; undo takes
/// the copies away and keeps copy 0.
#[test]
fn stack_from_the_inspector_with_copies_that_follow_a_turn() {
    let mut app = app_with(VOICE);
    assert_eq!(voice_rows(&mut app).mode, Mode::Single);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Stack,
        },
    );
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("make stack"));
    assert_eq!(count(&app, "loom.filter.svf"), 1, "one copy changes nothing");
    mode(&mut app, Message::SetCopies { tile: 2, copies: 4 });
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("set Copies"));
    assert_eq!(count(&app, "loom.filter.svf"), 4);
    let rows = voice_rows(&mut app);
    assert_eq!((rows.mode, rows.copies), (Mode::Stack, 4));
    assert_eq!(rows.spread.len(), 1);
    // The breadcrumb names the count.
    app.show_level(&ModulePath::top(2));
    assert_eq!(app.ui_snapshot().level.last().map(String::as_str), Some("Voice ×4"));
    let svf = at(&app, &[2, 5]).unwrap();
    let copy = at(&app, &[2, COPY_STEP + 3, 5]).unwrap();
    let cutoff = app
        .session
        .controller
        .graph()
        .node(svf)
        .unwrap()
        .parameter_index("cutoff")
        .unwrap();
    app.set_param(svf.0, cutoff, 3000.0);
    assert_eq!(
        app.session.controller.graph().node(copy).unwrap().params[cutoff],
        3000.0
    );
    // The copies are not tiles.
    assert!(!app.ui_snapshot().tiles.iter().any(|t| t.id == copy.0));
    app.show_level(&ModulePath::root());
    app.undo();
    app.undo();
    assert_eq!(count(&app, "loom.filter.svf"), 1);
    assert_eq!(at(&app, &[2, 5]), Some(svf), "copy 0 keeps its id");
}

/// A cable made inside a stack reaches every copy, and its undo is one
/// step that takes it from every copy.
#[test]
fn an_edit_inside_reaches_every_copy_as_one_step() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Stack,
        },
    );
    mode(&mut app, Message::SetCopies { tile: 2, copies: 3 });
    app.show_level(&ModulePath::top(2));
    let (sine, mul) = (at(&app, &[2, 3]).unwrap(), at(&app, &[2, 6]).unwrap());
    let cables_into_muls = |app: &App| {
        app.session
            .controller
            .graph()
            .cables()
            .filter(|(_, to)| app.session.controller.graph().node(to.module).unwrap().descriptor.id == "loom.math.mul")
            .count()
    };
    let _ = app.send(AppMessage::Canvas(loom_canvas::CanvasEvent::Connect {
        from: loom_canvas::PortRef {
            module: sine.0,
            index: 0,
            output: true,
        },
        to: loom_canvas::PortRef {
            module: mul.0,
            index: 0,
            output: false,
        },
    }));
    assert_eq!(cables_into_muls(&app), 3);
    app.undo();
    assert_eq!(cables_into_muls(&app), 0);
}

/// Select from the inspector: the Member control is first on the surface
/// and in the inspector, `Member: 1 (Voice)`; Add member adds "Voice 2";
/// single with two members is refused; the last member cannot go; inside,
/// any other module is refused.
#[test]
fn select_members_and_the_member_control() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Select,
        },
    );
    let rows = voice_rows(&mut app);
    assert_eq!(rows.mode, Mode::Select);
    assert_eq!(rows.members.len(), 1);
    assert!(rows.params[0].member, "the Member control comes first");
    assert_eq!(rows.params[0].label, "Member: 1 (Voice)");
    mode(&mut app, Message::AddMember(2));
    assert_eq!(app.ui_snapshot().undo.as_deref(), Some("add member"));
    let rows = voice_rows(&mut app);
    let names: Vec<&str> = rows.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["Voice", "Voice 2"]);
    assert_eq!(count(&app, "loom.filter.svf"), 2);
    // Member 2 from the surface control.
    let member = app.session.hierarchy.member_module(&ModulePath::top(2)).unwrap();
    let target = app.control_target(2, 0, loom_canvas::ControlPart::Main).unwrap();
    assert_eq!(target.module, member);
    assert_eq!((target.spec.min, target.spec.max), (1.0, 2.0));
    app.set_param(member.0, target.index, 2.0);
    assert_eq!(voice_rows(&mut app).params[0].label, "Member: 2 (Voice 2)");
    // Refusals.
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Single,
        },
    );
    assert_eq!(app.alerts.status(), modes::several_members("Voice", Mode::Single));
    mode(&mut app, Message::RemoveMember(rows.members[1].id));
    assert_eq!(voice_rows(&mut app).members.len(), 1);
    mode(&mut app, Message::RemoveMember(rows.members[0].id));
    assert_eq!(app.alerts.status(), modes::last_member(Mode::Select));
    app.show_level(&ModulePath::top(2));
    let refused = app.refuses_kind("loom.osc.saw");
    assert_eq!(refused, Some(modes::only_members(Mode::Select)));
}

/// A stack's dissolve and Save to library are refused with their texts,
/// and a slot is refused inside a stack of more than one copy.
#[test]
fn a_stack_refuses_dissolve_library_and_slots() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Stack,
        },
    );
    mode(&mut app, Message::SetCopies { tile: 2, copies: 2 });
    app.session.selection = [2].into();
    app.dissolve_subpatch();
    assert_eq!(app.alerts.status(), modes::dissolve_single_only("Voice"));
    assert!(!app.save_to_library(ModuleId(2)));
    assert_eq!(app.alerts.status(), modes::no_library(Mode::Stack));
    let svf = at(&app, &[2, 5]).unwrap();
    app.make_slot(svf.0, "loom.role.filter");
    assert_eq!(app.alerts.status(), modes::NO_SLOT_IN_STACK);
}

/// Stereo spread and a spread slider: the toggle is one step; one slider
/// drag is one step and reaches the file as a share of the knob.
#[test]
fn spread_and_stereo_spread_from_the_inspector() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Stack,
        },
    );
    mode(&mut app, Message::SetCopies { tile: 2, copies: 3 });
    mode(&mut app, Message::SetStereoSpread { tile: 2, on: true });
    assert!(voice_rows(&mut app).stereo_spread);
    for amount in [0.1, 0.2, 0.3] {
        mode(
            &mut app,
            Message::SetSpread {
                tile: 2,
                name: "Cutoff".to_string(),
                amount,
            },
        );
    }
    let _ = app.send(AppMessage::SliderReleased);
    assert_eq!(voice_rows(&mut app).spread[0].amount, 0.3);
    app.undo();
    assert_eq!(voice_rows(&mut app).spread[0].amount, 0.0, "one drag, one step");
    app.undo();
    assert!(!voice_rows(&mut app).stereo_spread);
}

/// The ids of the sub-patch tiles at the top level.
fn top_instances(app: &App) -> Vec<u32> {
    app.session
        .hierarchy
        .structural_tiles(&ModulePath::root())
        .into_iter()
        .filter(|(_, tile)| matches!(tile, loom_patch::Tile::Instance(_)))
        .map(|(id, _)| id.0)
        .collect()
}

/// A stack tile copies whole, its Copies and spread with it, and a select
/// tile keeps its members and its Member value (sub-patches.md, How modes
/// meet the rest: Clipboard).
#[test]
fn a_tile_with_a_mode_copies_whole() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Stack,
        },
    );
    mode(&mut app, Message::SetCopies { tile: 2, copies: 3 });
    app.session.selection = [2].into();
    let text = app.copy().expect("the tile copies");
    app.paste(loom_ui_core::Point::new(600.0, 400.0), Some(text));
    let copy = *top_instances(&app).iter().find(|id| **id != 2).expect("a pasted tile");
    let modes = app.session.hierarchy.modes_of(ModuleId(copy)).unwrap();
    assert_eq!((modes.mode, modes.copy_count()), (Mode::Stack, 3));
    assert_eq!(count(&app, "loom.filter.svf"), 6);

    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Select,
        },
    );
    mode(&mut app, Message::AddMember(2));
    let member = app.session.hierarchy.member_module(&ModulePath::top(2)).unwrap();
    app.set_param(member.0, 0, 2.0);
    app.session.selection = [2].into();
    let text = app.copy().expect("the tile copies");
    app.paste(loom_ui_core::Point::new(600.0, 400.0), Some(text));
    let copy = *top_instances(&app).iter().find(|id| **id != 2).expect("a pasted tile");
    assert_eq!(app.session.hierarchy.members_of(ModuleId(copy)).len(), 2);
    let path = app.session.hierarchy.path_of(ModuleId(copy));
    assert_eq!(
        app.session
            .hierarchy
            .member_value(app.session.controller.graph(), &path),
        Some(2.0)
    );
}

/// Inside a layer, a paste of a module and a cable between members are
/// refused with the layer text; a paste of a sub-patch tile adds a member.
#[test]
fn a_layer_level_refuses_pastes_of_modules_and_cables() {
    let mut app = app_with(VOICE);
    app.session.selection = [1].into();
    let note = app.copy().expect("the note module copies");
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Layer,
        },
    );
    mode(&mut app, Message::AddMember(2));
    let members = app.session.hierarchy.members_of(ModuleId(2));
    app.show_level(&ModulePath::top(2));
    app.session.selection = [members[0].0].into();
    let member_text = app.copy().expect("a member tile copies");
    app.paste(loom_ui_core::Point::new(0.0, 0.0), Some(note));
    assert_eq!(app.alerts.status(), modes::only_members(Mode::Layer));
    let _ = app.send(AppMessage::Canvas(loom_canvas::CanvasEvent::Connect {
        from: loom_canvas::PortRef {
            module: members[0].0,
            index: 0,
            output: true,
        },
        to: loom_canvas::PortRef {
            module: members[1].0,
            index: 0,
            output: false,
        },
    }));
    assert_eq!(app.alerts.status(), modes::only_members(Mode::Layer));
    app.paste(loom_ui_core::Point::new(0.0, 400.0), Some(member_text));
    assert_eq!(app.session.hierarchy.members_of(ModuleId(2)).len(), 3);
}

/// The mode rows draw and their controls work through the view: Add member
/// from a click, then Stereo spread on a stack from a click.
#[test]
fn the_mode_rows_draw_and_answer_clicks() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Select,
        },
    );
    app.session.selection = [2].into();
    let mut ui = crate::ui_test::Ui::with_app(app);
    ui.click_text("Add member");
    assert_eq!(ui.app().session.hierarchy.members_of(ModuleId(2)).len(), 2);
    assert!(ui.text("Members").is_some() && ui.text("Remove").is_some());
    ui.send(AppMessage::Mode(Message::RemoveMember(
        ui.app().session.hierarchy.members_of(ModuleId(2))[1].0,
    )));
    ui.send(AppMessage::Mode(Message::SetMode {
        tile: 2,
        mode: Mode::Stack,
    }));
    ui.send(AppMessage::Mode(Message::SetCopies { tile: 2, copies: 2 }));
    ui.redraw();
    assert!(ui.text("Copies").is_some() && ui.text("Spread").is_some());
    ui.click_text("Stereo spread");
    assert!(ui.app().session.hierarchy.modes_of(ModuleId(2)).unwrap().stereo_spread);
}

/// Member follows the member that plays: moving it up keeps it playing,
/// and removing an earlier member keeps it too (sub-patches.md, The editor
/// for modes).
#[test]
fn member_follows_its_member_when_members_move() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Select,
        },
    );
    mode(&mut app, Message::AddMember(2));
    mode(&mut app, Message::AddMember(2));
    let member = app.session.hierarchy.member_module(&ModulePath::top(2)).unwrap();
    app.set_param(member.0, 0, 2.0);
    assert_eq!(voice_rows(&mut app).params[0].label, "Member: 2 (Voice 2)");
    let second = voice_rows(&mut app).members[1].id;
    mode(
        &mut app,
        Message::MoveMember {
            member: second,
            up: true,
        },
    );
    assert_eq!(voice_rows(&mut app).params[0].label, "Member: 1 (Voice 2)");
    // Undo puts both the order and Member back, as one step.
    app.undo();
    assert_eq!(voice_rows(&mut app).params[0].label, "Member: 2 (Voice 2)");
    let first = voice_rows(&mut app).members[0].id;
    mode(&mut app, Message::RemoveMember(first));
    assert_eq!(voice_rows(&mut app).params[0].label, "Member: 1 (Voice 2)");
}

/// Deleting the last member with the Delete key, or every member at once,
/// says the named refusal and keeps them.
#[test]
fn deleting_the_last_member_by_key_is_refused_with_its_text() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Layer,
        },
    );
    mode(&mut app, Message::AddMember(2));
    let members = app.session.hierarchy.members_of(ModuleId(2));
    app.show_level(&ModulePath::top(2));
    assert!(!app.delete_tiles(&[members[0].0, members[1].0]));
    assert_eq!(app.alerts.status(), modes::last_member(Mode::Layer));
    assert!(app.delete_tiles(&[members[0].0]));
    assert!(!app.delete_tiles(&[members[1].0]));
    assert_eq!(app.alerts.status(), modes::last_member(Mode::Layer));
    assert_eq!(app.session.hierarchy.members_of(ModuleId(2)), vec![members[1]]);
}

/// A member cannot be dissolved inside its layer: its modules would sit at
/// a level that holds only members.
#[test]
fn a_member_cannot_be_dissolved_inside_its_layer() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Layer,
        },
    );
    let member = app.session.hierarchy.members_of(ModuleId(2))[0];
    app.show_level(&ModulePath::top(2));
    app.session.selection = [member.0].into();
    app.dissolve_subpatch();
    assert_eq!(app.alerts.status(), modes::only_members(Mode::Layer));
    assert_eq!(app.session.hierarchy.members_of(ModuleId(2)), vec![member]);
}

/// A Member change is an undo step like any parameter's: the Member module
/// is in the undo snapshot, though the file writes its value on the
/// instance.
#[test]
fn undo_puts_member_back() {
    let mut app = app_with(VOICE);
    mode(
        &mut app,
        Message::SetMode {
            tile: 2,
            mode: Mode::Select,
        },
    );
    mode(&mut app, Message::AddMember(2));
    let member = app.session.hierarchy.member_module(&ModulePath::top(2)).unwrap();
    app.turn_param(member.0, 0, 2.0);
    let _ = app.send(AppMessage::SliderReleased);
    assert_eq!(voice_rows(&mut app).params[0].label, "Member: 2 (Voice 2)");
    app.undo();
    assert_eq!(voice_rows(&mut app).params[0].label, "Member: 1 (Voice)");
}

/// A stack of two copies whose inside is a select of two constant members.
const STACKED_SELECT: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Stacked select"),
    subpatches: {
        "stack": SubPatch(
            metadata: Metadata(name: "Stack"),
            mode: Stack,
            copies: 2,
            modules: [
                PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("select"), params: {"member": 2.0}),
                PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
        ),
        "select": SubPatch(
            metadata: Metadata(name: "Select"),
            mode: Select,
            members: [1, 2],
            modules: [
                PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("saw")),
                PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("square")),
            ],
        ),
        "saw": SubPatch(
            metadata: Metadata(name: "Saw"),
            modules: [
                PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 0.25}),
                PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
        ),
        "square": SubPatch(
            metadata: Metadata(name: "Square"),
            modules: [
                PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": -0.25}),
                PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
        ),
    },
    modules: [
        PatchModule(id: 2, kind: "loom.struct.subpatch@1", subpatch: Some("stack")),
        PatchModule(id: 3, kind: "loom.io.audio-out@1"),
        PatchModule(id: 4, kind: "loom.clock.transport@1"),
    ],
    cables: [PatchCable(from: (2, "out"), to: (3, "in"))],
)"#;

/// A select inside a stack: when its playing member moves, Member follows
/// it in every copy, not only copy 0, so the copies play the same member.
#[test]
fn member_follows_its_member_in_every_copy_of_a_stack() {
    use loom_patch::flatten::{ADDED_STEP, MEMBER_STEP};
    let mut app = app_with(STACKED_SELECT);
    let member = |app: &App, path: &[u32]| {
        let id = at(app, path).expect("a Member module");
        app.session.controller.graph().node(id).unwrap().params[0]
    };
    let copy0 = [2, 1, ADDED_STEP + MEMBER_STEP];
    let copy1 = [2, COPY_STEP + 1, 1, ADDED_STEP + MEMBER_STEP];
    assert_eq!((member(&app, &copy0), member(&app, &copy1)), (2.0, 2.0));
    let square = app.session.hierarchy.members_of(at(&app, &[2, 1]).unwrap())[1];
    mode(
        &mut app,
        Message::MoveMember {
            member: square.0,
            up: true,
        },
    );
    assert_eq!((member(&app, &copy0), member(&app, &copy1)), (1.0, 1.0));
}
