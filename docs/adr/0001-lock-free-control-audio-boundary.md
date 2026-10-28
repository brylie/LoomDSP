# ADR-0001: Lock-free boundary between control and audio threads

- **Status:** Accepted
- **Date:** 2026-10-13
- **Related requirements:** NFR-RT-01 to 07, FR-GRAPH-01, FR-GRAPH-02, NFR-WEB-01, NFR-WEB-02

## Context

The audio thread must finish each buffer before a hard deadline. Locks, allocation, and system calls have unbounded duration, so the audio thread cannot use them. Yet the UI constantly changes parameters and graph structure, and the UI needs meter and scope data back.

## Options considered

1. **Mutex around shared state.** Simple. The audio thread can block when the UI holds the lock (priority inversion). Rejected.
2. **Lock-free SPSC queues for messages, plus atomic pointer swap for whole graph schedules.** Wait-free on the audio side. Needs careful memory handling.
3. **Triple buffering of all state.** Good for bulk telemetry. Wasteful for small, frequent parameter changes.

## Decision

Use option 2, with triple buffering only for large telemetry (scopes, spectra).

- **Control to audio:** a bounded SPSC queue of fixed-size messages (parameter change, MIDI event, "new schedule ready").
- **New graph schedules:** built and allocated on the control thread, then published through an atomic pointer. The audio thread picks it up at the next buffer start.
- **Audio to control:** a bounded SPSC queue that returns retired schedules and freed buffers for deallocation, plus telemetry.
- Queue indices sit on separate cache lines to avoid false sharing. Use acquire/release ordering.
- In the browser, the same queues live in a SharedArrayBuffer when available. Otherwise a `postMessage` adapter feeds them on the audio side (NFR-WEB-02).

## Consequences

- The engine core needs no threads of its own, which keeps it portable to WASM.
- All message types must be fixed-size and `Copy`. Large data (wavetables) is passed by pointer to memory allocated on the control thread.
- We need a deferred-free mechanism. Memory leaks are possible if the return queue is not drained, so tests must check this.
- Prefer existing crates (for example `rtrb`, `basedrop`, or `arc-swap` patterns) over custom code where they meet the requirements and licence rules.

## Implementation (P0)

- Queues: `rtrb` (MIT/Apache-2.0). Three queues: commands (control to audio), garbage (audio to control), and MIDI (MIDI thread to audio).
- Module instances live in a fixed array of engine slots, allocated once. A new module is created on the control thread and moved into a free slot. Removed modules are moved back through the garbage queue.
- The garbage queue has the same capacity as the command queue, and the controller drains it before sending, so it cannot overflow in practice. If it ever does, the engine leaks the object instead of freeing it, and counts the leak in `EngineStats`.
- The controller checks there is room for a schedule swap and all of its removals before sending any of them, so the engine never sees half an update.
- A test runs the engine under `assert_no_alloc` with edits between buffers (NFR-RT-06).
