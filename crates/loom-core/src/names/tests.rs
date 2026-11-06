use super::*;
use crate::descriptor::fixtures::NAMED;
use std::collections::BTreeMap;

/// A module as the clipboard or the patch loader holds it: a descriptor,
/// and placeholder data when the kind is unknown.
struct Held {
    unknown: Option<UnknownModule>,
}

impl ModuleNames for Held {
    fn descriptor(&self) -> &'static ModuleDescriptor {
        if self.unknown.is_some() {
            UnknownModule::descriptor()
        } else {
            &NAMED
        }
    }

    fn unknown(&self) -> Option<&UnknownModule> {
        self.unknown.as_ref()
    }
}

fn known() -> Held {
    Held { unknown: None }
}

fn placeholder() -> Held {
    Held {
        unknown: Some(UnknownModule {
            kind: "future.effect@3".into(),
            params: BTreeMap::new(),
            ranges: BTreeMap::new(),
            inputs: vec!["input".into(), "side".into()],
            outputs: vec!["wet".into()],
            parameters: vec!["amount".into(), "tone".into(), "mix".into()],
        }),
    }
}

/// A registered module's names come from its descriptor, both ways.
#[test]
fn a_known_module_names_its_ports_and_parameters_both_ways() {
    let module = known();
    assert_eq!(
        (module.input_count(), module.output_count(), module.parameter_count()),
        (2, 2, 2)
    );
    assert_eq!(module.input_name(1), Some("cutoff"));
    assert_eq!(module.output_name(0), Some("low"));
    assert_eq!(module.parameter_name(1), Some("cutoff"));
    assert_eq!(module.input_index("cutoff"), Some(1));
    assert_eq!(module.output_index("high"), Some(1));
    assert_eq!(module.parameter_index("gain"), Some(0));
    assert_eq!(module.output_label(1), Some(NAMED.outputs[1].label));
    assert_eq!(module.parameter_label(0), Some(NAMED.params[0].label));
}

/// A placeholder's names come from the patch file, in file order, and its
/// labels are those names.
#[test]
fn a_placeholder_names_its_ports_and_parameters_both_ways() {
    let module = placeholder();
    assert_eq!(
        (module.input_count(), module.output_count(), module.parameter_count()),
        (2, 1, 3)
    );
    assert_eq!(module.input_name(1), Some("side"));
    assert_eq!(module.output_name(0), Some("wet"));
    assert_eq!(module.parameter_name(2), Some("mix"));
    assert_eq!(module.input_index("side"), Some(1));
    assert_eq!(module.output_index("wet"), Some(0));
    assert_eq!(module.parameter_index("tone"), Some(1));
    assert_eq!(module.output_label(0), Some("wet"));
    assert_eq!(module.parameter_label(1), Some("tone"));
}

/// A missing name or index gives `None` for both kinds, never a panic.
#[test]
fn missing_names_and_indices_are_none() {
    for module in [known(), placeholder()] {
        assert_eq!(module.input_name(9), None);
        assert_eq!(module.output_name(9), None);
        assert_eq!(module.parameter_name(9), None);
        assert_eq!(module.output_label(9), None);
        assert_eq!(module.parameter_label(9), None);
        assert_eq!(module.input_index("nope"), None);
        assert_eq!(module.output_index("nope"), None);
        assert_eq!(module.parameter_index("nope"), None);
    }
    // A placeholder does not answer with its silent descriptor's names.
    assert_eq!(placeholder().parameter_index("gain"), None);
}
