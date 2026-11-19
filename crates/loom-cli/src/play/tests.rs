use super::*;

/// Empty or truncated input does not stop the player. A complete line does.
#[test]
fn player_requires_a_completed_line_to_stop() {
    assert!(!stop_line(&b""[..]));
    assert!(!stop_line(&b"incomplete"[..]));
    assert!(stop_line(&b"\n"[..]));
    assert!(stop_line(&b"stop\r\n"[..]));
}

/// Delivers SIGTERM to the isolated child after its input has closed.
#[cfg(unix)]
#[test]
fn headless_player_honours_sigterm() {
    use std::io::BufReader;
    use std::process::{Command, Stdio};
    const CHILD: &str = "LOOM_TEST_TERMINATION_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let stop = stop_on_signal().unwrap();
        assert!(!stop_line(std::io::stdin().lock()));
        let parts = loom_core::create_engine(loom_modules::registry(), loom_core::EngineConfig::default());
        let audio = loom_host_desktop::AudioOutput::clock(48_000, 128)
            .start(parts.engine)
            .unwrap();
        let live = Live {
            stats: audio.stats().clone(),
            audio,
            _midi: Vec::new(),
            controller: parts.controller,
        };
        let stats = live.stats.clone();
        let timed_out = Arc::new(AtomicBool::new(false));
        let timeout = timed_out.clone();
        let stop_at_deadline = stop.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(5));
            if !stop_at_deadline.swap(true, Ordering::Relaxed) {
                timeout.store(true, Ordering::Relaxed);
            }
        });
        std::io::Write::write_all(&mut std::io::stdout(), b"!\n").unwrap();
        play_until_stop(live, &stop, false);
        assert!(!timed_out.load(Ordering::Relaxed));
        assert!(stats.snapshot().callbacks > 0);
        return;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "play::tests::headless_player_honours_sigterm", "--nocapture"])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.as_mut().unwrap();
    assert!(
        BufReader::new(stdout)
            .lines()
            .any(|line| line.is_ok_and(|line| line.contains('!')))
    );
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    assert!(child.wait().unwrap().success());
}

/// Runs the modulated synth and transport long enough to expose drift or stuck audio.
#[test]
#[ignore = "12 hours of offline audio. Run in release mode."]
fn modulated_patch_survives_a_twelve_hour_offline_soak() {
    use loom_core::{EngineConfig, MidiEvent, ModuleId, ProbeKind};
    use std::path::Path;
    use std::sync::atomic::Ordering;
    const RATE: usize = 8_000;
    const BLOCK: usize = 512;
    const BAR: usize = 2 * RATE;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/patches/modulation.ron");
    let (_, mut parts) = crate::load_engine(&path, EngineConfig::with_sample_rate(RATE as f32), None).unwrap();
    let bar = parts.controller.output_ref(ModuleId(10), "bar").unwrap();
    let probe = parts.controller.add_probe(bar, ProbeKind::Scope).unwrap();
    parts.controller.commit().unwrap();
    assert!(parts.midi.send(MidiEvent::note_on(0, 0, 60, 100)));
    let (mut left, mut right) = ([0.0; BLOCK], [0.0; BLOCK]);
    let (mut peak, mut total, mut pulses) = (0.0_f32, 0.0_f64, 0_usize);
    let mut previous = None::<f32>;
    let frames = 12 * 3600 * RATE;
    for start in (0..frames).step_by(BLOCK) {
        let count = BLOCK.min(frames - start);
        parts.engine.process(&mut left[..count], &mut right[..count]);
        for &sample in &left[..count] {
            assert!(sample.is_finite());
            peak = peak.max(sample.abs());
            total += f64::from(sample);
            if start > RATE
                && let Some(last) = previous
            {
                // Every post-start block of the held, modulated note changes.
                if sample != last {
                    previous = None;
                }
            }
        }
        if start > RATE {
            assert!(peak > 0.01, "lost the held note at frame {start}");
            assert!(previous.is_none(), "stuck audio at frame {start}");
        }
        previous = Some(left[count - 1]);
        peak = 0.0;
        for chunk in parts.telemetry.scope() {
            assert_eq!(chunk.probe, probe);
            for (offset, &value) in chunk.samples().iter().enumerate() {
                if value > 0.5 {
                    let at = chunk.frame as usize + offset;
                    assert_eq!(at, pulses * BAR, "bar drift at frame {at}");
                    pulses += 1;
                }
            }
        }
        parts.controller.collect_garbage();
    }
    assert_eq!(pulses, frames / BAR, "bar pulse count, including frame zero");
    assert!((total / frames as f64).abs() < 0.001);
    let stats = parts.controller.stats();
    assert_eq!(stats.output_silenced.load(Ordering::Relaxed), 0);
    assert_eq!(stats.leaked.load(Ordering::Relaxed), 0);
}
