//! `loom-app --dump-ui`: prints what the UI shows as JSON and exits, with
//! no window, audio, or MIDI.

use std::path::Path;
use std::process::{Command, Output};

fn dump(args: &[&str]) -> Output {
    let patch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
    Command::new(env!("CARGO_BIN_EXE_loom-app"))
        .arg(patch)
        .arg("--dump-ui")
        .args(args)
        .output()
        .expect("loom-app runs")
}

#[test]
fn prints_the_selected_module_in_the_inspector() {
    let result = dump(&["--select", "4"]);
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(json["title"], "subtractive.ron - LoomDSP");
    assert_eq!(json["selection"], serde_json::json!([4]));
    assert!(json["tiles"].as_array().is_some_and(|t| !t.is_empty()));
    let module = &json["inspector"]["module"];
    assert_eq!(module["id"], 4);
    assert!(module["params"].as_array().is_some_and(|p| !p.is_empty()), "{module}");
}

#[test]
fn an_unknown_module_fails() {
    let result = dump(&["--select", "999"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no module 999"));
}
