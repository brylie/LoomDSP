# ADR-0042: Unpatched phase inputs are wired for the player

- **Status:** Accepted
- **Date:** 2026-11-09
- **Related requirements:** FR-LIB-01, FR-LIB-02, FR-LIB-07
- **Specification:** [Oscillators](../design/oscillators.md), [Clock and transport, Default inputs](../design/clock.md#default-inputs)
- **Refines:** [ADR-0015](0015-one-transport-per-patch.md). Its consequence "Default inputs are filled by the engine, so a module reads the same slice whether a cable is connected or not" now holds for engine-filled defaults only. See Decision.

## Context

Phase drives everything in LoomDSP: oscillators, LFOs, sequencers, clocks, and arrangement read a ramp from 0 to 1 and turn it into sound or events ([Phase and cycles](../learn/phase-and-cycles.md), ADR-0012). The project's model has three parts:

- **Phase sources** make a phase and need no input: the transport's bar and beat phase, Clock, and the Phase module.
- **Phase modifiers** bend a phase: shift, scale, reverse, wrap, reset.
- **Phase-driven modules** read a phase and do their job with it.

If every phase-driven module needed a phase cable, every patch would start with plumbing. Before this decision an oscillator read only a phase, so the smallest playable patch needed a Phase module in front of every oscillator, and an unpatched oscillator was silent. ADR-0015 already solved the same problem for synced modules: an unpatched phase input follows the transport. Oscillators need the same help, but their phase comes from their own pitch, not from the transport.

## Options considered

1. **Shapers read a phase only** (the design before this ADR). Pure, but every oscillator needs a Phase module patched in front, and an unpatched oscillator is silent. No other modular environment asks this of a beginner.
2. **Oscillators take a pitch only, and separate shaper modules read a phase.** Familiar, but it doubles the module list (a Saw and a Saw shaper), and the oscillator no longer shows the phase model at all.
3. **A normalled phase input.** With no cable, the module runs a phase it makes itself. A cable replaces it. (Chosen.)
4. **The module's own phase always runs, and a phase cable adds to it** as phase modulation. Useful for FM, but a bent or shared phase is no longer what the player patched: one Phase driving a Saw and a Sine keeps them in lock only if both pitches are held at 0. And patches with a Phase module in front would no longer sound the same.

## Decision

Option 3. **Every phase input has a phase when nothing is patched into it.** A cable always replaces that phase for the module. There are two kinds of default, and the choice between them follows where the phase comes from.

| The phase comes from | Default | Who fills it | What the module reads with no cable |
|---|---|---|---|
| Outside the module: the transport | `PortDefault::BarPhase`, `BeatPhase` | The engine, into a shared buffer | The transport's phase. The same slice as with a cable, so the module never checks. |
| The module's own pitch: an oscillator | `PortDefault::OwnPhase` | The module | Zeros. The module runs its own phase while `!io.is_connected(PHASE_IN)`. |

- **Engine-filled defaults** keep ADR-0015's promise: the module reads the same slice either way.
- **Module-filled defaults** cannot keep it, because the phase depends on the module's own pitch and knobs, and one shared buffer cannot hold every module's phase. So `OwnPhase` reads zeros, and the module branches on `io.is_connected`. The registry allows `OwnPhase` only on a phase input.
- **A cable replaces, it never adds.** Phase modulation that adds to the own phase is a separate input, added with the FM operator (MT-SRC-07).
- **Phase sources need no input.** Their settings live on the module, shown in the inspector. A phase source should not need a cable to run.

### Checklist for a module with an own phase

1. Declare the phase input with `default: PortDefault::OwnPhase` and `signal: SignalType::Phase`.
2. Run the shared own-phase code in `crates/loom-modules/src/own_phase.rs`. Do not write a second phasor: the shared one is bit-identical to a Phase module in front, which is what makes a patch with Phase in front and one without sound the same.
3. Hold the own phase while a cable is connected, and start it from 0 on `reset`.
4. Say in the phase input's description what happens with and without a cable.
5. The registry-wide test (`crates/loom-modules/tests/own_phase.rs`) checks every module that declares `OwnPhase`: with no cable it must play, and a cable must change what it plays. Nothing needs adding to it.

## Consequences

- A note into an oscillator plays with one cable, and existing patches with a Phase module in front sound the same, bit for bit.
- Help pages and hover text say "own phase" or "bar phase" for an unpatched input, so what the module does is never hidden.
- A module with an own phase must branch on `is_connected`. The registry-wide test catches a module that forgets.
- The Phase module stays useful as a source to share or bend. Whether it keeps its pitch input is a separate change to its own specification.
- ADR-0015's sentence about defaults read by the engine now covers engine-filled defaults only.

## Prior art

On a Eurorack module, a **normalled** input jack is wired inside the module to a default source, and plugging in a cable breaks that link. Option 3 is that convention applied to phase. Bitwig Grid made phase a signal type with modules that create, modify, and read it, and fills some unpatched inputs with the device's phase ([Grid modules](https://www.bitwig.com/userguide/latest/grid_modules/)). Its manual does not say whether a cable into an oscillator's phase input replaces or adds to the oscillator's phase; one oscillator's phase input has a 0 to 800 % amount, which suggests it adds. LoomDSP chose replace for the reasons under option 4, and keeps adding for a later phase modulation input. We take the ideas, not the designs ([ADR-0018](0018-clean-room-module-development.md)).
