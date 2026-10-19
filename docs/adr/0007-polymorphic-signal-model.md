# ADR-0007: Polymorphic signal model

- **Status:** Proposed
- **Date:** 2026-10-13
- **Related requirements:** FR-SIG-01 to 08, FR-MOD-01

## Context

We want "patch anything to anything", as in Eurorack and Bitwig Grid. Every output should be able to drive every input with a musically useful result and no conversion modules. At the same time, users need to read a patch quickly, so signals should look different by purpose.

We also want to avoid computing stereo on every cable. Most modulation is mono.

## Decision

### One sample format

Every cable carries `f32` samples at the engine sample rate. There is no control rate. Types are metadata for colour, default ranges, and module behaviour. They never block a connection.

### Value conventions

A useful mental model for Eurorack users: **1.0 equals 10 volts.**

| Type | Colour (proposal) | Range | Notes |
|---|---|---|---|
| Audio | Red | Bipolar, nominal -1.0 to 1.0 | Not clipped on cables. Clipping happens only at the final output, after a safety stage ([ADR-0019](0019-output-safety-stage.md)). |
| Modulation | Blue | Bipolar -1.0 to 1.0, or unipolar 0.0 to 1.0 | Envelopes are unipolar. LFOs are bipolar. |
| Pitch | Orange | 0.1 per octave. 0.0 = middle C (MIDI note 60) | Same as 1 V per octave under the 1.0 = 10 V model. -1.0 to 1.0 covers 20 octaves. |
| Phase | Purple | 0.0 to 1.0, wraps | Inputs wrap any value into range. |
| Gate | Yellow | 0.0 or 1.0 | Any input reads values above 0.5 as high. |

Why these ranges work together:
- An LFO (bipolar, ±1.0) into a pitch input gives ±10 octaves at full depth. The per-parameter modulation depth (FR-MOD-01) scales it to a useful amount without extra modules.
- A phase ramp into an audio output is a saw wave. A gate into a filter cutoff is a step. These "wrong" connections make sound, which is the point.
- Pitch at 0.1 per octave lets one signal hold both deep bass and high harmonics inside the same normalised range as everything else.

Ports also differ by shape, not only colour, for users with colour vision deficiency (NFR-UX-03).

### Triggers

A trigger is an event, such as "loop ended" or "bar 1". It travels on a gate cable, so it has no type of its own (OQ-19).

- **A trigger output is 1.0 for exactly one sample**, the frame of the event, and 0.0 on every other frame.
- **A trigger input counts rising edges:** a frame that is high (above 0.5) when the frame before it was low. So any gate also works as a trigger.
- **The first frame a module sees counts as following a low frame.** An input that is high from the start fires once, on frame 0. After `Module::reset`, the input counts as low again.
- **No hysteresis.** The 0.5 threshold is the same rule as for gates.

Options considered for what an output sends:

| Option | For | Against |
|---|---|---|
| One sample high (chosen) | Exact to the sample. Cheapest. The same at every sample rate. | Invisible on a scope unless the UI stretches it. Two triggers need at least one low frame between them. |
| A fixed short pulse, for example 1 ms | Visible, and familiar from Eurorack. | Triggers closer than the pulse merge into one, so counters miss events. Its length in samples changes with the sample rate. |

Why one sample: arrangement logic (ADR-0012) counts triggers, so a lost trigger is a wrong bar count, while an invisible one is only a display problem the UI can solve. Why the first frame counts: the transport's bar trigger fires on frame 0 of bar 1, and it must not be lost when a patch starts. Why no hysteresis: one threshold for every on/off input is easier to explain, and patches stay deterministic either way.

### Channel count inference

Channel count (mono or stereo) is inferred per cable when the graph is built, so stereo flows only where a stereo source exists. How it is inferred, how modules declare stereo behaviour, and what it costs are decided in [ADR-0020](0020-inferred-channel-count.md).

### Polyphony

Voice count is a separate dimension from channel count (FR-SIG-08). A cable can be polyphonic and stereo. The engine lays out data so voices sit next to each other in memory, which suits SIMD (NFR-PERF-03).

## Consequences

- Users never insert conversion modules. Patches stay small.
- CPU goes to stereo only where stereo exists.
- Module authors must declare channel behaviour for each port. The SDK provides sensible defaults.
- Module authors detect triggers with `loom_core::signal::EdgeDetector` and send them with `loom_core::signal::trigger`, so every module follows the same rules.
- Scopes and cable activity must stretch one-sample triggers so people can see them. The signal on the cable stays one sample long.
- These conventions become part of the patch format. Changing them later needs a migration. They should be confirmed before the end of P1.
