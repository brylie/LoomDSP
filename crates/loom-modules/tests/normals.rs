//! Every unconnected input plays its declared normal
//! ([ADR-0043](../../../docs/adr/0043-unpatched-means-normalled.md)).
//!
//! An input with a fixed-value default must play the same with no cable as
//! with a cable that holds that value. A module that checks
//! `is_connected` and makes some other value in code shows one default in
//! its hover text and plays another, and a padded voice of a narrower
//! bundle, which reads the declared default, plays differently from an
//! unpatched input. The list comes from the registry, so a new module is
//! checked without anyone adding it here.
//!
//! The switched inputs ADR-0043 names do something other than read a value
//! when nothing reaches them. They are listed in [`SWITCHED`], each with
//! its reason, and skipped.

use loom_core::testing::run;
use loom_core::{ModuleConfig, ModuleDescriptor, ModuleRole, PortDefault, TRANSPORT_MODULE};

const RATE: f32 = 48_000.0;
const FRAMES: usize = 2_400;

/// Inputs that switch to another source, or take no part, when nothing
/// reaches them (ADR-0043, decision 3): `(module id, input name, why)`.
const SWITCHED: &[(&str, &str, &str)] = &[
    ("loom.dyn.dynamics", "sidechain", "keys from the module's own input"),
    ("loom.rand.chance", "trigger2", "follows the lane above"),
    ("loom.rand.chance", "trigger3", "follows the lane above"),
    ("loom.rand.chance", "trigger4", "follows the lane above"),
    ("loom.logic.logic", "in1", "an unconnected input does not vote"),
    ("loom.logic.logic", "in2", "an unconnected input does not vote"),
    ("loom.logic.logic", "in3", "an unconnected input does not vote"),
    ("loom.logic.logic", "in4", "an unconnected input does not vote"),
    (
        TRANSPORT_MODULE,
        "run",
        "a cable takes over from the Run toggle only while connected",
    ),
    (
        TRANSPORT_MODULE,
        "reset",
        "a cable takes over from the panel only while connected",
    ),
];

/// True when `input` of `module` is a switched input.
fn switched(module: &str, input: &str) -> bool {
    SWITCHED.iter().any(|&(m, i, _)| m == module && i == input)
}

/// A signal for input `k` that is not any port's default and moves, so a
/// module's output depends on it: a slow wave between 0.1 and 0.9, with a
/// different phase per input.
fn driven(k: usize) -> Vec<f32> {
    (0..FRAMES)
        .map(|n| 0.5 + 0.4 * ((n as f32) * 0.003 + k as f32).sin())
        .collect()
}

/// Every output of `descriptor` over [`FRAMES`] frames. Every input but
/// `normal_in` carries a cable with [`driven`], so a fallback on any other
/// input cannot hide this one. `normal_in` holds its default `value`, on a
/// cable when `cabled`.
fn render(descriptor: &ModuleDescriptor, normal_in: usize, value: f32, cabled: bool) -> Vec<Vec<f32>> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!("filtered to DSP modules")
    };
    let mut module = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: FRAMES,
    });
    module.reset();
    let inputs: Vec<Vec<f32>> = (0..descriptor.inputs.len())
        .map(|k| if k == normal_in { vec![value; FRAMES] } else { driven(k) })
        .collect();
    let connected: Vec<bool> = (0..descriptor.inputs.len()).map(|k| k != normal_in || cabled).collect();
    let inputs: Vec<&[f32]> = inputs.iter().map(Vec::as_slice).collect();
    let mut outputs: Vec<Vec<f32>> = descriptor.outputs.iter().map(|_| vec![0.0; FRAMES]).collect();
    {
        let mut slices: Vec<&mut [f32]> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
        run(module.as_mut(), RATE, &inputs, &connected, &mut slices, &[]);
    }
    outputs
}

/// No module plays something other than its declared default on an
/// unconnected fixed-value input, except the switched inputs.
#[test]
fn every_unconnected_input_plays_its_declared_normal() {
    let mut checked = 0;
    let mut failed = Vec::new();
    for descriptor in loom_modules::registry().iter() {
        if !matches!(descriptor.role, ModuleRole::Dsp(_)) {
            continue;
        }
        for (k, port) in descriptor.inputs.iter().enumerate() {
            let PortDefault::Value(value) = port.default else {
                // Transport signals are filled by the engine, and an own
                // phase is ADR-0042's switched input.
                continue;
            };
            if switched(descriptor.id, port.name) {
                continue;
            }
            checked += 1;
            let unpatched = render(descriptor, k, value, false);
            let cabled = render(descriptor, k, value, true);
            let same = unpatched
                .iter()
                .zip(&cabled)
                .all(|(a, b)| a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits()));
            if !same {
                failed.push(format!(
                    "{} input `{}`: with no cable it plays something other than its declared default {value}",
                    descriptor.id, port.name
                ));
            }
        }
    }
    assert!(
        checked > 50,
        "the registry has fixed-value inputs to check, found {checked}"
    );
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

/// Every switched input names a real input of a registered module, so the
/// list cannot keep an exception for something that is gone.
#[test]
fn every_switched_input_exists() {
    let registry = loom_modules::registry();
    for &(module, input, why) in SWITCHED {
        let descriptor = registry
            .iter()
            .find(|d| d.id == module)
            .unwrap_or_else(|| panic!("{module} is registered ({why})"));
        assert!(
            descriptor.inputs.iter().any(|p| p.name == input),
            "{module} has an input `{input}` ({why})"
        );
    }
}
