use super::*;
use crate::descriptor::fixtures::*;

/// Looked up at compile time, the way modules name their indices.
const NAMED_CUTOFF: usize = NAMED.param_index("cutoff").unwrap();

const NAMED_CUTOFF_IN: usize = NAMED.input_index("cutoff").unwrap();

const NAMED_HIGH_OUT: usize = NAMED.output_index("high").unwrap();

/// Each lookup searches its own list, at compile time and at run time.
#[test]
fn indices_are_found_by_name_in_their_own_list() {
    assert_eq!((NAMED_CUTOFF, NAMED_CUTOFF_IN, NAMED_HIGH_OUT), (1, 1, 1));
    assert_eq!(NAMED.param_index("gain"), Some(0));
    assert_eq!(NAMED.input_index("in"), Some(0));
    assert_eq!(NAMED.output_index("low"), Some(0));
    // `in` is an input, not a parameter or an output.
    assert_eq!(NAMED.param_index("in"), None);
    assert_eq!(NAMED.output_index("in"), None);
}

/// Only the whole name matches: not a prefix, a longer name, or another case.
#[test]
fn only_the_exact_name_matches() {
    for name in ["cut", "cutoffs", "Cutoff", ""] {
        assert_eq!(NAMED.param_index(name), None, "{name:?}");
        assert_eq!(NAMED.input_index(name), None, "{name:?}");
    }
}

/// A range widget's two parameters load with one range: a saved one they agree
/// on that holds both, or else the smallest that does, and no saved value
/// changes (control-ranges.md, Ranges on a range widget).
#[test]
fn a_range_widgets_parameters_load_with_one_range() {
    const R1: ParamRange = ParamRange::new(-1.0, 1.0);
    const R5: ParamRange = ParamRange::new(-5.0, 5.0);
    const R10: ParamRange = ParamRange::new(-10.0, 10.0);
    const OFFERED: &[ParamRange] = &[R1, R5, R10];
    static PARAMS: [ParamSpec; 3] = [
        param("a", -10.0, 10.0, 0.0, "").ranges(OFFERED, R1),
        param("b", -10.0, 10.0, 1.0, "").ranges(OFFERED, R1),
        param("alone", -10.0, 10.0, 1.0, "").ranges(OFFERED, R1),
    ];
    static WIDGETS: [Widget; 1] = [Widget::range(0, 0, "a", "b", "Span")];
    let d = ModuleDescriptor {
        params: &PARAMS,
        panel: Panel::new(6, 2, &WIDGETS),
        ..NAMED
    };
    assert_eq!(d.range_partner(0), Some(1));
    assert_eq!(d.range_partner(1), Some(0));
    assert_eq!(d.range_partner(2), None);

    let load = |index, saved: [Option<ParamRange>; 2], values: [f32; 2]| {
        let at = |spec: &ParamSpec| usize::from(spec.name == "b");
        d.load_range(index, |spec| saved[at(spec)], |spec| values[at(spec)])
    };
    assert_eq!(load(0, [None, None], [0.0, 1.0]), Some((R1, false)), "the default");
    assert_eq!(
        load(0, [None, None], [0.0, 3.0]),
        Some((R5, false)),
        "holds both, no report"
    );
    assert_eq!(
        load(1, [None, None], [0.0, 3.0]),
        Some((R5, false)),
        "the same for both"
    );
    assert_eq!(
        load(0, [Some(R10), Some(R10)], [0.0, 1.0]),
        Some((R10, false)),
        "agreed"
    );
    assert_eq!(
        load(0, [Some(R1), Some(R10)], [0.0, 1.0]),
        Some((R1, true)),
        "they differ"
    );
    assert_eq!(
        load(1, [Some(R1), Some(R10)], [0.0, 1.0]),
        Some((R1, true)),
        "the same for both"
    );
    assert_eq!(
        load(0, [Some(R1), Some(R1)], [0.0, 3.0]),
        Some((R5, true)),
        "does not hold both"
    );
}

