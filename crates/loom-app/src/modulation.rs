//! What a control shows of its modulation assignments: the range arc on a
//! knob, or the dot. Spec: `docs/design/modulation.md`, Seeing assignments.

use loom_canvas::ModulationMark;
use loom_core::{Graph, ModuleId, ParamSpec, Polarity, WidgetKind};
use std::collections::BTreeMap;

/// One assignment as the range arc sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reach {
    /// Share of the control's range, from -1 to 1.
    pub depth: f32,
    /// The polarity the engine uses: the override, or the source's own.
    pub polarity: Polarity,
    /// False when the assignment is off.
    pub enabled: bool,
}

/// The assignments on every modulated parameter, by module and parameter
/// index, in engine order.
pub fn reaches(graph: &Graph) -> BTreeMap<(ModuleId, usize), Vec<Reach>> {
    let mut out: BTreeMap<(ModuleId, usize), Vec<Reach>> = BTreeMap::new();
    for (_, a) in graph.assignments() {
        let Some(source) = graph
            .node(a.source.module)
            .and_then(|n| n.descriptor.outputs.get(a.source.port))
        else {
            continue;
        };
        out.entry((a.target, a.param)).or_default().push(Reach {
            depth: a.depth,
            polarity: a.polarity.unwrap_or(source.polarity),
            enabled: a.enabled,
        });
    }
    out
}

/// The mark for a control bound to `spec`, at value `base`, with these
/// assignments.
///
/// A knob gets the range from `base + low` to `base + high` in its 0 to 1
/// position, clamped, where each enabled assignment adds to `low` and
/// `high` by the spec's table. A whole-number parameter rounds each end the
/// way the engine rounds the value. A range of zero length gives the dot.
pub fn mark(kind: WidgetKind, spec: &ParamSpec, base: f32, reaches: &[Reach]) -> ModulationMark {
    if reaches.is_empty() {
        return ModulationMark::None;
    }
    if reaches.iter().all(|r| !r.enabled) {
        return ModulationMark::DimmedDot;
    }
    if kind != WidgetKind::Knob {
        return ModulationMark::Dot;
    }
    let (mut low, mut high) = (0.0_f32, 0.0_f32);
    for r in reaches.iter().filter(|r| r.enabled) {
        match r.polarity {
            Polarity::Unipolar => {
                low += r.depth.min(0.0);
                high += r.depth.max(0.0);
            }
            Polarity::Bipolar => {
                low -= r.depth.abs();
                high += r.depth.abs();
            }
        }
    }
    let position = spec.normalize(base);
    let end = |offset: f32| {
        let t = (position + offset).clamp(0.0, 1.0);
        if spec.options.integer {
            spec.normalize(spec.denormalize(t))
        } else {
            t
        }
    };
    let (low, high) = (end(low), end(high));
    if high > low {
        ModulationMark::Range { low, high }
    } else {
        ModulationMark::Dot
    }
}

#[cfg(test)]
mod tests {
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
    fn every_assignment_off_dims_the_dot() {
        assert_eq!(
            range(&LINEAR, 0.5, &[off(0.25, Unipolar), off(0.5, Bipolar)]),
            ModulationMark::DimmedDot
        );
        assert_eq!(
            mark(
                WidgetKind::Toggle,
                &param(0.0, 1.0).integer(),
                0.0,
                &[off(1.0, Unipolar)]
            ),
            ModulationMark::DimmedDot
        );
    }

    #[test]
    fn toggles_and_radio_buttons_show_the_dot() {
        let on_off = param(0.0, 1.0).integer();
        assert_eq!(
            mark(WidgetKind::Toggle, &on_off, 0.0, &[on(1.0, Unipolar)]),
            ModulationMark::Dot
        );
        let radio = WidgetKind::Radio {
            options: &["a", "b", "c"],
        };
        assert_eq!(
            mark(radio, &param(0.0, 2.0).integer(), 1.0, &[on(0.5, Bipolar)]),
            ModulationMark::Dot
        );
    }

    #[test]
    fn polarity_follows_the_source_unless_overridden() {
        use loom_core::{EngineConfig, OutputRef, Registry, create_engine};
        use std::sync::Arc;

        let mut registry = Registry::new();
        loom_modules::register_all(&mut registry).unwrap();
        let mut controller = create_engine(Arc::new(registry), EngineConfig::default()).controller;
        let env = controller.add_module("loom.env.adsr").unwrap();
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
}
