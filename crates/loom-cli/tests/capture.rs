//! Tests for `loom capture` (`docs/design/audio-qa.md`, Device capture). Most need
//! no audio device. The one that needs a loopback device is ignored by
//! default and skips when `LOOM_LOOPBACK` is not set; CI's Loopback
//! capture job runs it on a PipeWire null sink. What reaches a loopback
//! during a stop is tested in `loom-host-desktop`'s ignored device tests.

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
    let out = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("devices")
        .output()
        .unwrap();
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("Audio inputs:"), "{stdout}");
    }
}

/// `loom capture` records the input side of the loopback named by
/// `LOOM_LOOPBACK` for half a second and exits with status 0. Nothing
/// plays into the loopback, so the file holds silence: this checks the
/// command, not what a device played. Needs `LOOM_LOOPBACK`.
#[test]
#[ignore = "needs a loopback audio device"]
fn captures_a_loopback_device() {
    let Ok(loopback) = std::env::var("LOOM_LOOPBACK") else {
        eprintln!("skipped: set LOOM_LOOPBACK to part of a loopback device's name, such as BlackHole");
        return;
    };
    let file = std::env::temp_dir().join(format!("loom-cli-capture-loopback-{}.wav", std::process::id()));
    let (status, stderr) = capture(&["--device", &loopback, "-o", file.to_str().unwrap(), "--seconds", "0.5"]);
    assert_eq!(status, 0, "{stderr}");
    // Printed only once the first frames have arrived.
    assert!(stderr.contains("Capturing \""), "{stderr}");

    let reader = hound::WavReader::open(&file).unwrap();
    let spec = reader.spec();
    assert_eq!((spec.channels, spec.bits_per_sample), (2, 32));
    assert_eq!(spec.sample_format, hound::SampleFormat::Float);
    // The length counts from the first frames, and the device delivers
    // whole buffers, so the bounds are loose.
    let seconds = f64::from(reader.duration()) / f64::from(spec.sample_rate);
    assert!((0.4..2.0).contains(&seconds), "captured {seconds} s");

    let _ = std::fs::remove_file(file);
}
