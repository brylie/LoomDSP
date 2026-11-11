use super::*;
use loom_core::{EngineConfig, EngineParts, create_engine};
use loom_patch::Patch;

/// The nested example loaded into a controller, with its hierarchy.
fn nested() -> (loom_core::Controller, Hierarchy) {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches");
    let text = std::fs::read_to_string(root.join("nested-subtractive.ron")).unwrap();
    let patch = Patch::from_ron(&text).unwrap();
    let EngineParts { mut controller, .. } = create_engine(loom_modules::registry(), EngineConfig::default());
    let mut ids = loom_patch::FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).unwrap();
    let hierarchy = Hierarchy::from_patch(&patch, controller.graph(), ids, controller.registry()).unwrap();
    (controller, hierarchy)
}

/// On a module tile the resolver gives the module's own parameter, within
/// its control range, as the canvas has always had it.
#[test]
fn a_module_control_resolves_to_its_own_parameter() {
    let (controller, hierarchy) = nested();
    let graph = controller.graph();
    let (id, node) = graph
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
        .unwrap();
    let widget = find_control(graph, &hierarchy, id.0, "cutoff").unwrap();
    let target = control_target(graph, &hierarchy, id.0, widget, ControlPart::Main).unwrap();
    assert_eq!(target.module, id);
    assert_eq!(target.index, node.parameter_index("cutoff").unwrap());
    assert_eq!(target.kind, WidgetKind::Knob);
    assert_eq!((target.spec.min, target.spec.max), (20.0, 20000.0));
    assert!(is_control(graph, &hierarchy, id.0, widget));
    assert!(widget_binds(graph, &hierarchy, id.0, widget, "cutoff"));
    assert_eq!(widget_name(graph, &hierarchy, id.0, widget), Some("cutoff".to_string()));
    assert!(control_target(graph, &hierarchy, id.0, widget, ControlPart::Second).is_none());
}

/// On the Voice tile the first surface control is the filter's cutoff two
/// levels down, a knob as inside, spanning the Filter's exposed range.
#[test]
fn a_surface_control_resolves_to_the_inner_parameter() {
    let (controller, hierarchy) = nested();
    let graph = controller.graph();
    let (svf, node) = graph
        .nodes()
        .find(|(_, n)| n.descriptor.id == "loom.filter.svf")
        .unwrap();
    assert_eq!(find_control(graph, &hierarchy, 2, "cutoff"), Some(0));
    assert_eq!(find_control(graph, &hierarchy, 2, "resonance"), None);
    let target = control_target(graph, &hierarchy, 2, 0, ControlPart::Main).unwrap();
    assert_eq!(target.module, svf);
    assert_eq!(target.index, node.parameter_index("cutoff").unwrap());
    assert_eq!(target.kind, WidgetKind::Knob);
    assert_eq!((target.spec.min, target.spec.max), (20.0, 20000.0));
    assert!(control_target(graph, &hierarchy, 2, 1, ControlPart::Main).is_none());
    assert!(control_target(graph, &hierarchy, 2, 0, ControlPart::Second).is_none());
    assert!(is_control(graph, &hierarchy, 2, 0));
    assert!(!is_control(graph, &hierarchy, 2, 1));
    assert!(widget_binds(graph, &hierarchy, 2, 0, "cutoff"));
    assert_eq!(widget_name(graph, &hierarchy, 2, 0), Some("cutoff".to_string()));
}

/// A whole number from 0 to 1 gets a toggle, named choices a segmented
/// control, radio buttons, or a select list by their count, and anything
/// else a knob.
#[test]
fn a_parameter_with_no_control_inside_gets_the_widget_its_value_kind_gets() {
    let spec = |min: f32, max: f32, integer: bool, choices: &'static [&'static str]| {
        let spec = ParamSpec {
            name: "p",
            label: "P",
            description: "A test parameter.",
            min,
            max,
            default: min,
            unit: "",
            options: loom_core::ParamOptions::DEFAULT,
        };
        let spec = if integer { spec.integer() } else { spec };
        spec.choices(choices)
    };
    assert_eq!(derived_kind(&spec(0.0, 1.0, true, &[])), WidgetKind::Toggle);
    assert_eq!(derived_kind(&spec(0.0, 7.0, true, &[])), WidgetKind::Knob);
    assert_eq!(derived_kind(&spec(0.0, 1.0, false, &[])), WidgetKind::Knob);
    assert_eq!(
        derived_kind(&spec(0.0, 2.0, true, &["a", "b", "c"])),
        WidgetKind::Segmented { width: 9 }
    );
    assert_eq!(
        derived_kind(&spec(0.0, 4.0, true, &["a", "b", "c", "d", "e"])),
        WidgetKind::Radio
    );
    assert_eq!(
        derived_kind(&spec(0.0, 5.0, true, &["a", "b", "c", "d", "e", "f"])),
        WidgetKind::Select
    );
}

