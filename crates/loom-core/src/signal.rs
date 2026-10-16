//! Signal types and value conventions (ADR-0007).
//!
//! Every cable carries `f32` samples at the engine sample rate. The type of a
//! port is metadata: it sets colour and default ranges, but never blocks a
//! connection. A useful mental model for Eurorack users: 1.0 equals 10 volts.

/// The purpose of a port. Used for colour and defaults, never to block a cable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignalType {
    /// Bipolar audio, nominally -1.0 to 1.0. Not clipped on cables.
    Audio,
    /// Modulation. Bipolar (-1.0 to 1.0) or unipolar (0.0 to 1.0).
    Modulation,
    /// Pitch at 0.1 per octave, 0.0 = middle C (MIDI note 60).
    Pitch,
    /// A ramp from 0.0 to 1.0 that wraps.
    Phase,
    /// On/off. Values above 0.5 are high.
    Gate,
}

impl SignalType {
    /// Short lowercase name, for display and file formats.
    pub fn name(self) -> &'static str {
        match self {
            SignalType::Audio => "audio",
            SignalType::Modulation => "modulation",
            SignalType::Pitch => "pitch",
            SignalType::Phase => "phase",
            SignalType::Gate => "gate",
        }
    }

    /// Suggested cable colour as RGB. The UI may override this with a theme.
    pub fn colour(self) -> [u8; 3] {
        match self {
            SignalType::Audio => [0xE0, 0x4F, 0x4F],
            SignalType::Modulation => [0x4F, 0x8F, 0xE0],
            SignalType::Pitch => [0xF0, 0x9A, 0x3E],
            SignalType::Phase => [0x9B, 0x6B, 0xE0],
            SignalType::Gate => [0xE8, 0xD2, 0x4A],
        }
    }
}

/// Gate threshold. Any value above this counts as "high".
pub const GATE_THRESHOLD: f32 = 0.5;

/// Returns true when a gate value is high.
#[inline]
pub fn gate_is_high(value: f32) -> bool {
    value > GATE_THRESHOLD
}

/// The value of a trigger output for one frame: 1.0 on the frame the
/// trigger fires, 0.0 on every other frame (ADR-0007).
///
/// Why one sample: a counter must never miss a trigger. A longer pulse would
/// merge two triggers that come close together.
#[inline]
pub fn trigger(fired: bool) -> f32 {
    if fired { 1.0 } else { 0.0 }
}

/// Finds triggers on an input: rising edges, where a high frame (above 0.5)
/// follows a low one (ADR-0007). Any gate works as a trigger this way.
///
/// Keep one in the module and call [`EdgeDetector::rising`] once per frame,
/// in order. The state carries over between calls to `process`, so the
/// result does not depend on block size.
///
/// A new detector counts as low, so an input that is high from the first
/// frame fires on that frame. There is no hysteresis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EdgeDetector {
    was_high: bool,
}

impl EdgeDetector {
    /// A detector whose previous frame counts as low.
    pub const fn new() -> Self {
        Self { was_high: false }
    }

    /// Takes the next frame's value. True when it is a rising edge.
    #[inline]
    pub fn rising(&mut self, value: f32) -> bool {
        let high = gate_is_high(value);
        let rising = high && !self.was_high;
        self.was_high = high;
        rising
    }

    /// Forgets the previous frame, as in a new detector. Call it from
    /// `Module::reset`.
    #[inline]
    pub fn reset(&mut self) {
        self.was_high = false;
    }
}

/// Finds the frames where a phase wraps: where it jumps by more than half a
/// cycle from one frame to the next, in either direction. This is the shared
/// basis for every trigger that marks the end of a cycle, so all of them
/// agree on where a cycle ends.
///
/// Why more than half a cycle: a phase running at any rate below half the
/// sample rate moves less than 0.5 per frame, so only a wrap can jump that
/// far. A phase running forward wraps from near 1 down to near 0; one
/// running backward wraps from near 0 up to near 1. Both are a new cycle.
///
/// The wrap is reported on the first frame of the new cycle. That is also
/// the end of the old cycle, so an "end of cycle" trigger fires there too.
///
/// A new detector has no previous frame. Its first frame counts as a wrap
/// only when the phase is exactly 0, so a phase that starts on a bar line
/// fires, and one joined halfway through a cycle does not.
///
/// `docs/learn/phase-and-cycles.md` explains the rule with examples, and why
/// simpler rules ("the phase equals 0", "the phase went down") fail.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WrapDetector {
    previous: Option<f32>,
}

