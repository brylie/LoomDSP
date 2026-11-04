//! Every smoothed parameter tells the engine where its glide has got to, for
//! every module (modulation spec, rule 9). When the first assignment on a
//! smoothed parameter is added, the engine starts its own glide from that
//! value, so a module that forgets `Module::param_glide` steps. These tests
//! run over the whole registry, so a new module cannot miss it.

mod common;

use common::{SAMPLE_RATE, changed};
use loom_core::testing::run;
use loom_core::{Module, ModuleConfig, ModuleDescriptor, ModuleRole, ParamMapping, ParamSpec};
use loom_modules::{ALL, test_modules};
use loom_rt::Smoothed;

/// Frames per call to a module, the largest block the harness uses.
const BLOCK: usize = 512;
/// Frames after an edit at which a smoothed parameter is still gliding: the
/// shortest smoothing time in use is 5 ms, which is 240 frames at 48 kHz.
const STILL_GLIDING: usize = 64;
/// Frames after which every glide has settled.
const SETTLED: usize = SAMPLE_RATE as usize;

/// The DSP modules among the built-ins and the test modules.
fn modules() -> impl Iterator<Item = &'static ModuleDescriptor> {
    ALL.iter()
        .chain(test_modules::ALL)
        .copied()
        .filter(|descriptor| matches!(descriptor.role, ModuleRole::Dsp(_)))
}

/// A new instance of `descriptor`, started as the `Controller` starts one:
/// every parameter at its default, then `reset()` once.
fn start(descriptor: &ModuleDescriptor) -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = descriptor.role else {
        panic!("{} is not a DSP module", descriptor.id);
    };
    let mut module = create(&ModuleConfig {
        sample_rate: SAMPLE_RATE,
        max_block: BLOCK,
    });
    for (index, spec) in descriptor.params.iter().enumerate() {
        module.set_param(index, spec.default);
    }
    module.reset();
    module
}

/// Runs `module` for `frames` frames on silent, unconnected inputs.
fn advance(module: &mut dyn Module, descriptor: &ModuleDescriptor, frames: usize) {
    let silence = vec![0.0; BLOCK];
    let unconnected = vec![false; descriptor.inputs.len()];
    let mut buffers = vec![vec![0.0; BLOCK]; descriptor.outputs.len()];
    let mut left = frames;
    while left > 0 {
        let n = left.min(BLOCK);
        let inputs: Vec<&[f32]> = descriptor.inputs.iter().map(|_| &silence[..n]).collect();
        let mut outputs: Vec<&mut [f32]> = buffers.iter_mut().map(|buffer| &mut buffer[..n]).collect();
        run(module, SAMPLE_RATE, &inputs, &unconnected, &mut outputs, &[]);
        left -= n;
    }
}

/// Each smoothed parameter of each module, with its index.
fn smoothed_params() -> impl Iterator<Item = (&'static ModuleDescriptor, usize, &'static ParamSpec)> {
    modules().flat_map(|descriptor| {
        descriptor
            .params
            .iter()
            .enumerate()
            .filter(|(_, spec)| spec.options.smooth_seconds > 0.0)
            .map(move |(index, spec)| (descriptor, index, spec))
    })
}

/// A continuous smoothed parameter reports a value between where it was and
/// where it is going, and nothing once it has settled.
#[test]
fn every_continuous_smoothed_parameter_reports_its_glide() {
    let mut failed = Vec::new();
    for (descriptor, index, spec) in smoothed_params().filter(|(_, _, spec)| !spec.options.integer) {
        let name = format!("{}:{}", descriptor.id, spec.name);
        let (from, to) = (spec.default, changed(spec));
        let mut module = start(descriptor);
        module.set_param(index, to);
        advance(module.as_mut(), descriptor, STILL_GLIDING);
        match module.param_glide(index) {
            Some(value) if value > from.min(to) && value < from.max(to) => {}
            Some(value) => failed.push(format!("{name}: {value} is not between {from} and {to}")),
            None => failed.push(format!("{name}: reports no glide")),
        }
        advance(module.as_mut(), descriptor, SETTLED);
        if let Some(value) = module.param_glide(index) {
            failed.push(format!("{name}: still reports {value} after it settled"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A whole-number parameter reports no glide. The module fades on its own
/// smoother and reads the fill only for the state it wants. A fill started
/// part-way would round to the old state and reverse the fade (rule 9).
#[test]
fn whole_number_parameters_report_no_glide() {
    let mut failed = Vec::new();
    for (descriptor, index, spec) in smoothed_params().filter(|(_, _, spec)| spec.options.integer) {
        let mut module = start(descriptor);
        module.set_param(index, changed(spec));
        advance(module.as_mut(), descriptor, STILL_GLIDING);
        if let Some(value) = module.param_glide(index) {
            failed.push(format!("{}:{} reports {value}", descriptor.id, spec.name));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// A knob follows one curve, whether or not it has an assignment. The engine
/// glides in the control's position (`fill_param`), so a logarithmic parameter
/// spends equal time per octave, and a module must glide the same way (#615).
/// This also finds a logarithmic parameter that a module smooths in linear
/// units. Positions are compared with an absolute tolerance, as a relative one
/// fails on a parameter that glides through zero.
///
/// Two kinds are left out. A decibel parameter glides in linear gain in the
/// module and in dB in the engine, which rule 9 accepts. A whole-number one
/// rounds in the engine, and its module fades on a smoother of its own.
#[test]
fn modules_glide_on_the_curve_the_engine_uses() {
    /// The most the two positions may differ, on a 0 to 1 control.
    const TOLERANCE: f32 = 1e-4;
    let mut failed = Vec::new();
    let same_curve =
        |spec: &ParamSpec| !spec.options.integer && !matches!(spec.options.mapping, ParamMapping::Decibels { .. });
    for (descriptor, index, spec) in smoothed_params().filter(|(_, _, spec)| same_curve(spec)) {
        let (from, to) = (spec.default, changed(spec));
        let mut module = start(descriptor);
        module.set_param(index, to);
        // The engine's smoother, as `fill_param` runs it.
        let mut engine = Smoothed::new(spec.normalize(from), SAMPLE_RATE, spec.options.smooth_seconds);
        engine.set_target(spec.normalize(to));
        let mut frame = 0;
        for at in [16, 64, 160] {
            advance(module.as_mut(), descriptor, at - frame);
            for _ in frame..at {
                engine.tick();
            }
            frame = at;
            let got = module.param_glide(index).map(|value| spec.normalize(value));
            let want = engine.current();
            if got.is_none_or(|got| (got - want).abs() > TOLERANCE) {
                failed.push(format!(
                    "{}:{} at frame {at}: position {got:?}, the engine has {want}",
                    descriptor.id, spec.name
                ));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The run is not empty: the checks above see the parameters they claim to.
#[test]
fn the_run_covers_the_modules_that_smooth() {
    let names: Vec<_> = smoothed_params()
        .map(|(descriptor, _, spec)| (descriptor.id, spec.name))
        .collect();
    for wanted in [
        ("loom.eq.eq", "mid_q"),
        ("loom.filter.crossover", "low_split"),
        ("loom.dyn.dynamics", "threshold"),
        ("loom.mix.mixer", "mute1"),
        ("test.smooth", "level"),
        ("test.panel", "glide"),
    ] {
        assert!(names.contains(&wanted), "{wanted:?} is not in {names:?}");
    }
}