/// A voice-count module like Voice assigner: `mode` filtered by `voices`, and
/// `unison` shown only in Oldest and Quietest.
static COUNTED: ModuleDescriptor = ModuleDescriptor {
    id: "test.counted",
    version: 1,
    name: "Counted",
    category: "Test",
    family: crate::Family::Interface,
    description: "",
    inputs: &[],
    outputs: &[],
    params: &[
        param("voices", 1.0, 16.0, 1.0, "").integer(),
        param("mode", 0.0, 3.0, 2.0, "")
            .integer()
            .choices(&["Oldest", "Quietest", "Legato", "Retrigger"])
            .choices_by_count("voices", 2),
        param("unison", 1.0, 4.0, 1.0, "").integer().shown_when("mode", &[0, 1]),
    ],
    role: ModuleRole::UnitDelay,
    panel: Panel::new(
        12,
        4,
        &[
            Widget::number(0, 0, "unison"),
            Widget::light(6, 0, "held").shown_when("mode", &[0, 1]),
        ],
    ),
    displays: &[],
    roles: &[],
};
const COUNTED_MODE: usize = COUNTED.param_index("mode").unwrap();
const COUNTED_UNISON: usize = COUNTED.param_index("unison").unwrap();

/// The effective value of a count-filtered choice is the stored one when the
/// count offers it, else the first choice offered there.
#[test]
fn a_count_filtered_choice_acts_with_the_mode_that_plays() {
    // 8 voices, stored Legato: plays Oldest.
    assert_eq!(COUNTED.effective_value(COUNTED_MODE, &[8.0, 2.0, 1.0]), 0.0);
    // 8 voices, stored Quietest: kept.
    assert_eq!(COUNTED.effective_value(COUNTED_MODE, &[8.0, 1.0, 1.0]), 1.0);
    // 1 voice, stored Oldest: plays Legato.
    assert_eq!(COUNTED.effective_value(COUNTED_MODE, &[1.0, 0.0, 1.0]), 2.0);
    // 1 voice, stored Retrigger: kept.
    assert_eq!(COUNTED.effective_value(COUNTED_MODE, &[1.0, 3.0, 1.0]), 3.0);
    // A parameter with no count filter acts with its stored value.
    assert_eq!(COUNTED.effective_value(COUNTED_UNISON, &[8.0, 2.0, 3.0]), 3.0);
}

/// Missing values count as defaults, and an index past the parameters does
/// not panic.
#[test]
fn effective_value_falls_back_on_short_values_and_bad_indices() {
    // No values: 1 voice, default Legato.
    assert_eq!(COUNTED.effective_value(COUNTED_MODE, &[]), 2.0);
    // Only the count: the stored mode is its default, Legato, which falls
    // back to Oldest at 8 voices.
    assert_eq!(COUNTED.effective_value(COUNTED_MODE, &[8.0]), 0.0);
    assert_eq!(COUNTED.effective_value(99, &[8.0, 2.0, 1.0]), 0.0);
    assert!(COUNTED.is_shown(COUNTED_UNISON, &[8.0]));
}

/// A control and a display widget hide by the effective choice, not the
/// stored one, so a control never hides while it still changes the sound.
#[test]
fn hiding_follows_the_effective_choice() {
    let [unison, light] = COUNTED.panel.widgets else {
        panic!("two widgets");
    };
    for (values, shown) in [
        ([8.0, 0.0, 1.0], true),  // 8 voices, Oldest
        ([8.0, 2.0, 1.0], true),  // 8 voices, stored Legato plays Oldest
        ([1.0, 2.0, 1.0], false), // 1 voice, Legato
        ([1.0, 0.0, 1.0], false), // 1 voice, stored Oldest plays Legato
    ] {
        assert_eq!(COUNTED.is_shown(COUNTED_UNISON, &values), shown, "{values:?}");
        assert_eq!(COUNTED.is_widget_shown(unison, &values), shown, "{values:?}");
        assert_eq!(COUNTED.is_widget_shown(light, &values), shown, "{values:?}");
    }
}
