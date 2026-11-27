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

/// Mono audio at 48 kHz, for tones.
fn mono_48k(samples: Vec<f32>) -> Audio {
    Audio {
        sample_rate: 48_000,
        channels: vec![samples],
    }
}

/// `seconds` of a sine at `hz` with peak `peak`, plus `offset`, at 48 kHz.
fn sine(hz: f64, peak: f64, offset: f64, seconds: f64) -> Vec<f32> {
    let frames = (seconds * 48_000.0) as usize;
    (0..frames)
        .map(|n| (peak * (std::f64::consts::TAU * hz * n as f64 / 48_000.0).sin() + offset) as f32)
        .collect()
}

/// The low-pass DC of the first channel, which must be measured.
fn low_pass_dc(audio: &Audio) -> f64 {
    analyze(audio, None, None).channels[0]
        .dc_low_pass
        .expect("the part is long enough to measure DC")
}

/// A DC offset shows in the whole mean and in the low-pass DC.
#[test]
fn measures_dc() {
    let c = &analyze(&mono(vec![0.25; 2000]), None, None).channels[0];
    assert!((c.dc - 0.25).abs() < 1e-9);
    // Both stages start at 0 and rise toward 0.25. The largest reading is
    // the last, at 2 s, when they are within 1e-4 of it.
    let dc = c.dc_low_pass.unwrap();
    assert!((dc - 0.25).abs() < 1e-4, "{dc}");
    assert!((c.rms - 0.25).abs() < 1e-6);
}

/// A short DC fault reads smaller than its size: a 100 ms burst of 0.5
/// reads about 0.11, the figure in the spec's known limits.
#[test]
fn a_short_burst_reads_smaller() {
    let mut samples = vec![0.0f32; 3 * 48_000];
    for s in &mut samples[72_000..76_800] {
        *s = 0.5;
    }
    let dc = low_pass_dc(&mono_48k(samples));
    assert!((0.10..0.12).contains(&dc), "{dc}");
}

/// Tones with no DC are not DC (#789): 40 Hz, middle C, and 1 kHz at peak
/// 0.5 pass a 0.001 limit, where a mean over 100 ms read middle C as about
/// 0.003. The figures are those in the spec's table, Measuring DC.
#[test]
fn tones_are_not_dc() {
    for (hz, most) in [(40.0, 0.0005), (261.6, 0.00004), (1000.0, 0.00001)] {
        let dc = low_pass_dc(&mono_48k(sine(hz, 0.5, 0.0, 3.0)));
        assert!(dc.abs() < most, "{hz} Hz: {dc}");
        assert!(dc.abs() < 0.001, "{hz} Hz: {dc}");
    }
}

/// A small offset under a tone is read as that offset, and its sign kept.
#[test]
fn an_offset_under_a_tone_is_dc() {
    for offset in [0.01, -0.002] {
        let dc = low_pass_dc(&mono_48k(sine(261.6, 0.5, offset, 3.0)));
        assert!((dc - offset).abs() < 1e-4, "{offset}: {dc}");
    }
}

/// A part no longer than the settling time has no low-pass DC; one sample
/// more has.
#[test]
fn a_short_part_has_no_low_pass_dc() {
    assert_eq!(
        analyze(&mono(vec![0.5; 1000]), None, None).channels[0].dc_low_pass,
        None
    );
    assert!(
        analyze(&mono(vec![0.5; 1001]), None, None).channels[0]
            .dc_low_pass
            .is_some()
    );
    let audio = mono(vec![0.5; 1500]);
    assert_eq!(analyze(&audio, Some(0.5), None).channels[0].dc_low_pass, None);
    assert_eq!(analyze(&mono(Vec::new()), None, None).channels[0].dc_low_pass, None);
}

/// A NaN in the part is read as 0 by the low-pass, so the reading after it
/// is still a number.
#[test]
fn a_nan_does_not_stay_in_the_low_pass() {
    let mut samples = vec![0.25; 3000];
    samples[100] = f32::NAN;
    let dc = analyze(&mono(samples), None, None).channels[0].dc_low_pass.unwrap();
    assert!(dc.is_finite() && (dc - 0.25).abs() < 1e-3, "{dc}");
}

/// The reading is the same on every run.
#[test]
fn the_low_pass_dc_is_repeatable() {
    let audio = mono_48k(sine(40.0, 0.5, 0.001, 2.0));
    assert_eq!(low_pass_dc(&audio).to_bits(), low_pass_dc(&audio).to_bits());
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
    let temp = tempfile::Builder::new()
        .prefix("loom-analysis-errors-")
        .tempdir()
        .unwrap();
    let dir = temp.path();
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
    let temp = tempfile::Builder::new().prefix("loom-analysis-").tempdir().unwrap();
    let dir = temp.path();
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
}