/// A sub-patch that exposes three parameters of two inner modules, in an
/// order unlike the modules', and a top-level LFO that modulates one of
/// them through the surface.
const THREE: &str = r#"Patch(
    version: 1,
    metadata: Metadata(name: "Three"),
    subpatches: {
        "mix": SubPatch(
            metadata: Metadata(name: "Mix"),
            modules: [
                PatchModule(id: 1, kind: "loom.mix.mixer@1", position: Some((0.0, 0.0))),
                PatchModule(id: 2, kind: "loom.filter.svf@1", params: {"cutoff": 500.0}, position: Some((300.0, 0.0))),
            ],
            exposed: [
                Exposed(name: "Resonance", module: 2, param: "resonance"),
                Exposed(name: "Level 1", module: 1, param: "level1"),
                Exposed(name: "Cutoff", module: 2, param: "cutoff", range: Some((100.0, 1000.0))),
            ],
        ),
    },
    modules: [
        PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("mix"), position: Some((200.0, 0.0))),
        PatchModule(id: 2, kind: "loom.clock.transport@1", position: Some((0.0, 300.0))),
        PatchModule(id: 3, kind: "loom.mod.lfo@1", position: Some((0.0, 0.0))),
    ],
    modulation: [
        PatchModulation(source: (3, "out"), target: (1, "Cutoff"), depth: 0.2),
    ],
)"#;

/// [`THREE`] loaded, with its hierarchy and the tiles' positions.
fn three() -> (loom_core::Controller, Hierarchy, BTreeMap<u32, (f32, f32)>) {
    let patch = Patch::from_ron(THREE).unwrap();
    let EngineParts { mut controller, .. } = create_engine(loom_modules::registry(), EngineConfig::default());
    let mut ids = loom_patch::FlatIds::new();
    patch.load_with_ids(&mut controller, &mut ids).unwrap();
    let hierarchy = Hierarchy::from_patch(&patch, controller.graph(), ids, controller.registry()).unwrap();
    let positions = hierarchy
        .layouts(&patch)
        .into_iter()
        .filter_map(|(id, layout)| Some((id.0, layout.position?)))
        .collect();
    (controller, hierarchy, positions)
}

/// The scene of `level` for [`three`].
fn scene_of(
    controller: &loom_core::Controller,
    hierarchy: &Hierarchy,
    positions: &BTreeMap<u32, (f32, f32)>,
    level: &ModulePath,
) -> loom_canvas::Scene {
    crate::scene::build(controller, hierarchy, level, positions, &BTreeMap::new())
}

/// A sub-patch with three exposed parameters and no designed panel shows
/// three widgets in the order they were exposed, each the widget its
/// parameter has inside: two knobs and the mixer's fader, placed left to
/// right. The hover text names the inner module and parameter.
#[test]
fn three_exposed_parameters_make_three_widgets_in_exposure_order() {
    let (controller, hierarchy, positions) = three();
    let scene = scene_of(&controller, &hierarchy, &positions, &ModulePath::root());
    let tile = scene.module(1).expect("the Mix tile");
    assert_eq!(tile.title, "Mix");
    let labels: Vec<&str> = tile.panel.widgets.iter().map(|w| w.label.as_str()).collect();
    assert_eq!(labels, ["Resonance", "Level 1", "Cutoff"]);
    assert!(matches!(tile.panel.widgets[0].kind, WidgetViewKind::Knob { .. }));
    assert!(matches!(
        tile.panel.widgets[1].kind,
        WidgetViewKind::Fader { vertical: true, .. }
    ));
    assert!(matches!(tile.panel.widgets[2].kind, WidgetViewKind::Knob { .. }));
    let places: Vec<(u8, u8)> = tile.panel.widgets.iter().map(|w| (w.x, w.y)).collect();
    assert_eq!(places, [(0, 0), (3, 0), (5, 0)], "left to right on one row");
    assert_eq!(
        (tile.panel.width, tile.panel.height),
        (10, 6),
        "the row is as tall as the fader"
    );
    let note = |i: usize| tile.panel.widgets[i].hover.note.clone().unwrap_or_default();
    assert_eq!(note(0), "Controls SVF › Resonance");
    assert_eq!(
        note(1),
        "Controls Mixer › Level 1",
        "the parameter's label, not the panel caption"
    );
    assert_eq!(note(2), "Controls SVF › Cutoff");
    assert!(
        !tile.panel.widgets[2].hover.description.is_empty(),
        "the inner description"
    );
    assert!(
        !tile.panel.widgets[0].exposed,
        "a surface control is not itself exposed"
    );
    assert_eq!(tile.panel.widgets[2].hover.name, None);
}

