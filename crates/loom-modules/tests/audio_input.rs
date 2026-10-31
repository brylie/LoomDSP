//! Audio In reads the input ring once for all modules (audio-input.md).

use loom_core::{EngineConfig, ProbeKind, audio_input::input_ring, create_engine};

/// Two input modules see the same frames; mono devices duplicate channel one.
#[test]
fn input_modules_share_frames_and_duplicate_mono() {
    for block in [1, 77, 512] {
        for channels in [1, 2] {
            let mut parts = create_engine(
                loom_modules::registry(),
                EngineConfig {
                    max_block: 512,
                    ..EngineConfig::default()
                },
            );
            let (mut writer, reader) = input_ring(1024, channels);
            for n in 0..1024 {
                let mut frame = [0.0; 8];
                frame[0] = n as f32 / 1024.0;
                frame[1] = -(n as f32) / 1024.0;
                assert!(writer.push(frame));
            }
            let c = &mut parts.controller;
            c.set_audio_input(Some(reader)).unwrap();
            let first = c.add_module("loom.io.audio-in").unwrap();
            let second = c.add_module("loom.io.audio-in").unwrap();
            let a = c.add_module("loom.mix.split").unwrap();
            let b = c.add_module("loom.mix.split").unwrap();
            c.connect_named(first, "out", a, "in").unwrap();
            c.connect_named(second, "out", b, "in").unwrap();
            let outputs = [(a, "left"), (a, "right"), (b, "left"), (b, "right")];
            let probes: Vec<_> = outputs
                .iter()
                .map(|&(module, name)| {
                    let port = c.output_ref(module, name).unwrap();
                    c.add_probe(port, ProbeKind::Scope).unwrap()
                })
                .collect();
            c.commit().unwrap();
            parts.engine.process(&mut vec![0.0; block], &mut vec![0.0; block]);
            let mut samples = vec![Vec::new(); 4];
            for chunk in parts.telemetry.scope() {
                let index = probes.iter().position(|&id| id == chunk.probe).unwrap();
                samples[index].extend_from_slice(chunk.samples());
            }
            assert!(samples.iter().all(|channel| channel.len() == block));
            for (i, &actual) in samples[0].iter().enumerate() {
                let value = i as f32 / 1024.0;
                assert_eq!(actual, value);
                assert_eq!(samples[2][i], value);
                assert_eq!(samples[1][i], if channels == 1 { value } else { -value });
                assert_eq!(samples[3][i], samples[1][i]);
            }
        }
    }
}

/// A missing pair is silent and its activity light stays off.
#[test]
fn unavailable_pair_is_silent() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let (mut writer, reader) = input_ring(128, 2);
    for _ in 0..128 {
        assert!(writer.push([0.5; 8]));
    }
    let c = &mut parts.controller;
    c.set_audio_input(Some(reader)).unwrap();
    let input = c.add_module("loom.io.audio-in").unwrap();
    c.set_param_named(input, "pair", 2.0).unwrap();
    let port = c.output_ref(input, "out").unwrap();
    let probe = c.add_probe(port, ProbeKind::Meter).unwrap();
    c.commit().unwrap();
    parts.engine.process(&mut [0.0; 64], &mut [0.0; 64]);
    let reading = parts.telemetry.pop_meter().unwrap();
    assert_eq!(reading.probe, probe);
    assert_eq!((reading.peak, reading.right), (0.0, Some((0.0, 0.0))));
    assert_eq!(parts.controller.display(input, 0), Some(0.0));
}

/// Gain scales both sides, and muting fades to zero over five milliseconds.
#[test]
fn gain_and_mute_work_on_the_input() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let (mut writer, reader) = input_ring(1024, 2);
    for _ in 0..1024 {
        assert!(writer.push([0.25; 8]));
    }
    let c = &mut parts.controller;
    c.set_audio_input(Some(reader)).unwrap();
    let input = c.add_module("loom.io.audio-in").unwrap();
    c.set_param_named(input, "gain", 2.0).unwrap();
    let out = c.output_ref(input, "out").unwrap();
    c.add_probe(out, ProbeKind::Meter).unwrap();
    c.commit().unwrap();
    parts.engine.process(&mut [0.0; 64], &mut [0.0; 64]);
    let initial = parts.telemetry.pop_meter().unwrap();
    assert_eq!(initial.peak, 0.5);
    assert_eq!(initial.right.unwrap().0, 0.5);
    parts.controller.set_param_named(input, "mute", 1.0).unwrap();
    let mut peaks = Vec::new();
    for _ in 0..5 {
        parts.engine.process(&mut [0.0; 64], &mut [0.0; 64]);
        peaks.push(parts.telemetry.pop_meter().unwrap().peak);
    }
    assert!(peaks.windows(2).all(|pair| pair[0] >= pair[1]), "{peaks:?}");
    assert!(peaks[4] < 0.005, "{peaks:?}");
}

