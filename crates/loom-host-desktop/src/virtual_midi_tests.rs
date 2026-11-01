use super::*;
use loom_core::{EngineConfig, OutputSafety, create_engine};

/// A note played on a virtual controller reaches the engine through
/// `connect_midi_named`, and closing the connection releases it.
#[test]
#[ignore = "needs virtual MIDI ports (macOS, or Linux with ALSA)"]
fn closing_a_real_input_releases_its_held_notes() {
    let config = EngineConfig {
        output_safety: OutputSafety::Off,
        ..EngineConfig::with_sample_rate(48_000.0)
    };
    let mut parts = create_engine(loom_modules::registry(), config);
    let c = &mut parts.controller;
    let note = c.add_module("loom.midi.note").unwrap();
    let out = c.add_module("loom.io.audio-out").unwrap();
    c.connect_named(note, "gate", out, "in").unwrap();
    c.commit().unwrap();
    let mut gate = || {
        let (mut left, mut right) = (vec![0.0; 256], vec![0.0; 256]);
        parts.engine.process(&mut left, &mut right);
        left[255]
    };
    let mut wait_for = |value: f32| {
        (0..200).any(|_| {
            std::thread::sleep(Duration::from_millis(5));
            gate() == value
        })
    };

    // Without a MIDI service (a container with no ALSA sequencer), skip.
    let mut keys = match VirtualController::create(&format!("Loom test {}", std::process::id())) {
        Ok(keys) => keys,
        Err(e) if e.is_missing_hardware() => return eprintln!("skipped: {e} (docs/dev/test-devices.md)"),
        Err(e) => panic!("{e}"),
    };
    let name = keys.input_name().unwrap();
    let input = connect_midi_named(&name, share_midi(parts.midi)).unwrap();
    assert_eq!(input.port_name(), name);
    keys.note_on(0, 60, 100).unwrap();
    assert!(wait_for(1.0), "the note plays");

    input.close();
    assert!(wait_for(0.0), "closing the input releases the held note");
    assert!(keys.holds_notes(), "while the key is still held");
}