/// Four or more exposed knobs wrap to a second row at 12 units.
#[test]
fn a_surface_wraps_at_twelve_units() {
    let knob = |label: &str| WidgetView {
        kind: WidgetViewKind::Knob { steps: 0 },
        x: 0,
        y: 0,
        label: label.to_string(),
        hover: HoverText::default(),
        hidden: false,
        exposed: false,
    };
    let panel = flow_layout((0..5).map(|i| knob(&i.to_string())).collect());
    let places: Vec<(u8, u8)> = panel.widgets.iter().map(|w| (w.x, w.y)).collect();
    assert_eq!(places, [(0, 0), (3, 0), (6, 0), (9, 0), (0, 3)]);
    assert_eq!((panel.width, panel.height), (12, 6));
    let empty = flow_layout(Vec::new());
    assert_eq!((empty.width, empty.height), (10, 2));
}

/// Inside the sub-patch, the exposed controls carry the mark and say what
/// they are exposed as; a control that is not exposed has neither.
#[test]
fn an_exposed_control_inside_has_its_mark_and_hover_text() {
    let (controller, hierarchy, positions) = three();
    let inside = ModulePath::top(1);
    let scene = scene_of(&controller, &hierarchy, &positions, &inside);
    let svf = hierarchy.id_at(&inside, 2).unwrap();
    let mixer = hierarchy.id_at(&inside, 1).unwrap();
    let widget = |tile: ModuleId, name: &str| {
        let index = find_control(controller.graph(), &hierarchy, tile.0, name).unwrap();
        scene.module(tile.0).unwrap().panel.widgets[index].clone()
    };
    let cutoff = widget(svf, "cutoff");
    assert!(cutoff.exposed);
    assert_eq!(cutoff.hover.note.as_deref(), Some("Exposed as Cutoff"));
    let resonance = widget(svf, "resonance");
    assert!(resonance.exposed);
    assert_eq!(resonance.hover.note.as_deref(), Some("Exposed as Resonance"));
    let level1 = widget(mixer, "level1");
    assert!(level1.exposed);
    assert_eq!(level1.hover.note.as_deref(), Some("Exposed as Level 1"));
    let level2 = widget(mixer, "level2");
    assert!(!level2.exposed);
    assert_eq!(level2.hover.note, None);
}

/// A surface control shows the inner parameter's value, in the exposed
/// range, and its modulation mark; the inner control shows the same
/// value in the module's own range.
#[test]
fn surface_values_follow_the_inner_parameter() {
    let (mut controller, hierarchy, _) = three();
    let inside = ModulePath::top(1);
    let svf = hierarchy.id_at(&inside, 2).unwrap();
    let cutoff = controller.graph().node(svf).unwrap().parameter_index("cutoff").unwrap();
    let mut values = PanelValues::new();
    crate::scene::update_panel_values(&mut values, &controller, &hierarchy, &BTreeMap::new());
    let surface = values.get(1, 2).expect("the surface cutoff has a value");
    assert_eq!(
        surface.text,
        crate::param::format(&controller.graph().node(svf).unwrap().spec(cutoff).unwrap(), 500.0)
    );
    // 500 Hz in a 100 to 1000 Hz logarithmic range: past the middle.
    assert!(surface.position > 0.6 && surface.position < 0.8, "{}", surface.position);
    assert_ne!(
        surface.modulation,
        loom_canvas::ModulationMark::None,
        "the LFO's assignment shows"
    );
    let inner_widget = find_control(controller.graph(), &hierarchy, svf.0, "cutoff").unwrap();
    let inner = values.get(svf.0, inner_widget).unwrap();
    assert_eq!(inner.text, surface.text);
    assert!(inner.position < surface.position, "the inner knob spans 20 to 20000 Hz");

    controller.set_param(svf, cutoff, 900.0).unwrap();
    crate::scene::update_panel_values(&mut values, &controller, &hierarchy, &BTreeMap::new());
    let surface = values.get(1, 2).unwrap();
    assert!(surface.position > 0.9, "{}", surface.position);
    assert_eq!(
        values.get(1, 0).map(|v| v.modulation),
        Some(loom_canvas::ModulationMark::None)
    );
}

