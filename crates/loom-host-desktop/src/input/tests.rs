use super::*;

/// A float configuration reported by a test device.
fn config(channels: u16, rate: u32, format: cpal::SampleFormat) -> cpal::SupportedStreamConfig {
    cpal::SupportedStreamConfig::new(channels, rate, cpal::SupportedBufferSize::Unknown, format)
}

/// A float rate range reported by a test device.
fn range(channels: u16, min: u32, max: u32, format: cpal::SampleFormat) -> cpal::SupportedStreamConfigRange {
    cpal::SupportedStreamConfigRange::new(channels, min, max, cpal::SupportedBufferSize::Unknown, format)
}

/// An interface's default stereo format must not hide its other pairs.
#[test]
fn input_config_exposes_available_channel_pairs() {
    let options = AudioOptions {
        sample_rate: Some(48_000),
        buffer_size: Some(256),
        ..AudioOptions::default()
    };
    let ranges = || {
        Ok(vec![
            range(2, 44_100, 96_000, cpal::SampleFormat::F32),
            range(8, 44_100, 96_000, cpal::SampleFormat::F32),
            range(16, 44_100, 96_000, cpal::SampleFormat::F32),
        ])
    };
    let chosen = choose_input_config(
        "Interface",
        config(2, 48_000, cpal::SampleFormat::F32),
        ranges,
        &options,
    )
    .unwrap();
    assert_eq!(chosen.channels, 8);
    assert_eq!(chosen.sample_rate, 48_000);
    assert_eq!(chosen.buffer_size, cpal::BufferSize::Fixed(256));
}

/// At another rate, use the smallest format that exposes all eight.
#[test]
fn input_config_uses_a_rate_compatible_multichannel_format() {
    let options = AudioOptions {
        sample_rate: Some(96_000),
        ..AudioOptions::default()
    };
    let ranges = || {
        Ok(vec![
            range(8, 44_100, 48_000, cpal::SampleFormat::F32),
            range(2, 44_100, 96_000, cpal::SampleFormat::F32),
            range(12, 44_100, 96_000, cpal::SampleFormat::F32),
            range(16, 44_100, 96_000, cpal::SampleFormat::F32),
        ])
    };
    let chosen = choose_input_config(
        "Interface",
        config(8, 48_000, cpal::SampleFormat::F32),
        ranges,
        &options,
    )
    .unwrap();
    assert_eq!(chosen.channels, 12);
    assert_eq!(chosen.sample_rate, 96_000);
}

/// Channel count never beats a known compatible output buffer size.
#[test]
fn input_config_prefers_a_compatible_buffer() {
    let options = AudioOptions {
        sample_rate: Some(48_000),
        buffer_size: Some(256),
        ..AudioOptions::default()
    };
    let ranges = || {
        Ok(vec![
            cpal::SupportedStreamConfigRange::new(
                8,
                48_000,
                48_000,
                cpal::SupportedBufferSize::Range { min: 512, max: 1024 },
                cpal::SampleFormat::F32,
            ),
            cpal::SupportedStreamConfigRange::new(
                4,
                48_000,
                48_000,
                cpal::SupportedBufferSize::Range { min: 64, max: 512 },
                cpal::SampleFormat::F32,
            ),
        ])
    };
    let default = config(2, 48_000, cpal::SampleFormat::F32);
    let chosen = choose_input_config("Interface", default, ranges, &options).unwrap();
    assert_eq!(chosen.channels, 4);
    assert_eq!(chosen.buffer_size, cpal::BufferSize::Fixed(256));
}

/// Enumeration failure keeps a usable default, but cannot invent one.
#[test]
fn input_config_falls_back_to_a_usable_default() {
    let options = AudioOptions {
        sample_rate: Some(48_000),
        ..AudioOptions::default()
    };
    let failed = || -> Result<Vec<cpal::SupportedStreamConfigRange>, HostError> {
        Err(HostError::Audio("listing failed".into()))
    };
    let chosen = choose_input_config(
        "Interface",
        config(2, 48_000, cpal::SampleFormat::F32),
        failed,
        &options,
    )
    .unwrap();
    assert_eq!(chosen.channels, 2);
    let failed = || -> Result<Vec<cpal::SupportedStreamConfigRange>, HostError> {
        Err(HostError::Audio("listing failed".into()))
    };
    assert!(matches!(
        choose_input_config(
            "Interface",
            config(2, 44_100, cpal::SampleFormat::F32),
            failed,
            &options
        ),
        Err(HostError::Audio(_))
    ));
}

/// Other sample formats and rates cannot satisfy the float input request.
#[test]
fn input_config_rejects_an_unsupported_rate() {
    let options = AudioOptions {
        sample_rate: Some(96_000),
        ..AudioOptions::default()
    };
    let ranges = || {
        Ok(vec![
            range(8, 44_100, 96_000, cpal::SampleFormat::I16),
            range(8, 44_100, 48_000, cpal::SampleFormat::F32),
        ])
    };
    assert!(matches!(
        choose_input_config(
            "Interface",
            config(2, 48_000, cpal::SampleFormat::F32),
            ranges,
            &options
        ),
        Err(HostError::UnsupportedSampleRate { rate: 96_000, .. })
    ));
}

/// The callback preserves channels in order and ignores channels after eight.
#[test]
fn interleaved_input_is_packed_into_frames() {
    let (writer, mut reader) = input_ring(2, 8);
    let mut callback = input_callback(writer, 9);
    callback(&[
        1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0,
    ]);
    let mut frames = [[0.0; 8]; 2];
    assert!(reader.read_block(&mut frames));
    assert_eq!(frames[0], [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    assert_eq!(frames[1], [10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0]);
}
