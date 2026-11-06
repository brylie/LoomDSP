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
