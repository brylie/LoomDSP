//! Every module that declares an own phase really runs one
//! ([ADR-0042](../../../docs/adr/0042-unpatched-phase-inputs-are-wired-for-the-player.md)).
//!
//! `PortDefault::OwnPhase` makes the engine feed zeros, and the module must
//! run its own phase while the input has no cable. A module that declares
//! it and forgets the branch reads a constant 0 and plays silence, while its
//! help page still says "own phase". The list comes from the registry, so a
//! new module is checked without anyone adding it here.

use loom_core::testing::run;
use loom_core::{ModuleConfig, ModuleDescriptor, ModuleRole, PortDefault};

const RATE: f32 = 48_000.0;
const FRAMES: usize = 4_800;

/// Every registered DSP module with an input that defaults to its own phase,
/// and the index of that input.
fn own_phase_modules() -> Vec<(&'static ModuleDescriptor, usize)> {
    loom_modules::registry()
        .iter()
        .filter(|d| matches!(d.role, ModuleRole::Dsp(_)))
        .filter_map(|d| {
            d.inputs
                .iter()
                .position(|p| p.default == PortDefault::OwnPhase)
                .map(|i| (d, i))
        })
        .collect()
}

/// The first output of `descriptor` over [`FRAMES`] frames, with `phase_in`
/// on a cable of zeros when `cabled`. A `Value` input reads its value, and
/// any other input reads zeros: what an own phase feeds, and what bar phase,
/// beat phase, and the play gate read on the stopped transport `run` uses.
fn render(descriptor: &ModuleDescriptor, phase_in: usize, cabled: bool) -> Vec<f32> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        unreachable!("filtered to DSP modules")
    };
    let mut module = create(&ModuleConfig {
        sample_rate: RATE,
        max_block: FRAMES,
    });
    module.reset();
    let inputs: Vec<Vec<f32>> = descriptor
        .inputs
        .iter()
        .map(|p| match p.default {
            PortDefault::Value(v) => vec![v; FRAMES],
            _ => vec![0.0; FRAMES],
        })
        .collect();
    let connected: Vec<bool> = (0..descriptor.inputs.len()).map(|i| cabled && i == phase_in).collect();
    let inputs: Vec<&[f32]> = inputs.iter().map(Vec::as_slice).collect();
    let mut outputs: Vec<Vec<f32>> = descriptor.outputs.iter().map(|_| vec![0.0; FRAMES]).collect();
    let mut outputs: Vec<&mut [f32]> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
    run(module.as_mut(), RATE, &inputs, &connected, &mut outputs, &[]);
    outputs[0].to_vec()
}

/// With no cable, a module with an own phase moves; with a cable of zeros,
/// it plays something else. Both fail for a module that ignores the cable
/// test and reads the engine's zeros.
#[test]
fn every_own_phase_module_runs_its_own_phase() {
    let modules = own_phase_modules();
    assert!(!modules.is_empty(), "the oscillators declare an own phase");
    let mut failed = Vec::new();
    for (descriptor, phase_in) in modules {
        let own = render(descriptor, phase_in, false);
        let cabled = render(descriptor, phase_in, true);
        if own.iter().all(|&v| v == own[0]) {
            failed.push(format!(
                "{}: with no phase cable the output does not move",
                descriptor.id
            ));
        }
        if own == cabled {
            failed.push(format!(
                "{}: a cable of zeros plays the same as no cable, so the own phase never runs",
                descriptor.id
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
