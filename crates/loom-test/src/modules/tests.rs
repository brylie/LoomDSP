use super::*;
use loom_core::{ParamMapping, WidgetKind};

#[test]
fn every_test_module_registers() {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    assert_eq!(registry.len(), ALL.len());
    for descriptor in ALL {
        assert!(descriptor.id.starts_with("test."), "{}", descriptor.id);
    }
}

#[test]
fn the_panel_has_every_widget_kind() {
    let kinds: Vec<WidgetKind> = PANEL.panel.widgets.iter().map(|w| w.kind).collect();
    let knob = |name: &str| {
        let w = PANEL.panel.widgets.iter().find(|w| w.bind == name).unwrap();
        assert_eq!(w.kind, WidgetKind::Knob, "{name}");
        PANEL.params[PANEL.param_index(name).unwrap()]
    };
    assert!(!knob("level").options.integer);
    assert!(knob("steps").options.integer);
    assert!(kinds.contains(&WidgetKind::Toggle));
    assert!(kinds.contains(&WidgetKind::Radio));
    assert!(kinds.contains(&WidgetKind::Button {
        mode: "mode",
        hold: "held"
    }));
    assert!(kinds.contains(&WidgetKind::Light));
    assert!(kinds.contains(&WidgetKind::Meter));
    assert!(kinds.contains(&WidgetKind::Scope));
}

#[test]
fn the_panel_has_every_mapping_and_polarity() {
    let params = PANEL.params;
    assert!(params.iter().any(|p| p.options.mapping == ParamMapping::Linear));
    assert!(params.iter().any(|p| p.options.mapping == ParamMapping::Logarithmic));
    assert!(params.iter().any(|p| p.options.integer));
    assert!(params.iter().any(|p| p.options.smooth_seconds > 0.0));
    let polarities: Vec<Polarity> = PANEL.outputs.iter().map(|p| p.polarity()).collect();
    assert_eq!(polarities, [Polarity::Unipolar, Polarity::Bipolar]);
}

#[test]
fn the_panel_writes_level_and_lights_on() {
    let factory = match PANEL.role {
        ModuleRole::Dsp(factory) => factory,
        _ => unreachable!(),
    };
    let mut module = factory(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 4,
    });
    module.set_param(LEVEL, 0.75);
    module.set_param(ON, 1.0);
    let (mut uni, mut bi) = (vec![0.0; 4], vec![0.0; 4]);
    loom_core::testing::run(module.as_mut(), 48_000.0, &[], &[], &mut [&mut uni, &mut bi], &[]);
    assert_eq!(uni, [0.75; 4]);
    assert_eq!(bi, [0.75; 4]);
    let mut lights = [0.0; 1];
    module.displays(&mut lights);
    assert_eq!(lights, [1.0]);
}
