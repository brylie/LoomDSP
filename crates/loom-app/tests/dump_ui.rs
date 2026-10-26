//! `loom-app --dump-ui`: prints what the UI shows as JSON and exits, with
//! no window, audio, or MIDI.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn dump(args: &[&str]) -> Output {
    let patch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
    dump_of(&patch, args)
}

fn dump_of(patch: &Path, args: &[&str]) -> Output {
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

/// The `--frames` test patch: see the comment at its top.
fn running() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/patches/running.ron")
}

/// The widget bound to `label` on module `id`.
fn widget(json: &serde_json::Value, id: u32, label: &str) -> serde_json::Value {
    let tile = json["tiles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id)
        .expect("a tile");
    let widgets = tile["widgets"].as_array().unwrap();
    widgets.iter().find(|w| w["label"] == label).expect("a widget").clone()
}

fn json_of(result: &Output) -> serde_json::Value {
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    serde_json::from_slice(&result.stdout).expect("JSON")
}

#[test]
fn frames_run_the_engine_first() {
    let patch = running();
    let still = json_of(&dump_of(&patch, &[]));
    assert_eq!(
        widget(&still, 3, "Cutoff")["live"],
        serde_json::Value::Null,
        "the engine has not run"
    );
    assert_eq!(widget(&still, 2, "Level")["position"], 0.0);
    assert_eq!(widget(&still, 4, "Out")["position"], 0.0);

    let ran = json_of(&dump_of(&patch, &["--frames", "4800"]));
    let knob = widget(&ran, 3, "Cutoff");
    let live = knob["live"].as_f64().expect("a live value");
    assert!(
        live > knob["position"].as_f64().unwrap(),
        "a positive source raises the cutoff: {knob}"
    );
    let level = widget(&ran, 2, "Level")["position"].as_f64().unwrap();
    assert!(level > 0.5, "the gate is held: {level}");
    // A steady 0.6 is -4.4 dB, which a meter from -60 dB to 0 dB shows at
    // about 0.926.
    let meter = widget(&ran, 4, "Out");
    let (rms, peak) = (meter["position"].as_f64().unwrap(), meter["peak"].as_f64().unwrap());
    assert!((rms - 0.926).abs() < 0.01, "the meter shows the constant: {meter}");
    assert!((peak - rms).abs() < 0.01, "a constant peaks at its RMS level: {meter}");
}

#[test]
fn frames_needs_dump_ui_or_screenshot_window() {
    let patch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/subtractive.ron");
    let result = Command::new(env!("CARGO_BIN_EXE_loom-app"))
        .arg(patch)
        .args(["--frames", "10", "--screenshot", "/dev/null"])
        .output()
        .expect("loom-app runs");
    assert!(!result.status.success());
}
