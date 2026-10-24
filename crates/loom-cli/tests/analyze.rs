//! Tests for `loom analyze`, run on the built binary
//! (`docs/design/audio-qa.md`, `loom analyze`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// A folder for this test's files, empty at the start.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("loom-cli-analyze-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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

/// A 0.5 peak at 0.25 s and one step of 0.5 there, on silence.
fn signal() -> Vec<f32> {
    let mut samples = vec![0.0; 1000];
    samples[250] = 0.5;
    samples
}

/// `--json` prints every measurement, the same for 16-bit, 24-bit, and
/// 32-bit float files.
#[test]
fn json_has_every_measurement_in_each_format() {
    let dir = folder("json");
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
        assert_eq!(json["frames"], 1000);
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
            "dc_window",
            "rms",
            "rms_dbfs",
        ] {
            assert!(!c[key].is_null(), "{bits}-bit: {key} missing in {c}");
        }
        assert_eq!(c["peak"], 0.5, "{bits}-bit");
        assert_eq!(c["peak_at"], 0.25, "{bits}-bit");
        assert_eq!(c["max_step"], 0.5, "{bits}-bit");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Text output names the measurements.
#[test]
fn text_output_lists_the_measurements() {
    let dir = folder("text");
    let path = dir.join("a.wav");
    write(&path, &signal(), 32, hound::SampleFormat::Float);
    let (status, stdout, _) = analyze(&[path.to_str().unwrap()]);
    assert_eq!(status, 0);
    for word in ["peak", "over ceiling", "at full scale", "largest step", "DC", "RMS"] {
        assert!(stdout.contains(word), "{word} missing in:\n{stdout}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A failed check exits with status 1 and names the check; passing checks
/// exit with 0.
#[test]
fn checks_set_the_exit_status() {
    let dir = folder("checks");
    let path = dir.join("a.wav");
    write(&path, &signal(), 32, hound::SampleFormat::Float);
    let file = path.to_str().unwrap();

    let (status, _, _) = analyze(&[file, "--max-peak", "0.6", "--max-step", "0.6", "--max-dc", "0.01"]);
    assert_eq!(status, 0);

    for (flag, limit) in [("--max-peak", "0.4"), ("--max-step", "0.4"), ("--max-dc", "0.001")] {
        let (status, _, stderr) = analyze(&[file, flag, limit]);
        assert_eq!(status, 1, "{flag}");
        assert!(stderr.contains(flag), "{flag} not named in: {stderr}");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// `--from` and `--to` measure only part of the file.
#[test]
fn from_and_to_measure_a_part() {
    let dir = folder("part");
    let path = dir.join("a.wav");
    write(&path, &signal(), 32, hound::SampleFormat::Float);
    let (status, stdout, _) = analyze(&[path.to_str().unwrap(), "--from", "0.5", "--json", "--max-peak", "0.1"]);
    assert_eq!(status, 0);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["frames"], 500);
    assert_eq!(json["channels"][0]["peak"], 0.0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// NaN and infinite limits, NaN times, and `--from` after `--to` are
/// refused with status 2, so a check can never pass by accident.
#[test]
fn unusable_options_exit_with_2() {
    let dir = folder("options");
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
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A NaN sample fails every check given, even when the finite samples
/// pass; with no checks, the file is only measured.
#[test]
fn non_finite_samples_fail_every_check() {
    let dir = folder("nan");
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
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file that cannot be read exits with status 2, not 1.
#[test]
fn an_unreadable_file_exits_with_2() {
    let dir = folder("bad");
    let path = dir.join("not.wav");
    std::fs::write(&path, b"not a wav file").unwrap();
    assert_eq!(analyze(&[path.to_str().unwrap()]).0, 2);
    assert_eq!(analyze(&[dir.join("missing.wav").to_str().unwrap()]).0, 2);
    std::fs::remove_dir_all(&dir).unwrap();
}
