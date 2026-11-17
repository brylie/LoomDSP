# ADR-0047: Portable MIDI protocol code lives in `loom-midi`

- **Status:** Accepted
- **Date:** 2026-11-17
- **Related requirements:** FR-MIDI-07, FR-LIB-25, NFR-PORT-02
- **Related decisions:** ADR-0001 (control and audio threads), ADR-0015 (one transport per patch)
- **Specification:** [MIDI clock](../design/clock.md#midi-clock)
- **Issue:** [#77](https://github.com/brylie/LoomDSP/issues/77)

## Context

MIDI clock needs the same wire messages on input and output. Measuring the gap between pulses only needs timestamps. A caller can supply those timestamps from an audio render, a device callback, or a test.

## Options considered

1. Keep message encoding and pulse calculations inside `loom-core`. This adds no crate, but using those pieces elsewhere would also bring in the graph and engine.
2. Keep them in `loom-host-desktop`. This puts them near the devices, but the portable engine also needs them and cannot depend on the desktop host.
3. Put them in a small portable crate. This adds a workspace member and lets the compiler enforce the dependency boundary.

## Decision

Use option 3. `loom-midi` owns short MIDI message encoding and decoding, clock units, and pulse interval calculations. It has no dependencies and accepts time as data from its caller.

`loom-core` owns the selected input, the transport, and the frame numbers for outgoing messages. `loom-host-desktop` owns ports and maps those frame numbers to playback times on its MIDI worker.

## Consequences

The codec and pulse calculations can be reused and tested without opening devices or building a patch. A new patch or device change still goes through the engine and host that own it.
