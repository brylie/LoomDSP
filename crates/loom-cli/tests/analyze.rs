//! Tests for `loom analyze`, run on the built binary
//! (`docs/design/audio-qa.md`, `loom analyze`).

use std::path::Path;
use std::process::Command;

/// A folder for this test's files, empty at the start and removed when
/// the guard drops, even when the test panics.
fn folder(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("loom-cli-analyze-{name}-"))
        .tempdir()
        .unwrap()
}

/// Writes mono `samples` at 1 kHz in the given format.
fn write(path: &Path, samples: &[f32], bits: u16, format: hound::SampleFormat) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 1000,
        bits_per_sample: bits,
        sample_format: format,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for &value in samples {
        match format {
            hound::SampleFormat::Float => writer.write_sample(value).unwrap(),
            hound::SampleFormat::Int => writer
                .write_sample((value * (1_i64 << (bits - 1)) as f32) as i32)
                .unwrap(),
        }
    }
    writer.finalize().unwrap();
}

/// Runs `loom analyze` with `args`; returns the exit status, stdout, and
/// stderr.
fn analyze(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_loom"))
        .arg("analyze")
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// A 0.5 peak at 0.25 s and one step of 0.5 there, on 2 s of silence:
/// longer than the 1 s the DC low-pass settles for.
fn signal() -> Vec<f32> {
    let mut samples = vec![0.0; 2000];
    samples[250] = 0.5;
    samples
}

/// `--json` prints every measurement, the same for 16-bit, 24-bit, and
/// 32-bit float files.
#[test]
fn json_has_every_measurement_in_each_format() {
    let temp = folder("json");
    let dir = temp.path();
    for (bits, format) in [
        (16, hound::SampleFormat::Int),
        (24, hound::SampleFormat::Int),
        (32, hound::SampleFormat::Float),
    ] {
        let path = dir.join(format!("{bits}.wav"));
        write(&path, &signal(), bits, format);
        let (status, stdout, _) = analyze(&[path.to_str().unwrap(), "--json"]);
        assert_eq!(status, 0, "{bits}-bit");
        let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
        assert_eq!(json["sample_rate"], 1000);
        assert_eq!(json["frames"], 2000);
        let c = &json["channels"][0];
        for key in [
            "first",
            "peak",
            "peak_dbfs",
            "peak_at",
            "over_ceiling",
            "full_scale",
            "non_finite",
            "max_step",
            "max_step_at",
            "dc",
            "dc_low_pass",
            "rms",
            "rms_dbfs",
        ] {
            assert!(!c[key].is_null(), "{bits}-bit: {key} missing in {c}");
        }
        assert_eq!(c["peak"], 0.5, "{bits}-bit");
        assert_eq!(c["peak_at"], 0.25, "{bits}-bit");
        assert_eq!(c["max_step"], 0.5, "{bits}-bit");
    }
}

/// Text output names the measurements.
#[test]
fn text_output_lists_the_measurements() {
    let temp = folder("text");
    let dir = temp.path();
    let path = dir.join("a.wav");
    write(&path, &signal(), 32, hound::SampleFormat::Float);
    let (status, stdout, _) = analyze(&[path.to_str().unwrap()]);
    assert_eq!(status, 0);
    for word in ["peak", "over ceiling", "at full scale", "largest step", "DC", "RMS"] {
        assert!(stdout.contains(word), "{word} missing in:\n{stdout}");
    }
}

/// A failed check exits with status 1 and names the check; passing checks
/// exit with 0.
#[test]
fn checks_set_the_exit_status() {
    let temp = folder("checks");
    let dir = temp.path();
    let path = dir.join("a.wav");
    write(&path, &signal(), 32, hound::SampleFormat::Float);
    let file = path.to_str().unwrap();

    let (status, _, _) = analyze(&[file, "--max-peak", "0.6", "--max-step", "0.6", "--max-dc", "0.001"]);
    assert_eq!(status, 0, "one sample is not DC");

    for (flag, limit) in [("--max-peak", "0.4"), ("--max-step", "0.4")] {
        let (status, _, stderr) = analyze(&[file, flag, limit]);
        assert_eq!(status, 1, "{flag}");
        assert!(stderr.contains(flag), "{flag} not named in: {stderr}");
    }

    let offset = dir.join("offset.wav");
    write(&offset, &[0.005; 2000], 32, hound::SampleFormat::Float);
    let (status, _, stderr) = analyze(&[offset.to_str().unwrap(), "--max-dc", "0.001"]);
    assert_eq!(status, 1);
    assert!(stderr.contains("--max-dc"), "--max-dc not named in: {stderr}");
}

/// `--max-dc` on a part of 1 s or less exits with status 2 and says the
/// part is too short, so it can never pass. Without the check, the file is
/// measured, and the low-pass DC prints as `null`.
#[test]
fn max_dc_on_a_short_part_exits_with_2() {
    let temp = folder("short");
    let dir = temp.path();
    let path = dir.join("a.wav");
    write(&path, &signal(), 32, hound::SampleFormat::Float);
    let file = path.to_str().unwrap();
    let (status, _, stderr) = analyze(&[file, "--from", "1.0", "--max-dc", "0.5"]);
    assert_eq!(status, 2, "{stderr}");
    assert!(stderr.contains("--max-dc") && stderr.contains("1.000 s"), "{stderr}");

    let (status, stdout, _) = analyze(&[file, "--from", "1.0", "--json"]);
    assert_eq!(status, 0);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["channels"][0]["dc_low_pass"].is_null(), "{stdout}");

    let (status, stdout, _) = analyze(&[file, "--from", "1.0"]);
    assert_eq!(status, 0);
    assert!(stdout.contains("too short"), "{stdout}");
}

