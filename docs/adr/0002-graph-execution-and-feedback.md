# ADR-0002: Graph execution model and sample-accurate feedback

- **Status:** Accepted
- **Date:** 2026-10-13
- **Related requirements:** FR-GRAPH-03, FR-GRAPH-04, FR-ENCAP-07, NFR-PERF-01, NFR-PERF-02, NFR-PERF-08

## Context

Users must be able to create feedback loops, and those loops must have a one-sample delay, not a one-buffer delay. If the delay equals the buffer size, a patch sounds different at 64 samples than at 512. That breaks sound parity (NFR-SND-03).

Processing one sample at a time across the whole graph is simple and always correct, but per-sample function calls are slow. Processing in blocks is fast but cannot express a one-sample loop.

## Options considered

1. **Whole graph per sample.** Correct, simple, slow. Overhead dominates small modules.
2. **Whole graph per block, feedback delayed by one block.** Fast, but sound depends on buffer size. Rejected.
3. **Hybrid.** Flatten sub-patches, find strongly connected components (SCCs). Run acyclic parts in blocks. Run each SCC per sample, in a fixed order, with a one-sample delay at the loop's break point.
4. **Compile the graph to code** (JIT or ahead-of-time). Fastest. Large complexity, and JIT is not available in all targets.

## Decision

Use option 3.

1. On the control thread, flatten all sub-patches into one graph (FR-ENCAP-07).
2. Find SCCs (for example with Tarjan's algorithm).
3. In each SCC, choose a break point. Use a user-placed one-sample delay if present, otherwise insert one and show it in the UI.
4. Topologically sort the condensed graph (each SCC is one node).
5. Build a schedule: block calls for acyclic modules, a per-sample inner loop for each SCC.

Modules implement both a block `process` and a single-sample `tick`, or provide a default `tick` built on `process` with block size 1.

## Consequences

- Acyclic patches run at full block speed.
- Feedback patches pay per-sample cost only inside the loop.
- The module trait has two entry points. Module authors need to test both. The SDK harness must cover this.
- Option 4 stays open as a later optimisation for exported plugins.
- This model does not depend on the browser's 128-sample block size. It works with any host buffer size.

## Validation

P0 benchmarks measure CPU cost for feedback loops containing 1, 10, and 50 modules, compared with the same modules in an acyclic chain. If per-sample overhead inside loops is too high, consider batching small sub-blocks inside the loop where the loop delay allows it, or code generation (option 4) for hot loops.

Result (2026-10-13, [p0-engine.md](../benchmarks/p0-engine.md)): per-sample mode adds about 5 to 15 ns per module per sample. A 50-filter loop uses 12% of one core's real-time budget. The model is kept.

Implementation notes:

- Modules have a single `process` entry point. Inside loops the engine calls it with one frame, so no separate `tick` method is needed. The requirement is that modules are block-size invariant, which the test suite checks.
- When the compiler must insert delays, it picks the module with the fewest unresolved inputs (lowest ID on ties) and delays those inputs. The choice is deterministic, and `loom check` shows it.
