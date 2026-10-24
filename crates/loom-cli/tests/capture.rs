//! Tests for `loom capture` that need no audio device
//! (`docs/design/audio-qa.md`, Part 4). The capture of a real loopback
//! device is tested in `loom-host-desktop`'s ignored device tests.

use std::process::Command;

/// Runs `loom capture` with `args`; returns the exit status and stderr.
fn capture(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("capture")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A device that does not match is a broken run (status 2), and the
/// error names what was looked for.
#[test]
fn no_matching_device_exits_with_2() {
    let file = std::env::temp_dir().join(format!("loom-cli-capture-none-{}.wav", std::process::id()));
    let name = "no device has this name 8f3c";
    let (status, stderr) = capture(&["--device", name, "-o", file.to_str().unwrap(), "--seconds", "0.1"]);
    assert_eq!(status, 2, "{stderr}");
    assert!(stderr.contains("error:"), "{stderr}");
    let _ = std::fs::remove_file(file);
}

/// A length that is not a positive number exits with status 2 before
/// any device is opened.
#[test]
fn an_unusable_length_exits_with_2() {
    for seconds in ["0", "-1", "NaN", "inf", "1e300"] {
        // With `=`, clap takes `-1` as the value, so the check in `capture`
        // is what refuses it.
        let length = format!("--seconds={seconds}");
        let (status, stderr) = capture(&["--device", "x", "-o", "unused.wav", &length]);
        assert_eq!(status, 2, "--seconds {seconds}: {stderr}");
        assert!(stderr.contains("--seconds"), "{stderr}");
    }
}

/// `loom devices` lists audio inputs, so a capture device can be found.
/// A machine with no audio system may fail to list anything; then there is
/// nothing to check.
#[test]
fn devices_lists_audio_inputs() {
    let out = Command::new(env!("CARGO_BIN_EXE_loom")).arg("devices").output().unwrap();
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("Audio inputs:"), "{stdout}");
    }
}
