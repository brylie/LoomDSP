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
    let hierarchy = Hierarchy::from_patch(&patch, controller.graph(), ids).unwrap();
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
