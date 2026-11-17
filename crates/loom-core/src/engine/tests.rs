use super::*;

/// A module's invalid tempo must not poison the clock when switching back to Internal.
#[test]
fn restoring_local_tempo_uses_the_effective_clamped_value() {
    let mut engine = crate::create_engine(Arc::new(crate::Registry::default()), EngineConfig::default()).engine;
    for (requested, expected) in [(f64::NAN, 120.0), (600.0, 300.0), (2.0, 20.0)] {
        engine.apply_local_transport(TransportRequest {
            tempo: Some(requested),
            ..TransportRequest::default()
        });
        assert_eq!(engine.transport.info(0).tempo(), expected);
        engine.select_midi_clock(Some(crate::MidiInputId(1)), false);
        engine.select_midi_clock(None, false);
        assert_eq!(engine.transport.info(0).tempo(), expected);
    }
}
