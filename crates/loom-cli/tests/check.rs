//! Tests for `loom check`, run on the built binary.

use std::process::Command;

/// A patch that saved a value for Button's transient `hold` loads, and
/// `loom check` says the value was ignored (transient-params.md).
#[test]
fn check_reports_a_saved_transient_value() {
    let temp = tempfile::Builder::new().prefix("loom-cli-check-").tempdir().unwrap();
    let dir = temp.path();
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
}

/// Check fails on a missing module, while render warns and produces audio.
#[test]
fn unknown_module_is_reported_by_check_and_render() {
    let temp = tempfile::Builder::new().prefix("loom-cli-unknown-").tempdir().unwrap();
    let dir = temp.path();
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
}

/// A patch with sub-patches is flattened: `check` counts them and `render`
/// writes audio (sub-patches.md, Flattening).
#[test]
fn nested_patches_check_and_render() {
    let temp = tempfile::Builder::new().prefix("loom-cli-nested-").tempdir().unwrap();
    let dir = temp.path();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/nested-subtractive.ron");
    let path = dir.join("nested.ron");
    std::fs::copy(&source, &path).unwrap();
    let wave = dir.join("render.wav");
    let check = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(
        check.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(stdout.contains("2 sub-patches, flattened"), "{stdout}");
    assert!(stdout.contains("10 modules"), "{stdout}");
    // A module inside a definition is named by its own id and its
    // sub-patch, since its flat id appears nowhere in the file.
    let skipped = dir.join("skipped.ron");
    std::fs::write(
        &skipped,
        r#"Patch(version: 1, subpatches: {"v": SubPatch(metadata: Metadata(name: "Voice"), modules: [PatchModule(id: 4, kind: "loom.osc.sine@1")])},
            modules: [PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("v")), PatchModule(id: 2, kind: "loom.io.audio-out@1")])"#,
    )
    .unwrap();
    let check = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("check")
        .arg(&skipped)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(
        check.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(
        stdout.contains("not processed, no path to the output: Sine 4 in Voice"),
        "{stdout}"
    );
    let render = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("render")
        .arg(&path)
        .arg("-o")
        .arg(&wave)
        .args(["--seconds", "0.01"])
        .output()
        .unwrap();
    assert!(render.status.success(), "{}", String::from_utf8_lossy(&render.stderr));
    assert!(wave.exists());
}

/// A missing patch file fails with the path, then the system's reason, as it
/// did when `loom-patch` read files (#617).
#[test]
fn check_names_a_missing_file() {
    let temp = tempfile::Builder::new().prefix("loom-cli-missing-").tempdir().unwrap();
    let path = temp.path().join("no-such-patch.ron");
    let out = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(stderr.starts_with(&format!("error: {}: ", path.display())), "{stderr}");
}

/// `loom check` passes on the polysynth example: it loads with nothing
/// unknown, every module processed, and no feedback loop (polyphony.md, The
/// shipped example).
#[test]
fn check_passes_on_the_polysynth_example() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/polysynth.ron");
    let out = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("\"Polysynth\""), "{stdout}");
    assert!(stdout.contains("12 modules, 11 cables"), "{stdout}");
    assert!(stdout.contains("no feedback loops"), "{stdout}");
    assert!(!stdout.contains("not processed"), "{stdout}");
    assert!(!stdout.contains("unknown module"), "{stdout}");
    assert!(stderr.is_empty(), "the patch loads as saved: {stderr}");
}

/// `loom check` passes on the large reference patch: it loads with nothing
/// unknown, every module processed, and no feedback loop (polyphony.md, The
/// large reference patch).
#[test]
fn check_passes_on_the_polysynth_large_example() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/polysynth-large.ron");
    let out = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("check")
        .arg(&path)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("\"Layered polysynth (benchmark)\""), "{stdout}");
    assert!(stdout.contains("96 modules, 119 cables"), "{stdout}");
    assert!(stdout.contains("no feedback loops"), "{stdout}");
    assert!(!stdout.contains("not processed"), "{stdout}");
    assert!(!stdout.contains("unknown module"), "{stdout}");
    assert!(stderr.is_empty(), "the patch loads as saved: {stderr}");
}

/// The CLI uses a linked part from the selected library. With no folder
/// or a missing file, it renders the stored copy.
#[test]
fn linked_parts_in_check_and_render_follow_the_selected_library() {
    let temp = tempfile::Builder::new().prefix("loom-cli-library-").tempdir().unwrap();
    let dir = temp.path();
    let library = dir.join("library");
    std::fs::create_dir_all(&library).unwrap();
    let patch = dir.join("linked.ron");
    std::fs::write(
        &patch,
        r#"Patch(version: 1,
        modules: [
            PatchModule(id: 1, kind: "loom.struct.subpatch@1", subpatch: Some("voice")),
            PatchModule(id: 2, kind: "loom.io.audio-out@1"),
        ],
        cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
        subpatches: {"voice": SubPatch(metadata: Metadata(name: "Voice"), library: Some("Voice.ron"),
            modules: [
                PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 0.25}),
                PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
            ],
            cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
        )},
    )"#,
    )
    .unwrap();
    let wave = dir.join("render.wav");
    let render = |use_library: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_loom"));
        cmd.arg("render")
            .arg(&patch)
            .arg("-o")
            .arg(&wave)
            .args(["--seconds", "0.02"]);
        if use_library {
            cmd.arg("--library").arg(&library);
        }
        let run = cmd.output().unwrap();
        let notes = String::from_utf8(run.stderr).unwrap();
        assert!(run.status.success(), "{notes}");
        let sample = hound::WavReader::open(&wave)
            .unwrap()
            .samples::<f32>()
            .last()
            .unwrap()
            .unwrap();
        (sample, notes)
    };
    assert_eq!(render(false).0, 0.25);
    let (sample, notes) = render(true);
    assert_eq!(sample, 0.25);
    assert!(notes.contains("is missing, so its stored copy is used"), "{notes}");

    std::fs::write(
        library.join("Voice.ron"),
        r#"Patch(version: 1, metadata: Metadata(name: "Voice"),
        modules: [
            PatchModule(id: 1, kind: "loom.math.constant@1", params: {"value": 0.5}),
            PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
        ],
        cables: [PatchCable(from: (1, "out"), to: (2, "in"))],
    )"#,
    )
    .unwrap();
    let (sample, notes) = render(true);
    assert_eq!(sample, 0.5);
    assert!(notes.contains("Updated from the library: Voice."), "{notes}");
    assert_eq!(render(false).0, 0.25);
    let check = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("check")
        .arg(&patch)
        .arg("--library")
        .arg(&library)
        .output()
        .unwrap();
    assert!(check.status.success());
    assert!(String::from_utf8_lossy(&check.stderr).contains("Updated from the library: Voice."));
}
