# ADR-0020: Channel count inferred per cable, with per-channel copies

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-19
- **Related requirements:** FR-SIG-07, FR-SIG-01, FR-SIG-03, FR-SIG-08, FR-CANVAS-12, NFR-RT-01, NFR-PERF-03
- **Specification:** [Stereo cables](../design/stereo.md)
- **Background:** [Stereo cables explainer](../learn/stereo-cables.md)
- **Refines:** [ADR-0007](0007-polymorphic-signal-model.md), section "Channel count inference", which sketched this idea.

## Context

Every cable today carries one channel. The only stereo point is Audio Out, which takes a left cable and a right cable. A stereo effect chain therefore needs every module and every cable twice. Patches double in size, and the player must keep both sides in step by hand.

FR-SIG-07 asks for stereo where a stereo source exists, and nowhere else. Most cables carry modulation, pitch, or gates, and running two channels on them would waste CPU and memory for nothing (OQ-03).

Three questions need an answer:

1. **Inference.** How does the engine know how many channels a cable carries?
2. **Declaration.** How does a module say what it does with stereo?
3. **Cost.** What does a patch that never uses stereo pay?

Constraints: the hard rules apply in full. Nothing on the audio thread may allocate or branch on data it cannot predict (rule 1). Output must not depend on block size (rule 2) or vary between runs (rule 3). The patch format should not change, because the width of a cable follows from the graph and is not something a player edits.

## Options considered

### Where channel count comes from

1. **Stereo on every cable.** Simple, and one code path. Doubles CPU and memory on every cable, most of which carry modulation. Rejected by OQ-03.
2. **Mono cables only (today).** No cost. Stereo patches double in size, and pan, width, and stereo reverb cannot be single modules.
3. **The player chooses per cable or per module.** Explicit, but it is one more thing to manage, and a wrong choice silently drops a channel.
4. **Detected at run time**, by checking whether left and right differ. CPU use would change with the signal, so the audio thread could not plan its work, and a patch could get louder or cheaper mid-performance.
5. **Inferred when the graph is compiled.** Sources declare stereo. Widths spread along cables on the control thread, and the compiled schedule holds the result. Costs nothing on the audio thread, and needs no player action.

### How a module handles stereo

A. **The engine runs a copy of the module per channel.** The module's code stays mono. Each copy has its own state. Most filters, shapers, delays, and amplifiers want exactly this. It cannot express modules that mix the channels, such as a panner or a stereo reverb.

B. **Every module reads and writes N channels itself.** Flexible, but every author handles channel loops in every module, and the common case is written again and again.

C. **A, plus native stereo ports for modules that need both channels at once.**

### What a mono-only input does with stereo

- **Sum** (L + R). A centred mono signal spread to both sides comes back 6 dB louder.
- **Left only.** Cheap, but silently drops the right side.
- **Mean** ((L + R) / 2). A centred signal comes back at the level it had.
- **Equal power** ((L + R) / √2). Keeps the loudness of unrelated left and right material, but a centred signal comes back 3 dB louder.
- **A method per signal type.** The mean for levels, but the maximum for gates (the mean of a gate high on one side is 0.5, which reads as low) and a mean that respects the wrap for phases (the plain mean of 0.9 and 0.1 is 0.5, not 0.0).

## Decision

Options 5 and C, with a downmix method per signal type.

- **Widths are inferred at compile time,** on the control thread, and never stored in a patch. A cable is stereo only when a module that declares a stereo output feeds it, directly or through modules that pass stereo on.
- **Each port declares its channel behaviour.** An input either makes the module run per channel when stereo arrives (`PerChannel`), or takes both channels itself (`Stereo`), or does neither (`Downmix`). A `Downmix` input reads one value made from the two sides while the module runs as one, and gives each copy its own side when another input already makes the module run per channel. An output either has as many channels as its module runs (`PerChannel`) or is always stereo (`Stereo`). A module with a `Stereo` port is never copied.
- **A module that receives stereo on a `PerChannel` input runs as two copies,** one per channel, each with its own state. All its outputs become stereo. A mono cable feeds both copies the same signal.
- **The input's signal type picks the downmix:** the mean for audio, modulation, and pitch, the maximum for gates, and the mean the shorter way round the circle for phases. The author writes only `Downmix`.

The exact rules, the defaults by signal type, and the API are in the [specification](../design/stereo.md).

## Consequences

- **A mono-only patch pays nothing on the audio thread.** Its schedule has the same steps, buffers, and module instances as today. The only new work is the inference pass when the graph compiles, which uses a work queue so that its cost is linear in the number of modules and cables. Modules gain one declaration per port, and their `process` code does not change.
- **Stereo costs only where it flows.** A copied module costs about twice as much, a downmix costs a few operations per sample, and a mono cable into a stereo port costs nothing (both channels read the same buffer).
- **Most module authors write mono code.** Only modules that mix channels (panners, stereo reverbs, linked limiters, stereo sources) handle two buffers themselves.
- **The patch format does not change.** A cable's width follows from the graph, so loading an old patch infers the same widths.
- **Changing one cable can change widths downstream,** so a recompile may create or retire module copies. Copies are created and freed on the control thread and handed over at a buffer boundary, as slots are today. A new right-hand copy starts from reset, so its first block can differ from the left. [ADR-0022](0022-copies-take-state-from-the-running-instance.md) copies copy 0's state into it instead.
- **Seeded modules must tell their copies apart,** or a copied noise source would be the same on both sides. Modules can read which channel a copy runs.
- **Feedback loops need a fixed point.** Widths inside a loop depend on each other. Width only grows from mono to stereo, so iteration stops, and a loop is stereo only if stereo enters it.
- **A downmix can cancel sound.** A signal whose sides are opposite in sign downmixes to silence. The explainer covers it, so players know why.
- **Audio Out and the mixer need stereo-aware ports,** and the UI needs each cable's width to draw stereo cables (FR-CANVAS-12). The spec lists these.
- **Polyphony is a separate dimension** (FR-SIG-08). The same compile pass is the natural place to infer voice count later, but how voices run (copies or SIMD lanes, NFR-PERF-03) is a separate decision.
