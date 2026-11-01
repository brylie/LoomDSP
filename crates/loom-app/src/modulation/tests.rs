use super::*;
use loom_core::{ParamOptions, Polarity::*};

const fn param(min: f32, max: f32) -> ParamSpec {
    ParamSpec {
        name: "p",
        label: "P",
        description: "A test parameter.",
        min,
        max,
        default: min,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
}

const LINEAR: ParamSpec = param(0.0, 1.0);
const WHOLE: ParamSpec = param(0.0, 10.0).integer();

fn on(depth: f32, polarity: Polarity) -> Reach {
    Reach {
        depth,
        polarity,
        enabled: true,
    }
}

fn off(depth: f32, polarity: Polarity) -> Reach {
    Reach {
        enabled: false,
        ..on(depth, polarity)
    }
}

fn range(spec: &ParamSpec, base: f32, reaches: &[Reach]) -> ModulationMark {
    mark(WidgetKind::Knob, spec, base, reaches)
}

fn assert_range(got: ModulationMark, low: f32, high: f32) {
    let ModulationMark::Range { low: l, high: h } = got else {
        panic!("expected a range, got {got:?}");
    };
    assert!(
        (l - low).abs() < 1e-6 && (h - high).abs() < 1e-6,
        "{l}..{h}, expected {low}..{high}"
    );
}

/// A quarter of the knob, or one step of a whole-number parameter if
/// that is larger: a toggle gets 1, three options 0.5, eight options
/// 0.25.
#[test]
fn default_depth_is_a_quarter_or_one_step() {
    assert_eq!(default_depth(&LINEAR), 0.25);
    assert_eq!(default_depth(&param(0.0, 1.0).integer()), 1.0);
    assert_eq!(default_depth(&param(0.0, 2.0).integer()), 0.5);
    assert_eq!(default_depth(&param(0.0, 7.0).integer()), 0.25);
    assert_eq!(default_depth(&WHOLE), 0.25);
}

#[test]
fn depths_read_as_signed_percentages() {
    assert_eq!(depth_text(0.25), "+25%");
    assert_eq!(depth_text(-0.1), "-10%");
    assert_eq!(depth_text(0.0), "0%");
    assert_eq!(depth_text(-0.001), "0%");
}

#[test]
fn polarity_choices_follow_or_override() {
    let options = PolarityChoice::options(Unipolar);
    assert_eq!(options[0].to_string(), "Follow source (unipolar)");
    assert_eq!(
        options.map(PolarityChoice::polarity),
        [None, Some(Unipolar), Some(Bipolar)]
    );
    assert_eq!(PolarityChoice::of(None, Bipolar), PolarityChoice::Follow(Bipolar));
    assert_eq!(PolarityChoice::of(Some(Unipolar), Bipolar), PolarityChoice::Unipolar);
}

#[test]
fn no_assignments_no_mark() {
    assert_eq!(range(&LINEAR, 0.5, &[]), ModulationMark::None);
}

#[test]
fn unipolar_reaches_one_way_and_bipolar_both() {
    assert_range(range(&LINEAR, 0.5, &[on(0.25, Unipolar)]), 0.5, 0.75);
    assert_range(range(&LINEAR, 0.5, &[on(-0.25, Unipolar)]), 0.25, 0.5);
    assert_range(range(&LINEAR, 0.5, &[on(0.25, Bipolar)]), 0.25, 0.75);
    // A negative bipolar depth swings just as far.
    assert_range(range(&LINEAR, 0.5, &[on(-0.25, Bipolar)]), 0.25, 0.75);
}

#[test]
fn assignments_add_up_then_clamp() {
    assert_range(
        range(
            &LINEAR,
            0.5,
            &[on(0.25, Unipolar), on(0.1, Bipolar), on(-0.05, Unipolar)],
        ),
        0.35,
        0.85,
    );
    assert_range(range(&LINEAR, 0.75, &[on(0.5, Bipolar)]), 0.25, 1.0);
    assert_range(range(&LINEAR, 0.1, &[on(-0.5, Unipolar)]), 0.0, 0.1);
}

#[test]
fn disabled_assignments_are_left_out() {
    assert_range(range(&LINEAR, 0.5, &[on(0.25, Unipolar), off(0.5, Bipolar)]), 0.5, 0.75);
}

#[test]
fn whole_numbers_round_each_end() {
    // The spec's example: 0 to 10, base 5, unipolar 0.25 ends at 8.
    assert_range(range(&WHOLE, 5.0, &[on(0.25, Unipolar)]), 0.5, 0.8);
    // The low end, 2.5, rounds half away from zero to 3, as in the
    // engine.
    assert_range(range(&WHOLE, 5.0, &[on(0.25, Bipolar)]), 0.3, 0.8);
}

#[test]
fn the_range_maps_through_the_control_position() {
    // 20 Hz to 20 kHz, logarithmic: 632 Hz sits at position 0.5.
    let cutoff = ParamSpec {
        max: 20_000.0,
        ..param(20.0, 20_000.0)
    }
    .logarithmic();
    let base = cutoff.denormalize(0.5);
    assert_range(range(&cutoff, base, &[on(0.1, Bipolar)]), 0.4, 0.6);
}

#[test]
fn a_zero_length_arc_shows_the_dot() {
    assert_eq!(range(&LINEAR, 0.5, &[on(0.0, Bipolar)]), ModulationMark::Dot);
    // Too small to reach the next whole number.
    assert_eq!(range(&WHOLE, 5.0, &[on(0.04, Unipolar)]), ModulationMark::Dot);
    // Pushing into the clamp at the maximum.
    assert_eq!(range(&LINEAR, 1.0, &[on(0.25, Unipolar)]), ModulationMark::Dot);
}

#[test]
fn every_assignment_off_shows_the_ring() {
    assert_eq!(
        range(&LINEAR, 0.5, &[off(0.25, Unipolar), off(0.5, Bipolar)]),
        ModulationMark::Ring
    );
    assert_eq!(
        mark(
            WidgetKind::Toggle,
            &param(0.0, 1.0).integer(),
            0.0,
            &[off(1.0, Unipolar)]
        ),
        ModulationMark::Ring
    );
}

#[test]
fn toggles_and_radio_buttons_show_the_dot() {
    let on_off = param(0.0, 1.0).integer();
    assert_eq!(
        mark(WidgetKind::Toggle, &on_off, 0.0, &[on(1.0, Unipolar)]),
        ModulationMark::Dot
    );
    let radio = WidgetKind::Radio;
    assert_eq!(
        mark(radio, &param(0.0, 2.0).integer(), 1.0, &[on(0.5, Bipolar)]),
        ModulationMark::Dot
    );
}

#[test]
fn polarity_follows_the_source_unless_overridden() {
    use loom_core::{EngineConfig, OutputRef, create_engine};

    let mut controller = create_engine(loom_modules::registry(), EngineConfig::default()).controller;
    let env = controller.add_module("loom.env.envelope").unwrap();
    let filter = controller.add_module("loom.filter.svf").unwrap();
    let cutoff = controller
        .graph()
        .node(filter)
        .unwrap()
        .descriptor
        .param_index("cutoff")
        .unwrap();
    // An envelope declares 0 to 1, so it is unipolar.
    let source = OutputRef { module: env, port: 0 };
    let id = controller.add_assignment(source, filter, cutoff, 0.25).unwrap();
    assert_eq!(reaches(controller.graph())[&(filter, cutoff)], vec![on(0.25, Unipolar)]);

    controller.set_assignment_polarity(id, Some(Bipolar)).unwrap();
    controller.set_assignment_enabled(id, false).unwrap();
    assert_eq!(reaches(controller.graph())[&(filter, cutoff)], vec![off(0.25, Bipolar)]);
}