/// `--from` and `--to` measure only part of the file.
#[test]
fn from_and_to_measure_a_part() {
    let temp = folder("part");
    let dir = temp.path();
    let path = dir.join("a.wav");
    write(&path, &signal(), 32, hound::SampleFormat::Float);
    let (status, stdout, _) = analyze(&[path.to_str().unwrap(), "--from", "0.5", "--json", "--max-peak", "0.1"]);
    assert_eq!(status, 0);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["frames"], 1500);
    assert_eq!(json["channels"][0]["peak"], 0.0);
}

/// NaN and infinite limits, NaN times, and `--from` after `--to` are
/// refused with status 2, so a check can never pass by accident.
#[test]
fn unusable_options_exit_with_2() {
    let temp = folder("options");
    let dir = temp.path();
    let path = dir.join("a.wav");
    write(&path, &signal(), 32, hound::SampleFormat::Float);
    let file = path.to_str().unwrap();
    for args in [
        vec!["--max-peak", "NaN"],
        vec!["--max-step", "inf"],
        vec!["--max-dc", "NaN"],
        vec!["--to", "NaN"],
        vec!["--from", "0.8", "--to", "0.2"],
    ] {
        let (status, _, stderr) = analyze(&[&[file][..], &args].concat());
        assert_eq!(status, 2, "{args:?}");
        assert!(stderr.contains(args[0]), "{args:?}: {stderr}");
    }
}

/// A NaN sample fails every check given, even when the finite samples
/// pass; with no checks, the file is only measured.
#[test]
fn non_finite_samples_fail_every_check() {
    let temp = folder("nan");
    let dir = temp.path();
    let path = dir.join("a.wav");
    let mut samples = signal();
    samples[500] = f32::NAN;
    write(&path, &samples, 32, hound::SampleFormat::Float);
    let file = path.to_str().unwrap();
    let (status, stdout, _) = analyze(&[file, "--json"]);
    assert_eq!(status, 0, "measuring alone does not fail");
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["channels"][0]["non_finite"], 1);
    let (status, _, stderr) = analyze(&[file, "--max-peak", "0.9", "--max-dc", "0.5"]);
    assert_eq!(status, 1);
    assert!(stderr.contains("--max-peak") && stderr.contains("--max-dc"), "{stderr}");
}

/// A file that cannot be read exits with status 2, not 1.
#[test]
fn an_unreadable_file_exits_with_2() {
    let temp = folder("bad");
    let dir = temp.path();
    let path = dir.join("not.wav");
    std::fs::write(&path, b"not a wav file").unwrap();
    assert_eq!(analyze(&[path.to_str().unwrap()]).0, 2);
    assert_eq!(analyze(&[dir.join("missing.wav").to_str().unwrap()]).0, 2);
}
