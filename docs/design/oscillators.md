# Oscillators Specification

How the oscillators (Sine, Saw, Pulse, Triangle) take a pitch, and how a phase cable takes over from it.

- **Kind:** living specification.
- **Status:** Implemented (2026-11-09).
- **Decision behind it:** [ADR-0042](../adr/0042-unpatched-phase-inputs-are-wired-for-the-player.md) (unpatched phase inputs are wired for the player). It builds on [ADR-0007](../adr/0007-polymorphic-signal-model.md) (pitch and phase signals) and [ADR-0017](../adr/0017-primitives-traits-and-abstractions.md) (one Oscillator type, built as several modules).
- **Use case and requirements:** UC1, FR-LIB-01, FR-LIB-02. Catalogue entry [MT-SRC-01](../product/module-catalogue.md).
- **Background:** [Phase and cycles](../learn/phase-and-cycles.md) explains what a phase is. [Pitch at 0.1 per octave](../learn/pitch.md) explains the pitch scale.
- **Prior art:** a Eurorack VCO takes a 1 V per octave input and has its own tuning knobs. The phase input is a **normalled** jack, in Eurorack terms: wired inside to a default source (the own phase), which a cable replaces. Bitwig Grid makes phase a signal type of its own; its manual does not say whether a phase cable replaces or adds to an oscillator's phase (ADR-0042, Prior art). We take the conventions, not their designs.
- **Issues:** hard sync and a sub output are [#547](https://github.com/brylie/LoomDSP/issues/547).
- **Code:** `crates/loom-modules/src/osc.rs`; the shared ports, knobs, `PitchedPhase`, and `split` in `crates/loom-modules/src/own_phase.rs`; `PortDefault::OwnPhase` in `crates/loom-core/src/descriptor/port.rs`. A new module with an own phase follows the checklist in ADR-0042.

## Goal

> I patch a note's pitch into a Saw and hear it at once. Later I patch one Phase into a Saw and a Sine, so they stay in lock, and bend that phase to get a new wave.

Phase drives everything in LoomDSP, but a player should not have to patch it by hand to hear an oscillator. So the phase is wired for them: with no cable, an oscillator runs its own phase. This is the same idea as an input that follows the transport with no cable ([clock](clock.md#default-inputs)).

## Behaviour

Sine, Saw, Pulse, and Triangle all have these ports and knobs.

| | Name | Signal | With no cable | What it does |
|---|---|---|---|---|
| Input | `pitch` | pitch | 0, middle C | Sets the frequency, at 0.1 per octave. |
| Input | `phase` | phase | own phase | Position in the wave, from 0 to 1. A cable replaces the own phase. |
| Output | `out` | audio | | The wave, from -1 to 1. |

| Knob | Range | Default | What it does |
|---|---|---|---|
| `tune` | -48 to 48 semitones | 0 | Shifts the pitch. Fractions of a semitone fine-tune it. |
| `octave` | -10 to 10 octaves | 0 | Shifts the pitch in whole octaves. |

Pulse also keeps its `width` knob. Its panel shows Tune, Octave, and Width, in that order. The others show Tune and Octave.

| What is connected | The wave follows |
|---|---|
| Nothing | Its own phase, at middle C plus the knobs. |
| `pitch` only | Its own phase, at the pitch plus the knobs. |
| `phase` | The cable. `pitch`, `tune`, and `octave` do nothing. |

## Rules

1. **With no phase cable, an oscillator runs its own phase.** The phase rises from 0 to 1 at the frequency of `pitch + tune / 120 + octave / 10`, exactly as a Phase module with the same knobs would make it. *Why:* one cable from a note should make a sound, as on any synthesizer. Making it the same as a Phase module, bit for bit, means a patch can drop a Phase module that only fed one oscillator and sound the same.
2. **A phase cable replaces the own phase,** like a normalled jack. The wave reads the cable, and `pitch`, `tune`, and `octave` are ignored. A phase **modulation** input, which adds to the own phase as many Eurorack VCOs' PM inputs do, is a separate input that comes later (Out of scope). *Why:* phase drives everything, and any phase must be able to drive any shape: one Phase for several shapes in lock, a phase from Phase Shift or Phase Scale to bend the wave, or a slow ramp to read the wave as a table. Adding the knobs to a cable's phase would give two ways to tune one oscillator, and a bent phase would no longer be the shape the player made.
3. **The own phase holds while a cable is connected.** When the cable is removed, the own phase continues from where it stopped. *Why:* it costs nothing while it is not heard, and a held phase gives the same result at any block size.
4. **The knobs are read every sample, with no smoothing.** `octave` rounds to a whole number. Modulation can move both knobs. *Why:* the same as Phase, so rule 1 holds. A pitch step is not a click: the phase does not jump, only its speed changes.
5. **A wild pitch cannot stop the oscillator.** A pitch so high that the frequency overflows runs the phase at its largest step, just under half the sample rate, and a pitch that is not a number gives the same. The wave plays again as soon as the pitch is normal ([#121](https://github.com/brylie/LoomDSP/issues/121)). *Why:* a feedback patch can send any value into a pitch, and the sound must come back.
6. **A reset starts the own phase from 0.** The controller resets a new module once. *Why:* the same patch gives the same output, bit for bit.
7. **The band limit works the same from either phase.** Saw, Pulse, and Triangle smooth their edges from the step between this phase and the last one ([DSP algorithms](dsp-algorithms.md)). *Why:* one way to find the step means an own phase and a cable sound the same at the same speed.
8. **The phase input's default is "own phase".** It is a `PortDefault` of its own, so help pages and the inspector say "own phase", not "0". The engine reads it as 0, and the module uses its own phase. The registry refuses it on any input that is not a phase. *Why:* the help text must say what really happens, and the default only means something for a phase.
9. **The Sound source role has an optional `pitch` input and optional `tune` and `octave` knobs.** *Why:* a slot that swaps a Saw for a Pulse keeps the cable into `pitch`, and the tuning carries across by name ([sub-patches](sub-patches.md)). Noise fills the role too, with none of them.

## Acceptance criteria

- [x] Given no phase cable, when an oscillator plays a gliding pitch with tune 7 and octave -1, then its output is the same, bit for bit, as a Phase module with those knobs in front of it. Checked for all four (`osc/own_phase_tests.rs`).
- [x] Given nothing connected, a Saw plays middle C, 261.63 Hz.
- [x] Given a phase cable, changing `pitch`, `tune`, and `octave` does not change the output.
- [x] Given a cable connected and then removed, the own phase continues from where it stopped.
- [x] `octave` 1.4 plays the same as `octave` 1.
- [x] A pitch of 50, then NaN, gives finite output, and the wave plays again afterwards. The extreme-input test checks every oscillator's `pitch` stays bounded, and has a stated reason why it cannot match a clean render: the wave comes back at another point in its cycle.
- [x] A reset gives the same render again.
- [x] Reference renders of all four with only `pitch` connected match at block sizes 1, 77, and 512 (`tests/reference/*_at_pitch.wav`).
- [x] The registry refuses `PortDefault::OwnPhase` on an input that is not a phase.
- [x] The help pages show `own phase` as the phase input's default, and Tune and Octave on each oscillator.
- [x] Every example patch where a Phase module fed only one oscillator now patches the pitch into the oscillator, and its reference render does not change.
- [x] Every registered module that declares an own phase moves with no cable and plays differently with a cable of zeros (`tests/own_phase.rs`), so a module that forgets the `is_connected` branch fails.

## Out of scope

- **Hard sync and a sub output.** [#547](https://github.com/brylie/LoomDSP/issues/547). Both work on the own phase, so they build on this.
- **A frequency or phase modulation input** that adds to the own phase. The catalogue lists it for MT-SRC-01. It needs its own rules for depth and units, and the FM operator abstraction (MT-SRC-07) is where it will be used.
- **A phase output** on the oscillator. Phase gives one when a patch needs to share a phase.
- **A mark on the port for own phase.** Inputs that follow the transport show a clock mark, because they follow something outside the module ([panels](panels.md#clock-mark-on-transport-inputs)). An oscillator making its own phase is what an oscillator is expected to do, so the hover text is enough.
- **Smoothing on `tune` and `octave`.** Rule 4 says why it is not needed.

### Phase defaults in a bundle

A phase cable narrower than the oscillator's voice count supplies only its own
voices. Later voices read `OwnPhase`, as the bundle padding rule requires.
`PortIo::uses_default` distinguishes this case from a cable supplying a phase
value. `is_connected` still reports whether a cable exists. The shared own-phase
helper uses `uses_default`, so an unpatched voice and a padded voice both run
their own phase, while a real cable of zeros still holds its phase at zero.
