# Stereo Cables Specification

How the engine decides which cables carry stereo, and what modules do with it, so a stereo chain is one row of modules instead of two.

- **Kind:** living specification.
- **Status:** Agreed, not built yet ([#23](https://github.com/brylie/LoomDSP/issues/23)). Rule 13's cold start of copy 1 is Agreed for now and may change with the research in [#203](https://github.com/brylie/LoomDSP/issues/203).
- **Decision behind it:** [ADR-0020](../adr/0020-inferred-channel-count.md), refining [ADR-0007](../adr/0007-polymorphic-signal-model.md).
- **Use case and requirements:** FR-SIG-07, FR-SIG-01, FR-SIG-03, FR-CANVAS-12, NFR-RT-01
- **Background:** [Stereo cables](../learn/stereo-cables.md): why copies, why the mean, why a downmix can cancel.
- **Prior art:** Eurorack patches stereo as two cables and dual modules. VCV Rack carries several channels on one cable and lets each module decide what to do with them. Bitwig Grid works in stereo by default. Named only to explain the trade-offs; the rules below are our own.
- **Issues:** #23, #203 (new paths without a cold start)
- **Code:** not built yet. It will touch `crates/loom-core/src/descriptor.rs`, `compile.rs`, `module.rs`, and `engine.rs`.

## Goal

"I have a stereo source. I want to run it through a filter and a delay and hear both sides, without building the chain twice. And my modulation cables should not cost twice as much because somewhere in the patch there is stereo."

Example: a stereo source goes into a filter, then a shaper, then Audio Out. An LFO moves the filter cutoff. The filter and the shaper run once per side. The LFO and its cable stay mono.

## Behaviour

The player does nothing to make stereo happen. They patch as today.

| The player does | What happens |
|---|---|
| Patches a stereo output into a module's audio input | That module runs once per side. Its outputs become stereo, and so does everything downstream that passes audio on. |
| Patches a mono output into the same chain | Nothing changes. A mono chain stays mono and costs what it costs today. |
| Patches a stereo output into a control input, such as a cutoff | The control reads the average of the two sides, unless the module already runs per side for another reason. Then each side reads its own channel. |
| Removes the last stereo cable into a chain | The chain becomes mono again at the next compile. |

How the canvas shows a stereo cable is FR-CANVAS-12 (P2). See Open points.

## Rules

### Widths

1. **A cable carries one channel (mono) or two (stereo).** Nothing else. *Why:* FR-SIG-07 asks for stereo. More channels (surround, multichannel output) can extend the same rules later and are out of scope.

2. **A cable's width is the width of the output it comes from.** It is computed when the graph compiles and is never stored in a patch. *Why:* the width follows from the graph, so storing it would only let it go stale. Old patches load and get the same widths.

### Declaring stereo behaviour

3. **Every port declares its channel behaviour** in a new required `PortSpec` field, `channels: Channels`, written in the struct literal like `polarity`:

   | Value | On an input | On an output |
   |---|---|---|
   | `Channels::PerChannel` | Stereo arriving here makes the module run once per channel. | As many channels as the module runs: mono, or stereo when copied. |
   | `Channels::Downmix` | Stereo arriving here does not make the module run per channel. | Not allowed. |
   | `Channels::Stereo` | The module reads both channels itself. | The module always writes two channels. |

   *Why required:* like polarity, it is a choice the author should make on purpose. A default hidden in a builder would make every control input either too costly or wrong.

4. **The usual choice by signal type:** audio inputs are `PerChannel`. Modulation, pitch, phase, and gate inputs are `Downmix`. Outputs are `PerChannel`. An author may choose otherwise, for example a stereo-aware amplifier whose gain input should also run per channel. Generators such as oscillators and noise keep `PerChannel` outputs, so they stay mono. A generator declares a `Stereo` output only when its two sides differ by design, such as spread unison or noise that differs on each side. *Why:* audio is what players want in stereo. Control signals are almost always mono, and FR-SIG-07 says not to pay twice for them.

5. **A module with any `Stereo` port is never copied.** Its other inputs must be `Downmix`, and its other outputs are always mono. The registry rejects a descriptor that breaks this, and one with `Downmix` on an output. *Why:* a module that reads both channels itself has already decided how to handle them. Copying it would run its stereo logic twice.

### Inference

6. **A module runs as two copies when it has no `Stereo` port and at least one `PerChannel` input receives a stereo cable.** Otherwise it runs as one instance. *Why:* this is the only case where per-channel processing is both wanted and possible.

7. **Output widths:** a `Stereo` output is always stereo. A `PerChannel` output is stereo when its module runs as two copies, and mono otherwise. So a module with no inputs, such as an oscillator, is mono unless it declares a `Stereo` output. *Why:* stereo then flows only from modules that make it, as FR-SIG-07 asks.

8. **Widths are found with a work queue, to a fixed point.** Start with every module and output mono, and queue every module with a `Stereo` output. Take a module from the queue and apply rule 7 to its outputs. For each module those outputs feed, apply rule 6. If it now runs as two copies, mark it and queue it. Stop when the queue is empty. A module changes from mono to stereo at most once, so each module is queued at most once and each cable is followed at most once. The work is linear in modules plus cables. *Why:* inside a feedback loop the widths depend on each other, so a single pass in order is not enough. Repeating full passes until nothing changes would also be correct, but in the worst case costs modules times cables. Starting from mono means a loop is stereo only if stereo enters it, which is the smallest answer that is consistent.

9. **Unit delays follow the same rules.** `loom.time.delay1` has a `PerChannel` input and output. A delay the compiler inserts takes the width of the cable it breaks. Each channel has its own delay state, carried across schedule swaps by its key and channel. *Why:* a delay is an ordinary per-channel module; treating it any other way would mix the sides inside a loop.

### What inputs read

10. **Each copy reads its own channel.** Copy 0 is the left channel and copy 1 is the right.

    | Input | Cable | Module runs as one | Module runs as two copies |
    |---|---|---|---|
    | `PerChannel` or `Downmix` | none | its default | its default, both copies |
    | `PerChannel` or `Downmix` | mono | the cable | the cable, both copies |
    | `PerChannel` | stereo | (cannot happen, rule 6) | copy 0 left, copy 1 right |
    | `Downmix` | stereo | the mean of left and right | copy 0 left, copy 1 right |
    | `Stereo` | none | its default on both channels | (never copied, rule 5) |
    | `Stereo` | mono | the cable on both channels | |
    | `Stereo` | stereo | left and right | |

    *Why:* once a module runs per channel, giving each copy its own side of every stereo input is free and keeps the image. A downmix is only for modules that stay mono.

11. **A downmix is `(left + right) / 2`.** The compiler gives each downmixed input a buffer, written just before the module runs (one sample at a time inside a feedback loop). *Why:* a mono signal spread to both sides downmixes back to the same level. A sum would make it 6 dB louder, and taking the left side would drop the right without warning. The explainer shows the cases.

12. **A mono cable into a `Stereo` input costs nothing.** Both channels point at the same buffer. *Why:* no copy is needed, because inputs are read only.

### Copies

13. **Copy 0 is the module's existing instance.** It keeps its engine slot and its state when the module becomes stereo or mono again. Copy 1 is created on the control thread from the registry, reset, and given the module's current parameter values before the new schedule is sent. When the module becomes mono, copy 1 is sent back to the control thread to be freed. *Why:* the audio thread must not allocate or free (hard rule 1). Keeping copy 0 means a chain that turns stereo keeps its left side's sound.

14. **Both copies get every `set_param` call.** Each copy keeps its own parameter smoothing. *Why:* a module has one set of controls, whatever its width.

15. **A copy can ask which channel it is.** `ProcessContext::channel()` returns 0 for a module that runs as one, or for copy 0, and 1 for copy 1. A module with a seed (the Seed trait) must mix the channel into its seed. *Why:* without it, a copied noise source or random step generator gives the same values on both sides, which is mono in disguise.

16. **Displays and modulation live values come from copy 0.** *Why:* the module has one panel. Copy 0 always exists, so lights do not jump when the module changes width.

### Modulation

17. **Modulation assignments never make a module run per channel.** A stereo source into a module that runs as one gives the mean. Into a module that runs as two copies, each copy gets its own side, and a mono source gives both copies the same. *Why:* modulating a parameter from a stereo source is rare, and doubling the target for it would surprise the player. The rule mirrors `Downmix` inputs.

### API

18. **Stereo ports have their own accessors.** `PortIo::stereo_input(index)` returns `[&[f32]; 2]` and `PortIo::stereo_output(index)` returns `[&mut [f32]; 2]`. `PortIo::input`, `output`, and `split` are unchanged and, used on a `Stereo` port, give the left channel. *Why:* mono modules keep exactly the code they have. Only modules that opted in to stereo see two buffers.

19. **The compile report lists the width of every output.** *Why:* the canvas needs it to draw stereo cables (FR-CANVAS-12), and the inspector to show it.

### Audio Out, telemetry, and display

20. **Audio Out has one `Stereo` input, `in`,** in place of `left` and `right`. A mono cable plays on both sides, through rule 12. *Why:* one stereo cable to the output is the natural end of a stereo chain. The mono case behaves as before, and costs nothing.

21. **Join and Split are the first stereo modules,** built in P1 from MT-MIX-03. Join takes two mono inputs (`Stereo` output). Split takes one `Stereo` input and has a mono output per side. *Why:* with one stereo input on Audio Out, a player with separate left and right signals needs Join. Join is also the smallest module that makes stereo, so the feature can be heard and tested. Split gives back the case that a downmix cannot: reading one side on purpose.

22. **A meter on a stereo output shows both channels. A scope shows the mean of the two sides,** the same downmix as rule 11. *Why:* level is where the sides most often differ, and a player checks it per side. A scope trace is for shape, and one trace is easier to read. A scope of a signal whose sides are opposite shows a flat line, like any downmix (see the explainer). To see one side, probe a Split output.

23. **A port's hover text says "Stereo" when its cable is stereo,** until FR-CANVAS-12 (P2) draws stereo cables. *Why:* the player needs some way to see a width the engine chose for them.

### Cost

24. **A patch with no stereo source compiles to the same schedule as before this feature.** Same module operations, buffers, and instances, and no width checks inside `process`. The only extra work is the inference pass on the control thread. *Why:* FR-SIG-07's reason for existing. Most patches are mono, and most cables carry control signals.

## Acceptance criteria

- [ ] Given a patch with no `Stereo` output anywhere, when it compiles, then its schedule has the same number of module operations, buffers, and instances as on the main branch before this change, and the engine benchmark shows no regression beyond noise.
- [ ] Given a stereo test source into a filter into Audio Out, when the patch runs, then the filter runs as two copies, and each side matches a mono filter fed that side alone.
- [ ] Given a stereo cable into a `Downmix` input of a module that runs as one, then the input reads exactly `(left + right) / 2`.
- [ ] Given a mono cable into a `Stereo` input, then both channels read the same buffer, and no downmix or copy buffer is added.
- [ ] Given a feedback loop that no stereo cable enters, then every cable in it stays mono.
- [ ] Given a feedback loop whose modules pass audio on through `PerChannel` inputs, and one stereo cable into a `PerChannel` input in the loop, then every module in the loop runs as two copies, every cable in it is stereo, and it compiles.
- [ ] Given the same mono loop, and a stereo cable only into a `Downmix` input in the loop (such as a filter cutoff), then the loop stays mono, and that input reads the mean.
- [ ] Given a long chain built so that stereo reaches modules in the reverse of the order they are stored, then inference visits each module and cable at most once.
- [ ] Given a stereo chain, when the stereo cable is removed and the patch recompiles, then copy 1 of each module is freed on the control thread, copy 0 keeps its state, and the `no_alloc` tests pass.
- [ ] Given a copied module, when it is run through the module test harness, then its output is identical at block sizes 1, 77, and 512.
- [ ] Given a copied seeded module, then its left and right outputs differ, and both are the same on every run.
- [ ] Given each example patch, updated to Audio Out's `in` port, when it loads, then it gives bit-identical output to the patch before this change.
- [ ] Given a mono cable into Audio Out, then both sides of the device output carry it.
- [ ] Given two mono signals through Join and then Split, then each Split output is bit-identical to the signal that went in on that side.
- [ ] Given a probe on a stereo output, then the meter reports each channel, and the scope shows `(left + right) / 2`.
- [ ] Given a stereo cable into a port, when the player hovers it, then the hover text says "Stereo". (Manual check.)
- [ ] Given a descriptor with `Downmix` on an output, or a module with a `Stereo` port and a `PerChannel` input, then the registry rejects it.

## Out of scope

- **More than two channels** (surround, multichannel interfaces). The rules extend, but nothing needs them yet.
- **Polyphony** (FR-SIG-08, P2). Voice count is another dimension, decided with voice allocation.
- **Choosing a width by hand,** such as forcing a cable mono. A player who wants that patches through a downmix input or a stereo tool.
- **Mid/side cables.** Mid/side is a conversion a module does (MT-MIX-03), not a cable width.
- **Other stereo modules:** pan, width, and mid/side (MT-MIX-03 after P1), a stereo reverb, a stereo mixer. Each follows the catalogue. Join and Split are in scope (rule 21).
- **Cable styles for stereo** (FR-CANVAS-12, P2).

## Open points

1. **Copy 1 starts from reset.** When a cable change makes a running chain stereo, the right side starts cold, so a filter or delay can click or sound different for a moment. Players who know hardware will hear that as a bug. Research in #203: copy state from copy 0, crossfade from the left side, or another approach. Until it lands, the cold start stands.
