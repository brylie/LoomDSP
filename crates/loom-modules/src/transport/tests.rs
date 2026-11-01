use super::*;
use loom_core::ModuleConfig;

fn transport() -> Box<dyn Module> {
    let ModuleRole::Dsp(create) = TRANSPORT.role else {
        unreachable!("the Transport is a DSP module")
    };
    let mut module = create(&ModuleConfig {
        sample_rate: 48_000.0,
        max_block: 1024,
    });
    for (index, param) in TRANSPORT.params.iter().enumerate() {
        module.set_param(index, param.default);
    }
    module
}

/// The first request sends every setting, so a loaded patch sets the
/// transport. Later requests send only changes.
#[test]
fn requests_send_changes_only() {
    let mut module = transport();
    let first = module.transport_request(false, false);
    assert_eq!(first.tempo, Some(120.0));
    assert_eq!(first.time_signature, Some(TimeSignature::FOUR_FOUR));
    assert_eq!(first.running, Some(true));
    assert!(!first.reset);
    assert!(module.transport_request(true, false).is_empty());
    module.set_param(TEMPO, 96.0);
    module.set_param(DENOMINATOR, 3.0);
    module.set_param(NUMERATOR, 7.0);
    let next = module.transport_request(false, false);
    assert_eq!(next.tempo, Some(96.0));
    assert_eq!(next.time_signature, Some(TimeSignature::new(7, 8)));
    assert_eq!(next.running, None);
}

/// A press of Reset asks for one reset; holding it asks for no more.
#[test]
fn reset_press_asks_once() {
    let mut module = transport();
    module.transport_request(false, false);
    module.set_param(RESET, 1.0);
    assert!(module.transport_request(false, false).reset);
    assert!(!module.transport_request(false, false).reset);
    module.set_param(RESET, 0.0);
    assert!(!module.transport_request(false, false).reset);
}
