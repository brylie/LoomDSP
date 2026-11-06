use super::*;

/// Each variant prints the text the app and the CLI showed before the
/// warnings were typed (#621), so nothing the user reads changes.
#[test]
fn each_warning_keeps_its_text() {
    let unknown = |kinds: &[&str]| LoadWarning::UnknownModules {
        kinds: kinds.iter().map(|k| k.to_string()).collect(),
    };
    assert_eq!(
        unknown(&["loom.fx.shimmer@1"]).to_string(),
        "1 module is not in this version: `loom.fx.shimmer@1`. They make no sound. Their settings are kept when you save. Open the patch in a newer LoomDSP to play them."
    );
    assert_eq!(
        unknown(&["a@1", "b@1", "a@1", "c@1", "d@1", "e@1"]).to_string(),
        "6 modules are not in this version: `a@1`, `b@1`, `c@1` and 2 more. They make no sound. Their settings are kept when you save. Open the patch in a newer LoomDSP to play them."
    );
    assert_eq!(
        LoadWarning::TransportAdded { module: 9 }.to_string(),
        "the patch had no Transport, so one was added as module 9 (120 BPM, 4/4, running)"
    );
    assert_eq!(
        LoadWarning::RangeIgnored {
            module: 3,
            param: "gain".into()
        }
        .to_string(),
        "module 3: parameter `gain` offers no ranges, so its saved range is ignored"
    );
    let saved = ParamRange::new(-1.0, 1.0);
    let loaded = ParamRange::new(-5.0, 5.0);
    let not_kept = |reason| LoadWarning::RangeNotKept {
        module: 4,
        param: "offset".into(),
        saved,
        reason,
        loaded,
    };
    assert_eq!(
        not_kept(RangeReason::DoesNotHold { value: 3.0 }).to_string(),
        format!(
            "module 4: parameter `offset` was saved with range {saved}, which does not hold its value 3; it loads with {loaded}"
        )
    );
    assert_eq!(
        not_kept(RangeReason::NotOffered).to_string(),
        format!(
            "module 4: parameter `offset` was saved with range {saved}, which it does not offer; it loads with {loaded}"
        )
    );
    assert_eq!(
        LoadWarning::SharedRangeNotKept {
            module: 1,
            param: "low".into(),
            partner: "high".into(),
            loaded
        }
        .to_string(),
        format!(
            "module 1: parameters `low` and `high` share a range, and the saved ranges do not hold both values; they load with {loaded}"
        )
    );
}

/// Only unknown modules are a warning; the rest are notes.
#[test]
fn only_unknown_modules_are_a_warning() {
    assert_eq!(
        LoadWarning::UnknownModules {
            kinds: vec!["a@1".into()]
        }
        .level(),
        LoadLevel::Warning
    );
    assert_eq!(LoadWarning::TransportAdded { module: 1 }.level(), LoadLevel::Note);
    assert_eq!(
        LoadWarning::RangeIgnored {
            module: 1,
            param: "p".into()
        }
        .level(),
        LoadLevel::Note
    );
}
