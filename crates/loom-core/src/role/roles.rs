//! The initial roles (`docs/product/module-catalogue.md`, Roles).
//!
//! Sound source takes a phase, not a pitch, because every LoomDSP
//! oscillator is phase driven (ADR-0012): the Phase generator turns a pitch
//! into a phase that the oscillator reads. Voice takes the per-voice
//! signals of ADR-0038, a pitch, a gate, and a velocity, in place of a
//! "note in". Most core parameters are optional, because the built-in
//! modules that fill a role do not all have them.

use super::{RoleDescriptor, RoleParam, RolePort};
use crate::signal::SignalType;

/// A required port.
const fn port(name: &'static str, label: &'static str, signal: SignalType, description: &'static str) -> RolePort {
    RolePort {
        name,
        label,
        description,
        signal,
        required: true,
    }
}

/// An optional port.
const fn optional_port(
    name: &'static str,
    label: &'static str,
    signal: SignalType,
    description: &'static str,
) -> RolePort {
    RolePort {
        required: false,
        ..port(name, label, signal, description)
    }
}

/// A required continuous parameter.
const fn param(name: &'static str, label: &'static str, unit: &'static str, description: &'static str) -> RoleParam {
    RoleParam {
        name,
        label,
        description,
        unit,
        whole: false,
        required: true,
    }
}

/// An optional continuous parameter.
const fn optional(name: &'static str, label: &'static str, unit: &'static str, description: &'static str) -> RoleParam {
    RoleParam {
        required: false,
        ..param(name, label, unit, description)
    }
}

/// `loom.role.source`: something that makes a sound, at a pitch or from a phase.
pub static SOURCE: RoleDescriptor = RoleDescriptor {
    id: "loom.role.source",
    version: 1,
    name: "Sound source",
    description: "Makes a sound. An oscillator plays at its pitch, or follows a phase cable.",
    inputs: &[
        optional_port(
            "pitch",
            "Pitch",
            SignalType::Pitch,
            "Sets the frequency. 0 is middle C, and +0.1 is one octave up.",
        ),
        optional_port(
            "phase",
            "Phase",
            SignalType::Phase,
            "Position in the wave, from 0 to 1. Replaces the source's own phase.",
        ),
    ],
    outputs: &[port("out", "Out", SignalType::Audio, "The sound.")],
    params: &[
        optional("level", "Level", "", "How loud the sound is, from 0 to 1."),
        optional("tune", "Tune", "st", "Shifts the pitch in semitones."),
        optional("octave", "Octave", "oct", "Shifts the pitch in octaves."),
    ],
    thru: false,
};

/// `loom.role.filter`: shapes the spectrum of a signal around a cutoff.
pub static FILTER: RoleDescriptor = RoleDescriptor {
    id: "loom.role.filter",
    version: 1,
    name: "Filter",
    description: "Shapes the spectrum of a signal around a cutoff frequency.",
    inputs: &[
        port("in", "In", SignalType::Audio, "The audio to filter."),
        optional_port(
            "cutoff",
            "Cutoff",
            SignalType::Pitch,
            "Moves the cutoff as pitch: +0.1 doubles it.",
        ),
    ],
    outputs: &[port("out", "Out", SignalType::Audio, "The filtered audio.")],
    params: &[
        param(
            "cutoff",
            "Cutoff",
            "Hz",
            "The frequency where the filter starts to cut.",
        ),
        optional("resonance", "Resonance", "", "Boosts the frequencies near the cutoff."),
        optional("drive", "Drive", "", "Raises the input level so the filter saturates."),
    ],
    thru: true,
};

/// `loom.role.shaper`: distorts a signal.
pub static SHAPER: RoleDescriptor = RoleDescriptor {
    id: "loom.role.shaper",
    version: 1,
    name: "Shaper",
    description: "Distorts a signal.",
    inputs: &[port("in", "In", SignalType::Audio, "The audio to shape.")],
    outputs: &[port("out", "Out", SignalType::Audio, "The shaped audio.")],
    params: &[
        param("drive", "Drive", "", "How hard the signal is pushed into the shape."),
        optional(
            "mix",
            "Mix",
            "",
            "0 is the input alone, and 1 is the shaped signal alone.",
        ),
    ],
    thru: false,
};

/// `loom.role.envelope`: a level that follows a gate.
pub static ENVELOPE: RoleDescriptor = RoleDescriptor {
    id: "loom.role.envelope",
    version: 1,
    name: "Envelope",
    description: "Makes a level that rises when a gate goes high and falls when it goes low.",
    inputs: &[port(
        "gate",
        "Gate",
        SignalType::Gate,
        "Going high starts the envelope; going low releases it.",
    )],
    outputs: &[port(
        "out",
        "Out",
        SignalType::Modulation,
        "The envelope level, from 0 to 1.",
    )],
    params: &[
        param("attack", "Attack", "s", "Time to rise after the gate goes high."),
        param("release", "Release", "s", "Time to fall after the gate goes low."),
        optional(
            "decay",
            "Decay",
            "s",
            "Time to fall to the sustain level after the attack.",
        ),
        optional("sustain", "Sustain", "", "The level held while the gate stays high."),
    ],
    thru: false,
};

/// `loom.role.modulator`: a moving level with no input.
pub static MODULATOR: RoleDescriptor = RoleDescriptor {
    id: "loom.role.modulator",
    version: 1,
    name: "Modulator",
    description: "Makes a moving level to modulate parameters with.",
    inputs: &[],
    outputs: &[port("out", "Out", SignalType::Modulation, "The moving level.")],
    params: &[
        optional("rate", "Rate", "Hz", "How fast the level moves."),
        optional("amount", "Amount", "", "How far the level moves, from 0 to 1."),
    ],
    thru: false,
};

/// `loom.role.effect`: processes audio in to audio out.
pub static EFFECT: RoleDescriptor = RoleDescriptor {
    id: "loom.role.effect",
    version: 1,
    name: "Effect",
    description: "Processes audio.",
    inputs: &[port("in", "In", SignalType::Audio, "The audio to process.")],
    outputs: &[port("out", "Out", SignalType::Audio, "The processed audio.")],
    params: &[optional(
        "mix",
        "Mix",
        "",
        "0 is the input alone, and 1 is the processed signal alone.",
    )],
    thru: true,
};

/// `loom.role.voice`: plays a note.
pub static VOICE: RoleDescriptor = RoleDescriptor {
    id: "loom.role.voice",
    version: 1,
    name: "Voice",
    description: "Plays one note from a pitch, a gate, and a velocity.",
    inputs: &[
        port("pitch", "Pitch", SignalType::Pitch, "The note's pitch, 0.1 per octave."),
        port("gate", "Gate", SignalType::Gate, "High while the note plays."),
        optional_port(
            "velocity",
            "Velocity",
            SignalType::Modulation,
            "How hard the note was played, from 0 to 1.",
        ),
    ],
    outputs: &[port("out", "Out", SignalType::Audio, "The note's sound.")],
    params: &[optional("level", "Level", "", "How loud the voice is, from 0 to 1.")],
    thru: false,
};

/// Every initial role.
pub static ALL: &[&RoleDescriptor] = &[&SOURCE, &FILTER, &SHAPER, &ENVELOPE, &MODULATOR, &EFFECT, &VOICE];
