//! A fault captured only on a bar wrap must fail the suite's own scenario.

use super::on_wrap;
use loom_core::{
    Channels, Family, Module, ModuleDescriptor, ModuleRole, Panel, PortDefault, PortIo, PortRange, PortSpec,
    ProcessContext, SignalType, signal::WrapDetector,
};

/// A signal port with enough text for the harness's descriptor checks.
const fn port(name: &'static str) -> PortSpec {
    PortSpec {
        name,
        label: name,
        description: "A wrap capture test signal.",
        signal: SignalType::Modulation,
        default: PortDefault::Value(0.0),
        range: PortRange::Signal,
        channels: Channels::PerChannel,
    }
}

/// Stores an input only on a phase wrap, without guarding non-finite values.
static WRAP_CAPTURE: ModuleDescriptor = ModuleDescriptor {
    id: "test.wrap-capture",
    version: 1,
    name: "Wrap capture",
    category: "Test",
    family: Family::Interface,
    description: "Keeps the input sampled on a bar wrap.",
    inputs: &[
        port("in"),
        PortSpec {
            signal: SignalType::Phase,
            ..port("phase").with_default(PortDefault::BarPhase)
        },
    ],
    outputs: &[port("out")],
    params: &[],
    role: ModuleRole::Dsp(|_| Box::new(WrapCapture::default())),
    panel: Panel::EMPTY,
    displays: &[],
    roles: &[],
};

/// A faulty module that retains its captured value until the next bar.
#[derive(Default)]
struct WrapCapture {
    /// The previous phase, to recognise the wrap.
    wrap: WrapDetector,
    /// Input captured on the last wrap.
    held: f32,
}

impl Module for WrapCapture {
    fn process(&mut self, _: &ProcessContext<'_>, io: &mut PortIo<'_>) {
        let ([input, phase], [out]) = io.split::<2, 1>();
        for ((&input, &phase), out) in input.iter().zip(phase).zip(out) {
            if self.wrap.wrapped(phase) {
                self.held = input;
            }
            *out = self.held;
        }
    }
}

/// The actual suite builder must put bad values across the default phase wrap.
#[test]
#[should_panic(expected = "test.wrap-capture: after NaN on input `in` (frames 22150 to 22157)")]
fn the_shared_wrap_scenario_detects_a_bad_capture() {
    on_wrap(&WRAP_CAPTURE).check_non_finite_recovery();
}

/// A custom start also controls the length check, before any buffer indexing.
#[test]
#[should_panic(expected = "the non-finite check needs more than 22638")]
fn the_wrap_scenario_needs_time_to_recover() {
    on_wrap(&WRAP_CAPTURE).frames(22_638).check_non_finite_recovery();
}