/// The LFO's line to the cutoff ends at the surface control of the Mix
/// tile, since its target is inside. Inside the sub-patch, where the LFO
/// is not, no line is drawn.
#[test]
fn a_line_across_the_boundary_ends_at_the_surface_control() {
    let (controller, hierarchy, positions) = three();
    let scene = scene_of(&controller, &hierarchy, &positions, &ModulePath::root());
    let lines = scene.modulations();
    assert_eq!(lines.len(), 1);
    assert_eq!((lines[0].from, lines[0].to, lines[0].widget), ((3, 0), 1, Some(2)));
    let inside = scene_of(&controller, &hierarchy, &positions, &ModulePath::top(1));
    assert!(inside.modulations().is_empty());
}

/// Inside the Voice, the Filter tile's surface control is exposed on the
/// Voice too: it carries the mark and its note says so. The Voice's own
/// surface control, at the top, is not exposed further.
#[test]
fn a_surface_item_exposed_further_out_has_the_mark() {
    let (controller, hierarchy) = nested();
    let voice = ModulePath::top(2);
    let scene = crate::scene::build(&controller, &hierarchy, &voice, &BTreeMap::new(), &BTreeMap::new());
    let filter_tile = hierarchy.id_at(&voice, 5).unwrap();
    let control = &scene.module(filter_tile.0).unwrap().panel.widgets[0];
    assert!(control.exposed);
    assert_eq!(
        control.hover.note.as_deref(),
        Some("Controls SVF › Cutoff. Exposed as cutoff")
    );
    let top = crate::scene::build(
        &controller,
        &hierarchy,
        &ModulePath::root(),
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    let control = &top.module(2).unwrap().panel.widgets[0];
    assert!(!control.exposed);
    assert_eq!(control.hover.note.as_deref(), Some("Controls Filter › cutoff"));
}

/// A structural voice count exposed on an otherwise ordinary sub-patch surface.
fn voices_app() -> App {
    let patch = Patch::from_ron(
        r#"Patch(version: 1,
        subpatches: {"v": SubPatch(
            modules: [(id: 1, kind: "test.voices@1")],
            exposed: [(name: "Voices", module: 1, param: "voices")])},
        modules: [(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("v"), position: Some((200.0, 0.0)))])"#,
    )
    .unwrap();
    let loaded = crate::Loaded::read(
        EngineConfig::default(),
        false,
        crate::Source::Recovered(Box::new(patch)),
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

/// An exposed structural value uses the ordinary surface edit path and recompiles.
#[test]
fn an_exposed_voice_count_changes_the_inner_bundle_width() {
    let mut app = voices_app();
    let target = app.control_target(1, 0, ControlPart::Main).unwrap();
    assert!(target.spec.options.shapes_graph);
    app.edit(|app| {
        let _ = app.update(crate::Message::Canvas(loom_canvas::CanvasEvent::TypedValue {
            module: 1,
            widget: 0,
            text: "8".to_string(),
        }));
    });
    let c = &app.session.controller;
    let out = c.output_ref(target.module, "out").unwrap();
    assert_eq!(c.last_report().width(out).voices, 8);
    let _ = app.update(crate::Message::Undo);
    assert_eq!(app.session.controller.last_report().width(out).voices, 1);
    let _ = app.update(crate::Message::Redo);
    assert_eq!(app.session.controller.last_report().width(out).voices, 8);
}

/// A surface cannot make an inner structural parameter eligible for MIDI learn.
#[test]
fn an_exposed_voice_count_cannot_be_learned() {
    let mut ui = crate::ui_test::Ui::with_app(voices_app());
    ui.send(crate::Message::Learn(crate::learn::Message::Toggle));
    let at = ui.control(1, "Voices");
    ui.move_to(at);
    ui.press();
    ui.release();
    assert!(ui.app.session.learn.armed().is_none());
    assert!(!ui.app.session.scene.is_live_control(1, 0));
    let target = ui.app.control_target(1, 0, ControlPart::Main).unwrap();
    assert!(
        ui.app
            .session
            .controller
            .map_midi(
                loom_core::MidiControl {
                    channel: 0,
                    controller: 12
                },
                target.module,
                target.index
            )
            .is_err()
    );
}

/// An exposed name cannot bypass the inner parameter's modulation restriction.
#[test]
fn an_exposed_voice_count_cannot_take_modulation() {
    let mut app = voices_app();
    let target = app.control_target(1, 0, ControlPart::Main).unwrap();
    let source = app.session.controller.output_ref(target.module, "out").unwrap();
    assert!(
        app.session
            .controller
            .add_assignment(source, target.module, target.index, 0.5)
            .is_err()
    );
    assert!(
        !crate::matrix::targets(app.session.controller.graph())
            .iter()
            .any(|item| item.module == target.module && item.param == target.index)
    );
    assert!(!app.session.scene.is_live_control(1, 0));
}
