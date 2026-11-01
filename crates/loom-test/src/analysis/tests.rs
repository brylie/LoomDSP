use super::*;

/// Mono audio at 1 kHz, so times are easy to read.
fn mono(samples: Vec<f32>) -> Audio {
    Audio {
        sample_rate: 1000,
        channels: vec![samples],
    }
}

/// The peak is the largest size, with its time and its level in dBFS.
#[test]
fn finds_the_peak_and_its_time() {
    let mut samples = vec![0.1; 1000];
    samples[250] = -0.5;
    let a = analyze(&mono(samples), None, None);
    let c = &a.channels[0];
    assert_eq!(c.peak, 0.5);
    assert_eq!(c.peak_at, 0.25);
    assert!((c.peak_dbfs - (-6.0206)).abs() < 1e-3);
    assert_eq!(c.first, 0.1);
    assert_eq!(a.frames, 1000);
    assert_eq!(a.seconds, 1.0);
}

/// One jump shows as the largest step, at the sample after the jump.
#[test]
fn finds_one_step() {
    let mut samples = vec![0.0; 100];
    for s in &mut samples[40..] {
        *s = 0.3;
    }
    let c = &analyze(&mono(samples.clone()), None, None).channels[0];
    assert!((c.max_step - 0.3).abs() < 1e-6);
    assert_eq!(c.max_step_at, 0.04);
    assert!((max_step(&samples) - 0.3).abs() < 1e-6);
}

/// A DC offset shows in the whole mean and in the window mean. The
/// window finds an offset that the whole mean dilutes.
#[test]
fn measures_dc() {
    let c = &analyze(&mono(vec![0.25; 1000]), None, None).channels[0];
    assert!((c.dc - 0.25).abs() < 1e-9);
    assert!((c.dc_window - 0.25).abs() < 1e-9);
    assert!((c.rms - 0.25).abs() < 1e-6);

    let mut samples = vec![0.0; 1000];
    for s in &mut samples[500..600] {
        *s = -0.5;
    }
    let c = &analyze(&mono(samples), None, None).channels[0];
    assert!((c.dc - (-0.05)).abs() < 1e-9);
    assert!((c.dc_window - (-0.5)).abs() < 1e-9, "{}", c.dc_window);
}

/// NaN and infinity are counted and left out of the other measurements.
#[test]
fn counts_non_finite_samples() {
    let c = &analyze(&mono(vec![0.5, f32::NAN, 0.5, f32::INFINITY, 0.5]), None, None).channels[0];
    assert_eq!(c.non_finite, 2);
    assert_eq!(c.peak, 0.5);
    assert_eq!(c.max_step, 0.0);
    assert!((c.dc - 0.5).abs() < 1e-9);
}

/// Samples over the ceiling and at full scale are counted separately.
#[test]
fn counts_samples_over_the_ceiling_and_at_full_scale() {
    let c = &analyze(&mono(vec![0.5, 0.9, -0.95, 1.0, -1.2]), None, None).channels[0];
    assert_eq!(c.over_ceiling, 4);
    assert_eq!(c.full_scale, 2);
}

/// `from` and `to` measure only a part, and times stay those of the file.
#[test]
fn measures_a_part() {
    let mut samples = vec![0.0; 1000];
    samples[100] = 0.9;
    samples[700] = 0.4;
    let a = analyze(&mono(samples), Some(0.5), Some(0.8));
    assert_eq!(a.frames, 300);
    assert_eq!(a.from_seconds, 0.5);
    assert_eq!(a.channels[0].peak, 0.4);
    assert_eq!(a.channels[0].peak_at, 0.7);
}

/// The tape helpers give infinity for broken audio, never a small value
/// that a check could pass on.
#[test]
fn helpers_do_not_hide_non_finite_samples() {
    assert_eq!(peak(&[0.1, f32::NAN]), f32::INFINITY);
    assert_eq!(max_step(&[0.0, f32::NAN]), f32::INFINITY);
    assert_eq!(peak(&[f32::NEG_INFINITY]), f32::INFINITY);
    assert_eq!(peak(&[0.25, -0.5]), 0.5);
    assert_eq!(max_step(&[0.25, -0.5]), 0.75);
}

/// A file in another format names the file and the format; a broken
/// file keeps the decoder's error as its source.
#[test]
fn read_errors_name_the_file_and_keep_the_source() {
    use std::error::Error;
    let dir = std::env::temp_dir().join(format!("loom-test-analysis-errors-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("8bit.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 8000,
        bits_per_sample: 8,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    writer.write_sample(0_i8).unwrap();
    writer.finalize().unwrap();
    let e = read_wav(&path).unwrap_err();
    assert!(
        e.to_string().contains("8bit.wav") && e.to_string().contains("8-bit"),
        "{e}"
    );
    assert!(e.source().is_none());

    let broken = dir.join("broken.wav");
    std::fs::write(&broken, b"not a wav file").unwrap();
    let e = read_wav(&broken).unwrap_err();
    assert!(e.to_string().contains("broken.wav"), "{e}");
    assert!(e.source().is_some());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Silence gives zeros, and a level of negative infinity dBFS.
#[test]
fn silence_and_empty_audio() {
    let c = &analyze(&mono(vec![0.0; 10]), None, None).channels[0];
    assert_eq!(c.peak, 0.0);
    assert_eq!(c.peak_dbfs, f64::NEG_INFINITY);
    let a = analyze(&mono(Vec::new()), None, None);
    assert_eq!(a.frames, 0);
    assert_eq!(a.channels[0].first, 0.0);
}

/// Integer and float WAV files read back to the same values.
#[test]
fn reads_16_24_and_32_bit_files() {
    let dir = std::env::temp_dir().join(format!("loom-test-analysis-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (bits, format) in [
        (16, hound::SampleFormat::Int),
        (24, hound::SampleFormat::Int),
        (32, hound::SampleFormat::Float),
    ] {
        let path = dir.join(format!("{bits}.wav"));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44_100,
            bits_per_sample: bits,
            sample_format: format,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for value in [0.5f32, -0.25, 0.0, 0.75] {
            match format {
                hound::SampleFormat::Float => writer.write_sample(value).unwrap(),
                hound::SampleFormat::Int => {
                    writer.write_sample((value * (1 << (bits - 1)) as f32) as i32).unwrap();
                }
            }
        }
        writer.finalize().unwrap();
        let audio = read_wav(&path).unwrap();
        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.channels, vec![vec![0.5, 0.0], vec![-0.25, 0.75]], "{bits}-bit");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
