//! The integration tests of loom-modules, built as one test binary.
//!
//! Each file in this folder is a module here. One binary links once,
//! where one binary per file linked the whole crate graph again for
//! each file. `no_alloc.rs` and `polyphony.rs` stay binaries of their
//! own because they install a global allocator that refuses to
//! allocate, and an allocator applies to its whole binary.

mod common;

mod audio_input;
mod cable_fade;
mod copy_state;
mod dynamics_eq_crossover;
mod effect_chain;
mod engine;
mod extreme;
mod first_values;
mod hot_patch;
mod live_changes;
mod midi_inputs;
mod midi_learn;
mod modes;
mod modulated_choices;
mod modulation;
mod non_finite;
mod normals;
mod output_safety;
mod own_phase;
mod pairs;
mod panels;
mod panic;
mod param_glide;
mod patches;
mod polyphonic_midi;
mod ranges;
mod reference;
mod reset;
mod selector_stereo;
mod slots;
mod stereo_tools;
mod subpatches;
mod synth_voice;
mod transport;
mod voice_assign;
mod voice_merge;
mod voice_split;
mod voice_spread;