impl WrapDetector {
    /// A detector with no previous frame.
    pub const fn new() -> Self {
        Self { previous: None }
    }

    /// Takes the next frame's phase. True when the phase has wrapped.
    #[inline]
    pub fn wrapped(&mut self, phase: f32) -> bool {
        let wrapped = match self.previous {
            Some(previous) => (phase - previous).abs() > 0.5,
            None => phase == 0.0,
        };
        self.previous = Some(phase);
        wrapped
    }

    /// Forgets the previous frame, as in a new detector. Call it from
    /// `Module::reset`.
    #[inline]
    pub fn reset(&mut self) {
        self.previous = None;
    }
}

/// Pitch conventions: 0.1 per octave, 0.0 = middle C.
///
/// Why 0.1 per octave: it keeps 20 octaves inside the normalised -1.0 to 1.0
/// range, so pitch can modulate anything and anything can modulate pitch.
/// It is also 1 V/octave under the "1.0 = 10 V" model.
pub mod pitch {
    /// Frequency of pitch 0.0 (middle C, MIDI note 60) in Hz.
    pub const REFERENCE_HZ: f32 = 261.625_58;
    /// MIDI note number of pitch 0.0.
    pub const REFERENCE_NOTE: f32 = 60.0;
    /// Pitch units per octave.
    pub const PER_OCTAVE: f32 = 0.1;
    /// Pitch units per semitone.
    pub const PER_SEMITONE: f32 = PER_OCTAVE / 12.0;

    /// Converts a pitch value to Hz.
    #[inline]
    pub fn to_hz(pitch: f32) -> f32 {
        REFERENCE_HZ * (pitch / PER_OCTAVE).exp2()
    }

    /// Converts Hz to a pitch value.
    #[inline]
    pub fn from_hz(hz: f32) -> f32 {
        (hz / REFERENCE_HZ).log2() * PER_OCTAVE
    }

    /// Converts a MIDI note number to a pitch value.
    #[inline]
    pub fn from_midi_note(note: f32) -> f32 {
        (note - REFERENCE_NOTE) * PER_SEMITONE
    }