/// Recorded input, gain, and mute produce the same samples at every block size.
#[test]
fn recorded_input_gain_and_mute_are_block_invariant() {
    fn render(block: usize) -> (Vec<f32>, Vec<f32>) {
        let mut parts = create_engine(
            loom_modules::registry(),
            EngineConfig {
                output_safety: loom_core::OutputSafety::Off,
                ..EngineConfig::default()
            },
        );
        let (mut writer, reader) = input_ring(1024, 2);
        for n in 0..1024 {
            let mut frame = [0.0; 8];
            frame[0] = 0.1 + (n % 11) as f32 * 0.01;
            frame[1] = -0.2 + (n % 7) as f32 * 0.01;
            assert!(writer.push(frame));
        }
        let c = &mut parts.controller;
        c.set_audio_input(Some(reader)).unwrap();
        let input = c.add_module("loom.io.audio-in").unwrap();
        c.set_param_named(input, "gain", 2.0).unwrap();
        let out = c.add_module("loom.io.audio-out").unwrap();
        c.connect_named(input, "out", out, "in").unwrap();
        c.commit().unwrap();
        let mut left = vec![0.0; 960];
        let mut right = vec![0.0; 960];
        for (start, end, mute) in [(0, 320, 0.0), (320, 640, 1.0), (640, 960, 0.0)] {
            parts.controller.set_param_named(input, "mute", mute).unwrap();
            let mut pos = start;
            while pos < end {
                let next = (pos + block).min(end);
                parts.engine.process(&mut left[pos..next], &mut right[pos..next]);
                pos = next;
            }
        }
        (left, right)
    }
    let one = render(1);
    assert_eq!(one, render(77));
    assert_eq!(one, render(512));
    assert!(one.0[200] > 0.0 && one.1[200] < 0.0);
    assert!(one.0[639].abs() < 0.005 && one.1[639].abs() < 0.005);
    assert!(one.0[959] > 0.0 && one.1[959] < 0.0);
}

/// The activity light follows sound in the selected pair, even while muted.
#[test]
fn active_light_tracks_input_sound() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let (mut writer, reader) = input_ring(64, 2);
    for _ in 0..64 {
        assert!(writer.push([0.0; 8]));
    }
    let c = &mut parts.controller;
    c.set_audio_input(Some(reader)).unwrap();
    let input = c.add_module("loom.io.audio-in").unwrap();
    c.set_param_named(input, "mute", 1.0).unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(input, "out", out, "in").unwrap();
    c.commit().unwrap();
    parts.engine.process(&mut [0.0; 64], &mut [0.0; 64]);
    assert_eq!(parts.controller.display(input, 0), Some(0.0));
    for _ in 0..64 {
        let mut frame = [0.0; 8];
        frame[1] = 0.25;
        assert!(writer.push(frame));
    }
    parts.engine.process(&mut [0.0; 64], &mut [0.0; 64]);
    assert_eq!(parts.controller.display(input, 0), Some(1.0));
}

/// Each of four input pairs reads its two channels from an eight-channel device.
#[test]
fn every_input_pair_uses_its_own_channels() {
    let mut parts = create_engine(loom_modules::registry(), EngineConfig::default());
    let (mut writer, reader) = input_ring(64, 8);
    let frame = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8];
    for _ in 0..64 {
        assert!(writer.push(frame));
    }
    let controller = &mut parts.controller;
    controller.set_audio_input(Some(reader)).unwrap();
    let mut probes = Vec::new();
    for pair in 1..=4 {
        let input = controller.add_module("loom.io.audio-in").unwrap();
        let split = controller.add_module("loom.mix.split").unwrap();
        controller.set_param_named(input, "pair", pair as f32).unwrap();
        controller.connect_named(input, "out", split, "in").unwrap();
        for name in ["left", "right"] {
            let output = controller.output_ref(split, name).unwrap();
            probes.push(controller.add_probe(output, ProbeKind::Scope).unwrap());
        }
    }
    controller.commit().unwrap();
    parts.engine.process(&mut [0.0; 64], &mut [0.0; 64]);
    let mut seen = [0; 8];
    for chunk in parts.telemetry.scope() {
        let channel = probes.iter().position(|&probe| probe == chunk.probe).unwrap();
        assert!(chunk.samples().iter().all(|&value| value == frame[channel]));
        seen[channel] += chunk.len as usize;
    }
    assert_eq!(seen, [64; 8]);
}
