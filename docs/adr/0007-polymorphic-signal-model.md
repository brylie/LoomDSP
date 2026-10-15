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
| Audio | Red | Bipolar, nominal -1.0 to 1.0 | Not clipped on cables. Clipping happens only at the final output. |
| Modulation | Blue | Bipolar -1.0 to 1.0, or unipolar 0.0 to 1.0 | Envelopes are unipolar. LFOs are bipolar. |
| Pitch | Orange | 0.1 per octave. 0.0 = middle C (MIDI note 60) | Same as 1 V per octave under the 1.0 = 10 V model. -1.0 to 1.0 covers 20 octaves. |
| Phase | Purple | 0.0 to 1.0, wraps | Inputs wrap any value into range. |
| Gate | Yellow | 0.0 or 1.0 | Any input reads values above 0.5 as high. |

Why these ranges work together:
- An LFO (bipolar, ±1.0) into a pitch input gives ±10 octaves at full depth. The per-parameter modulation depth (FR-MOD-01) scales it to a useful amount without extra modules.
- A phase ramp into an audio output is a saw wave. A gate into a filter cutoff is a step. These "wrong" connections make sound, which is the point.
- Pitch at 0.1 per octave lets one signal hold both deep bass and high harmonics inside the same normalised range as everything else.

Ports also differ by shape, not only colour, for users with colour vision deficiency (NFR-UX-03).

### Channel count inference

- Each output declares its channel layout: `Mono`, `Stereo`, or `FollowsInput(port)`.
- When the graph is built (on the control thread), layouts propagate along cables.
- Each input declares how it handles stereo arriving at a mono input: `Sum` (mix to mono) or `PerChannel` (the engine runs the module once per channel, with separate state).
- Most DSP modules (filters, distortion, delays) default to `PerChannel`. Most modulation inputs default to `Sum`.

### Polyphony

Voice count is a separate dimension from channel count (FR-SIG-08). A cable can be polyphonic and stereo. The engine lays out data so voices sit next to each other in memory, which suits SIMD (NFR-PERF-03).

## Consequences

- Users never insert conversion modules. Patches stay small.
- CPU goes to stereo only where stereo exists.
- Module authors must declare channel behaviour for each port. The SDK provides sensible defaults.
- These conventions become part of the patch format. Changing them later needs a migration. They should be confirmed before the end of P1.
