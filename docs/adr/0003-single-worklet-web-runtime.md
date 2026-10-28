# ADR-0003: One WASM instance per patch in a single AudioWorklet

- **Status:** Deferred
- **Date:** 2026-10-13
- **Related requirements:** FR-WAM-01, FR-WAM-02, FR-APP-02, NFR-WEB-01 to 04

## Context

The Web Audio API lets us connect many nodes together. But a cycle between native nodes adds at least one 128-sample render quantum of delay. That breaks sample-accurate feedback (FR-GRAPH-04). Crossing between JavaScript and WASM for every module is also slow.

Faust and RNBO both run the complete DSP inside one AudioWorkletProcessor for this reason.

## Options considered

1. **One AudioWorkletNode per module, connected with Web Audio routing.** Easy to visualise. Breaks feedback, high overhead. Rejected.
2. **Whole patch in one WASM instance inside one AudioWorkletProcessor.** Same model as native. Feedback works.
3. **One instance per top-level sub-patch.** Some isolation. Feedback across sub-patch boundaries breaks. Rejected.

## Decision

Use option 2. The browser app and every exported WAM2 plugin run the full patch in one WASM engine instance inside one AudioWorkletProcessor. The engine runs its own schedule (ADR-0002) and talks to the main thread through the queues in ADR-0001.

## Consequences

- One code path for native and web DSP.
- The engine must handle the fixed 128-frame quantum efficiently. It also must not assume the host block size equals the worklet quantum, since WAM hosts may change.
- Heavy patches cannot spread across CPU cores in the browser for now. Multithreading is a later optimisation, kept off the real-time path (NFR-WEB-04).
- The WASM binary size matters because every exported plugin carries it (NFR-WEB-03).
