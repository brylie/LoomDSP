//! Tests for `loom check`, run on the built binary.

use std::process::Command;

/// A patch that saved a value for Button's transient `hold` loads, and
/// `loom check` says the value was ignored (transient-params.md).
#[test]
fn check_reports_a_saved_transient_value() {
    let dir = std::env::temp_dir().join(format!("loom-cli-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("held.ron");
    std::fs::write(
        &path,
        r#"Patch(version: 1, modules: [(id: 7, kind: "loom.ui.button@1", params: {"mode": 1.0, "hold": 1.0})])"#,
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout.contains("module 7 saves `hold`"), "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Check fails on a missing module, while render warns and produces audio.
#[test]
fn unknown_module_is_reported_by_check_and_render() {
    let dir = std::env::temp_dir().join(format!("loom-cli-unknown-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("missing.ron");
    let wave = dir.join("render.wav");
    std::fs::write(
        &path,
        r#"Patch(version: 1, modules: [(id: 7, kind: "future.effect@1")])"#,
    )
    .unwrap();
    let check = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!check.status.success());
    assert!(String::from_utf8_lossy(&check.stdout).contains("future.effect@1"));
    let render = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("render")
        .arg(&path)
        .arg("-o")
        .arg(&wave)
        .args(["--seconds", "0.01"])
        .output()
        .unwrap();
    assert!(render.status.success(), "{}", String::from_utf8_lossy(&render.stderr));
    let warning = String::from_utf8_lossy(&render.stderr);
    assert!(warning.contains("warning:") && warning.contains("future.effect@1"));
    assert!(wave.exists());
    let _ = std::fs::remove_dir_all(dir);
}
