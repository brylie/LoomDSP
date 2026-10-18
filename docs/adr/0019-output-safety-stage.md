# ADR-0019: A safety stage on the final output

- **Status:** Proposed
- **Date:** 2026-10-18
- **Related requirements:** FR-PERF-08, FR-PERF-05, NFR-SND-02, NFR-SND-03, NFR-RT-01
- **Specification:** [Output safety](../design/output-safety.md)
- **Background:** [Output safety explainer](../learn/output-safety.md)
- **Extends:** [ADR-0007](0007-polymorphic-signal-model.md), which says audio is clipped only at the final output.

## Context

Since #123 the engine clips its summed output to -1 to 1 and turns NaN and infinity into silence. That stops broken numbers from reaching the device. It does not stop a legal signal that is still harmful:

- **DC.** A Constant of 1.0 into Audio Out passes the clip unchanged. It holds the speaker cone at full excursion and heats the voice coil, and nobody hears it happen.
- **Loud, legal signals.** Full-scale noise or a square wave is painfully loud on headphones. A hard clip at 0 dBFS also lets peaks between samples go over in the device's converter.
- **Sudden jumps.** Loading a patch, changing a cable, or turning a knob to an extreme (as in #121) can go from silence to full scale in one sample.

LoomDSP is built for live performance, and patching anything to anything is the point (FR-SIG-03). So these mistakes will happen, often on stage, through a PA, with no time to react. The protection must already be on when they happen.

Constraints: the stage runs on the audio thread, so it must be real-time safe, block-size invariant, and deterministic (hard rules 1 to 3). Some players need the raw signal: studio users who master elsewhere, and players who send control voltages to Eurorack through a DC-coupled interface. A lookahead limiter is cleaner but adds latency, which live players feel.

## Options considered

1. **Clip only (today).** Simple, no latency, no colouring. Does nothing about DC, loudness, or jumps.
2. **Leave it to the patch.** Players add a limiter module before Audio Out. Flexible, but it protects only patches whose author remembered, and an accident is exactly the case nobody planned for.
3. **A fixed safety stage, always on.** A DC blocker, a limiter, and a fade-in in the engine, with no setting. Safe, but it removes DC that some players need, and it forces one latency trade-off on everyone.
4. **A configurable safety stage in the engine, on by default.** The same stage, with a mode: *Low latency* (no lookahead), *Clean* (short lookahead, adds its latency), or *Off*. The setting belongs to the machine (app preferences), not the patch.

## Decision

Option 4. The engine runs a safety stage after it sums its outputs: a DC blocker, a safety limiter with a ceiling below full scale, and a short fade-in when audio starts. Its mode is *Low latency*, *Clean*, or *Off*, set in the app's preferences, and *Low latency* is the default. The clip to -1 to 1 and the silencing of non-finite samples from ADR-0007 stay, as the last step, whatever the mode.

The setting is per machine, not per patch, because what it protects is the player's speakers and ears. A patch shared from someone else must never be able to turn the protection off.

## Consequences

- A patch mistake no longer reaches speakers as DC, full-scale noise, or a click at load. Players can patch freely without a limiter of their own.
- The default adds no latency. Players who prefer clean limiting choose *Clean* and accept a few milliseconds.
- With the stage on, the app's output differs from the raw patch output: by the DC blocker always, and by the limiter when it acts. Offline renders and exported plugins default to *Off*, so tests and NFR-SND-03 compare the raw signal (see the spec).
- Players sending DC to hardware must choose *Off*, and lose the protection for that machine. Per-channel settings wait for multi-channel outputs.
- Panic (#25) and a future master volume must say where they sit relative to the stage. The spec gives the order.
- The engine needs a command to change the mode, and a small delay line allocated when the engine is built, so that switching to *Clean* never allocates.
- Engine-level tests that check exact output values must choose a mode explicitly.