    /// Converts a pitch value to a (fractional) MIDI note number.
    #[inline]
    pub fn to_midi_note(pitch: f32) -> f32 {
        pitch / PER_SEMITONE + REFERENCE_NOTE
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn reference_points() {
            assert!((to_hz(0.0) - REFERENCE_HZ).abs() < 1e-3);
            assert!((to_hz(0.1) - 2.0 * REFERENCE_HZ).abs() < 1e-2);
            // A4 = MIDI 69 = 440 Hz.
            assert!((to_hz(from_midi_note(69.0)) - 440.0).abs() < 1e-2);
            assert!((to_midi_note(from_hz(440.0)) - 69.0).abs() < 1e-3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges(detector: &mut EdgeDetector, input: &[f32]) -> Vec<bool> {
        input.iter().map(|&value| detector.rising(value)).collect()
    }

    #[test]
    fn rising_edges_fire_once() {
        let input = [0.0, 1.0, 1.0, 0.0, 0.7, 0.5, 0.51];
        let found = edges(&mut EdgeDetector::new(), &input);
        assert_eq!(found, [false, true, false, false, true, false, true]);
    }

    #[test]
    fn high_on_the_first_frame_fires() {
        assert_eq!(edges(&mut EdgeDetector::new(), &[1.0, 1.0]), [true, false]);
    }

    #[test]
    fn one_sample_triggers_need_a_low_frame_between_them() {
        let separate = [trigger(true), trigger(false), trigger(true)];
        assert_eq!(edges(&mut EdgeDetector::new(), &separate), [true, false, true]);
        let merged = [trigger(true), trigger(true)];
        assert_eq!(edges(&mut EdgeDetector::new(), &merged), [true, false]);
    }

    #[test]
    fn edges_do_not_depend_on_how_frames_are_split() {
        let input: Vec<f32> = (0..100).map(|i| if i % 7 < 3 { 1.0 } else { 0.0 }).collect();
        let whole = edges(&mut EdgeDetector::new(), &input);
        for block in [1, 2, 5, 64] {
            let mut detector = EdgeDetector::new();
            let split: Vec<bool> = input
                .chunks(block)
                .flat_map(|chunk| edges(&mut detector, chunk))
                .collect();
            assert_eq!(split, whole, "block size {block}");
        }
    }

    #[test]
    fn reset_counts_as_low_again() {
        let mut detector = EdgeDetector::new();
        assert!(detector.rising(1.0));
        detector.reset();
        assert!(detector.rising(1.0));
    }

    #[test]
    fn nan_is_never_high() {
        let mut detector = EdgeDetector::new();
        assert!(!detector.rising(f32::NAN));
        assert!(detector.rising(1.0));
    }

    #[test]
    fn trigger_values() {
        assert_eq!(trigger(true), 1.0);
        assert_eq!(trigger(false), 0.0);
        assert!(gate_is_high(trigger(true)));
    }

    fn wraps(detector: &mut WrapDetector, input: &[f32]) -> Vec<bool> {
        input.iter().map(|&phase| detector.wrapped(phase)).collect()
    }

    #[test]
    fn forward_wraps_fire_on_the_first_frame_of_the_new_cycle() {
        let input = [0.25, 0.5, 0.75, 0.0, 0.25, 0.5, 0.75, 0.0];
        let found = wraps(&mut WrapDetector::new(), &input);
        assert_eq!(found, [false, false, false, true, false, false, false, true]);
    }

    #[test]
    fn backward_wraps_fire_too() {
        let input = [0.5, 0.25, 0.0, 0.75, 0.5];
        let found = wraps(&mut WrapDetector::new(), &input);
        assert_eq!(found, [false, false, false, true, false]);
    }

    #[test]
    fn first_frame_fires_only_on_exactly_zero() {
        assert_eq!(wraps(&mut WrapDetector::new(), &[0.0, 0.1]), [true, false]);
        assert_eq!(wraps(&mut WrapDetector::new(), &[0.37, 0.4]), [false, false]);
        assert_eq!(wraps(&mut WrapDetector::new(), &[0.999, 0.0]), [false, true]);
    }

    #[test]
    fn small_steps_and_wobble_do_not_fire() {
        let input = [0.4, 0.6, 0.55, 0.9, 0.45];
        let found = wraps(&mut WrapDetector::new(), &input);
        assert_eq!(found, [false; 5]);
    }

    #[test]
    fn wraps_do_not_depend_on_how_frames_are_split() {
        let input: Vec<f32> = (0..200).map(|i| (i as f32 * 0.07).fract()).collect();
        let whole = wraps(&mut WrapDetector::new(), &input);
        for block in [1, 2, 5, 64] {
            let mut detector = WrapDetector::new();
            let split: Vec<bool> = input
                .chunks(block)
                .flat_map(|chunk| wraps(&mut detector, chunk))
                .collect();
            assert_eq!(split, whole, "block size {block}");
        }
    }

    #[test]
    fn wrap_reset_forgets_the_previous_frame() {
        let mut detector = WrapDetector::new();
        assert!(detector.wrapped(0.0));
        assert!(!detector.wrapped(0.0));
        detector.reset();
        assert!(detector.wrapped(0.0));
    }

    #[test]
    fn nan_never_wraps() {
        let mut detector = WrapDetector::new();
        assert!(!detector.wrapped(0.9));
        assert!(!detector.wrapped(f32::NAN));
        assert!(!detector.wrapped(0.1));
        assert!(detector.wrapped(0.95));
    }
}
