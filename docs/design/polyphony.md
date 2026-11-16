# Polyphony and Voice Allocation Specification

How a cable carries one value per voice, how notes are given to voices, which modules make, merge, and spread voices, and what a module author has to do to become polyphonic (nothing, in most cases).

- **Kind:** living specification.
- **Status:** Agreed. Settled with the maintainer on 2026-11-06. The bundle rules, the voice count, allocation, and the four voice modules are M2. The three sources followed [end to end](#three-sources-end-to-end) are the test of the model; the chord sequencer and the note cycle themselves are M3. The shared allocator and bundle engine are implemented. Note input, Voice merge, Voice split, and Voice spread are implemented. Voice assigner remains pending. Refined on 2026-11-14 and 2026-11-15 for [#675](https://github.com/brylie/LoomDSP/issues/675) and [#677](https://github.com/brylie/LoomDSP/issues/677): Voice split, Voice spread, and the shipped 8-voice example with its benchmark. Agreed with the maintainer on 2026-11-16 for [#676](https://github.com/brylie/LoomDSP/issues/676), [#793](https://github.com/brylie/LoomDSP/issues/793), [#794](https://github.com/brylie/LoomDSP/issues/794), and [#787](https://github.com/brylie/LoomDSP/issues/787): how Voice assigner identifies and follows a note, Voice merge's `gate` output and its non-finite ranking, and the second reference patch.
- **Decision behind it:** [ADR-0038](../adr/0038-notes-travel-as-per-voice-signals.md) (per-voice signals, bundle as inferred width), refining [ADR-0020](../adr/0020-inferred-channel-count.md) (inferred channel count) and [ADR-0007](../adr/0007-polymorphic-signal-model.md) (every cable is `f32`). [ADR-0022](../adr/0022-copies-take-state-from-the-running-instance.md) for the state of a new copy. [ADR-0030](../adr/0030-panels-hide-controls-a-mode-does-not-use.md) for the allocator's panel. [ADR-0045](../adr/0045-a-seeded-value-of-a-voice-is-the-same-on-both-sides.md) for Voice spread's random offsets on a stereo voice.
- **Use case and requirements:** UC10 (sequence chords and note clips), UC2 (live play). FR-SIG-08, FR-VOICE-01 to FR-VOICE-05, FR-MIDI-03 (MPE), FR-LIB-13, FR-LIB-20, NFR-RT-01, NFR-PERF-01, NFR-PERF-03.
- **Builds on:** [Stereo cables](stereo.md) (widths, copies, downmix), [Changing a running patch](live-patching.md) (schedule swaps), [Sub-patches](sub-patches.md) (stack mode is not voices), [Module data](module-data.md) (stored notes and chords), [Scale and tuning](scale-and-tuning.md) (chords as scale steps), [Arrangement](arrangement.md) (the note cycle).
- **Background:** [Voices and polyphony](../learn/voices-and-polyphony.md): what a voice is, why stealing exists, why a bundle is inferred, and why a gate carries the timing.
- **Prior art:** per-voice signals are how a modular grid such as Bitwig's Grid carries polyphony. Hardware polysynths settled oldest-note and quietest-note stealing decades ago. Named only to explain the ideas; the rules and modules below are our own.
- **Issues:** [#665](https://github.com/brylie/LoomDSP/issues/665) (this spec), [#666](https://github.com/brylie/LoomDSP/issues/666) (the note model), [#675](https://github.com/brylie/LoomDSP/issues/675) (Voice merge, split, and spread), [#677](https://github.com/brylie/LoomDSP/issues/677) (the 8-voice example and its benchmark), [#676](https://github.com/brylie/LoomDSP/issues/676) (Voice assigner), [#793](https://github.com/brylie/LoomDSP/issues/793) and [#794](https://github.com/brylie/LoomDSP/issues/794) (Voice merge's `gate` output and non-finite values), [#787](https://github.com/brylie/LoomDSP/issues/787) (the large reference patch). Users: [#510](https://github.com/brylie/LoomDSP/issues/510) (MIDI out), [#307](https://github.com/brylie/LoomDSP/issues/307) (multi-lane sequencer), [#50](https://github.com/brylie/LoomDSP/issues/50) (stored notes).
- **Code:** `crates/loom-dsp/src/voice.rs` (allocator); `crates/loom-core/src/width.rs` and `compile.rs` (bundles and copies). `crates/loom-modules/src/test_modules.rs` supplies the bundle test source. `crates/loom-modules/src/midi.rs` (Note input), `voice.rs` (Voice merge, Voice split, and Voice spread), and `crates/loom-core/src/mpe.rs` (per-input MPE configuration). Voice split's tests are in `crates/loom-modules/tests/voice_split.rs`, its reference render in `tests/reference.rs`, and its engine tests (a modulated `first`, a stereo bundle) in `tests/polyphony.rs`. Voice spread's Even offsets come from `loom_dsp::blend::spread_offset`, which the stack's spread already uses. Its tests are in `crates/loom-modules/tests/voice_spread.rs`, its reference render in `tests/reference.rs`, and its engine test for ADR-0045 in `tests/polyphony.rs`. The shipped example is `examples/patches/polysynth.ron`. Its reference render is tested in `crates/loom-modules/tests/patches.rs`, its compile report and headroom in `tests/polyphony.rs`, what the app and a screen reader show of it in `crates/loom-app/src/polysynth_tests.rs`, that `loom check` accepts it in `crates/loom-cli/tests/check.rs`, and its share of real time is printed by `crates/loom-modules/benches/engine.rs` ([the record](../benchmarks/2026-11-15-polysynth.md)). Voice assigner is `ASSIGN` in `crates/loom-modules/src/voice.rs`, sharing its `voices` and `mode` parameters with Note input. Its tests are in `crates/loom-modules/tests/voice_assign.rs`, its reference render in `tests/reference.rs`, and its 8-voice allocation check in `tests/no_alloc.rs`. Its effective-mode hiding of `unison` is `ModuleDescriptor::effective_value` and `is_shown` in `crates/loom-core/src/descriptor.rs`, tested in `descriptor/tests.rs`. What the app and a screen reader show of it is tested in `crates/loom-app/src/polysynth_tests.rs`. It is placed in `examples/dev/all-modules.ron` and listed in the widget lists of `crates/loom-modules/src/tests.rs`. Voice merge's `gate` output and non-finite ranking are tested in `crates/loom-modules/tests/voice_merge.rs`. The large reference patch is `examples/patches/polysynth-large.ron`: its reference render in `tests/patches.rs`, its compile report and headroom in `tests/polyphony.rs`, `loom check` in `crates/loom-cli/tests/check.rs`, the app in `crates/loom-app/src/polysynth_tests.rs`, and its benchmark line in `benches/engine.rs`. `scripts/gen-polysynth-large.py` writes it.

## Goal

> I set Note input to 8 voices and cable it to an oscillator, a filter, an envelope, and a Voice merge into Audio Out. That is a polysynth: I did not change one module. I hold a chord and play over it, and when I run out of voices the oldest note gives way. Later I replace the keyboard with a chord sequencer, and the notes two chords share keep ringing on their voice. I patch one LFO into the filter, and every voice follows it.

## Terms

| Term | Meaning |
|---|---|
| **Voice** | One note's worth of signal: a pitch, a gate, a velocity, and expression, and everything a patch makes from them. |
| **Bundle** | A cable that carries one value per voice. Its **voice count** is from 1 to 16. A cable with one voice is an ordinary cable. |
| **Width** | A cable's voice count together with its stereo side count ([stereo](stereo.md)). A cable can be 8 voices of stereo: 16 channels. |
| **Copy** | One instance of a module, running one voice (and, inside that, one stereo side). Voices are numbered from 1; voice 1 is the module's original instance. The code may index from 0. |
| **Source** | A module that makes a bundle from notes: Note input, Voice assigner, and in M3 the note cycle, the chord sequencer, and the note effects. |
| **Allocator** | The rule that gives an incoming note a voice. One algorithm in `loom-dsp`, used by every source. |

## Behaviour

What the player sees.

| The player does | What happens |
|---|---|
| Sets **Voices** on Note input to 8 | Every cable from it, and every module after it up to a Voice merge or Audio Out, becomes 8 voices wide. Hover text on a port says `8 voices`. The canvas draws the width ([FR-CANVAS-12](../product/requirements.md)). |
| Plays a ninth note | The oldest note's voice is taken: its gate drops for one sample and rises again with the new pitch, so its envelope restarts. |
| Patches an LFO (mono) into the filter cutoff of a polyphonic chain | Every voice gets the same LFO. Nothing to set. |
| Patches Voice spread after the bundle into the oscillator's detune | Each voice gets its own offset from the one knob. |
| Cables a polyphonic chain straight into Audio Out | The voices are summed. A Voice merge gives other choices: average, last, lowest, highest. |
| Patches the pitch bundle into a Voice merge set to Lowest, and the gate bundle into its `gate` | One pitch from a chord: the lowest held key. When that key is released, the next lowest held key takes over. When every key is up, the pitch stays on the last note, so a release after it does not jump. Voice merge's `gate` output goes to the bass envelope: it is high while the chosen key is held, stays high when the next lowest key takes over, and falls when every key is up. So the bass plays legato, as Note input does at one voice. |
| Patches a mono sequencer's pitch and gate into a Voice assigner at 8 voices | Each new step takes its own voice, so the release tails of earlier steps keep ringing under the new one. |
| Sets **Unison** to 2 on a Voice assigner and patches Voice spread after it to detune | Each key takes two voices, detuned from each other. |
| Sets Voices from 8 to 4 while playing | Voices 1 to 4 keep sounding. Notes on voices 5 to 8 stop at the swap. |
| Sets Voices to 1 | The module is a mono source again, with last-note priority, and the **Mode** control shows Legato and Retrigger instead of the stealing rules. |
| Opens the inspector on a polyphonic module | The module has one set of controls, as a stereo module does. Displays and lights show voice 1. |

## Rules

### Bundles

1. **A cable carries 1 to 16 voices, each mono or stereo.** *Why 16:* the allocator is a fixed table, and sixteen voices is more than the M2 exit criterion and the reference patch need. The sub-patch stack has the same limit, for the same reason. A wider need is a later change to one number.

2. **A cable's width is the width of the output it comes from.** It is computed when the graph compiles and never stored in a patch ([stereo](stereo.md), rule 2). *Why:* a stored width goes stale. The voice count that matters is stored, as a parameter on the source (rule 12).

3. **Ports gain one more channel behaviour.** The `channels` field of a `PortSpec` ([stereo](stereo.md), rule 3) takes `Channels::Voices` on either side. The three stereo values keep their stereo meaning, and for voices they all behave alike:

   | Value | On an input | On an output |
   |---|---|---|
   | `Channels::PerChannel`, `Channels::Downmix`, `Channels::Stereo` | A bundle arriving here makes the module run once per voice. Inside each voice the stereo meaning is unchanged: `PerChannel` copies per side, `Downmix` reads one value from the two sides, `Stereo` reads both sides of its voice. | As many voices as the module runs; sides as the stereo rules say. |
   | `Channels::Voices` | The module reads every voice of the bundle itself, and is not copied per voice. | The output has as many voices as the module's `voices` parameter says. The module is a source and is never copied. A module may have both: it then runs as one instance (Voice assigner). |

   *Why voices do not follow the stereo split:* the stereo rules make control inputs `Downmix` so a stereo patch does not pay twice for a control signal that is the same on both sides ([stereo](stereo.md), rule 4). A voice's pitch and gate are the opposite case: they differ per voice by definition, and an envelope that read the maximum of eight gates could not play eight notes. The oscillators, Phase generator, Envelope, and Amplifier keep their `Downmix` control inputs and become polyphonic with no change. *Why one name for both sides of `Voices`:* it mirrors `Stereo`, which also means "reads both" on an input and "writes both" on an output.

4. **A module runs as one copy per voice when any input other than a `Voices` input receives a bundle.** Its voice count is the largest voice count among those inputs. A module with any `Voices` port never runs per voice, whatever its other inputs receive. Its inputs must all be `Voices` or `Downmix`; the registry refuses other input declarations. Inside each voice, the stereo rules apply unchanged, so a voice can run as two copies. *Why the largest:* a 4-voice pitch and an 8-voice gate into one oscillator should give 8 oscillators, not 4; the narrower cable is padded (rule 6).

5. **Output widths.** A `Voices` output has the module's voice count. Any other output has as many voices as its module runs. For a bundle-aware module, a `PerChannel` output takes its stereo sides from the first declared `Voices` input, the signal input; later control inputs do not widen it. A `Stereo` output is stereo per voice. So a module with no inputs is one voice wide unless it declares a `Voices` output, and voices flow only from sources, as stereo flows only from stereo sources ([stereo](stereo.md), rule 7).

6. **A narrower cable into a wider module is padded with the port's default.** Copies past the cable's last voice read the input's default: zero for a gate, the default pitch for a pitch. Padded value inputs keep their declared default and still report that a cable exists. An input whose default is a normal of 1 (Amp `level`, Multiply `b`, Divide `b`) pads with 1, so a voice past the end of a narrower level bundle plays at unity, as the module does with no cable. When an input chooses an alternate source, its missing voice uses that source: OwnPhase runs its phase, Dynamics keys from its own signal, and Chance follows the lane above. These fallbacks use `uses_default`, while `is_connected` remains cable-only. *Why:* missing value inputs should not invent a playing signal; alternate-source inputs already define what replaces a missing source. Mono cables are the common case: a mono cable into a polyphonic module feeds every copy, as it feeds both stereo sides. That is the fan-out FR-VOICE-03 asks for, and it costs no module.

7. **Voices are never downmixed by the engine.** A module that wants one signal from a bundle has a `Voices` input, or sits after a Voice merge, which offers sum, average, last, lowest, and highest. Audio Out is the one exception (rule 9). *Why:* the stereo downmix exists so that a mono module can sit on a stereo cable without a thought. There is no one right way to fold eight voices into one, and an averaged pitch is never what a player meant, so the choice stays with the player.

   A modulation assignment from a bundle follows the same rule and the stereo one ([stereo](stereo.md), rule 17, [modulation](modulation.md)): an assignment never makes its target run per voice. When the target already runs per voice, each copy reads its own voice of the source; when the target runs as one, it reads voice 1. *Why:* an assignment is not a cable, so it cannot decide a module's width, and this rule rules out a mean. A player who wants eight voices of modulation into one parameter patches a Voice merge first.

8. **Widths are found with the stereo work queue, to a fixed point.** Start with every module at one voice, queue every module with a `Voices` output, and apply rules 4 and 5 outward until nothing changes. A module's voice count only grows, so each module is queued at most 16 times in theory and once in practice. *Why:* [stereo](stereo.md), rule 8, for the same reason: inside a feedback loop the widths depend on each other.

9. **Audio Out sums the voices** of a bundle on each side, before the output safety stage. Probes, scopes, meters, lights, displays, and modulation live values read voice 1, as they read stereo copy 0 ([stereo](stereo.md), rule 16). Hover text on a port names the width (`8 voices`, `8 voices, stereo`). *Why sum:* a chord played into the output should be a chord. The mean would make one note of eight 18 dB quiet. *Why voice 1:* the module has one panel, and voice 1 always exists.

10. **Unit delays and inserted delays take the width of the cable they break,** with state per voice and side, carried across swaps by key, voice, and side ([stereo](stereo.md), rule 9).

11. **A patch with no source wider than one voice compiles to the same schedule as before this feature.** *Why:* [stereo](stereo.md), rule 24: most patches are mono, and the inference pass is the only cost they pay.

### The voice count

12. **The voice count is a parameter, `voices`, from 1 to 16, on every source.** Its default is 1, so an existing patch with a Note input keeps playing as it did. It changes the graph, so it is not modulatable, as the stack's Copies is ([sub-patches](sub-patches.md#modes)). It is kept in the inspector and saved with the patch. Changing it is one undo step. *Why on the source:* FR-VOICE-01 asks that a patch or sub-patch can be polyphonic with a voice count. The count belongs to whatever hands out the voices, and a patch or a sub-patch is polyphonic wherever a bundle flows through it. A sub-patch's Patch input passes a bundle's width through unchanged, and a sub-patch with a source inside has that source's count. A sub-patch that wants a count of its own, whatever feeds it, puts a Voice assigner at its input. Nothing is marked.

13. **Stack mode and voices multiply.** A stacked sub-patch fed a bundle runs copies times voices. *Why allowed:* unison of a polyphonic voice is a normal instrument. The cost is the product, and the inspector shows it as it shows any module's load.

14. **Voice 1 is the module's existing instance.** It keeps its engine slot and state when the count changes. New voice copies are created on the control thread from the registry, reset, and given the module's current parameter values before the schedule is sent, as stereo copy 1 is ([stereo](stereo.md), rule 13). **A new voice copy starts from `reset` at the swap and does not take another voice's state.** *Why:* a voice is a separate note, not a second side of one signal, so copying a sounding voice into a new one would play a ghost. Inside a voice, a new stereo side still takes its voice's left-side state (ADR-0022), because that is one signal.

15. **A voice removed by a lower count is cut at the swap,** as a removed module is ([live patching](live-patching.md)). Its copy goes back to the control thread to be freed. The allocator forgets its note. *Why:* a lower count is an edit, not a performance gesture; the player who wants fewer voices on stage sets the count before the set.

16. **Every copy gets every `set_param` call,** and keeps its own smoothing ([stereo](stereo.md), rule 14). A seeded module mixes its voice index into its seed, as it mixes its stereo side ([stereo](stereo.md), rule 27), so eight voices of noise differ. *Why:* one set of controls, whatever the width; and copies of a random module that played the same numbers would be mono in disguise.

17. **The copy index and the voice count are given to the module.** `ProcessContext` names the copy's voice (1 to V), its side, and the module's voice count V, so a module such as Voice spread can act on its place among the copies without any other input. *Why:* the index is the only thing that differs between copies before any cable does, and an even spread needs to know how many there are.

### Structural parameter edits (implemented, #672)

- A parameter marked `ParamOptions::shapes_graph` must be an integer and cannot
  be transient. Only DSP modules may own `Voices` ports; engine-handled roles
  follow ordinary ports' inferred widths. A `Voices` output requires a structural
  `voices` parameter with bounds 1 to 16. The registry checks these rules.
- Editing a structural parameter stages the graph value, prepares a schedule
  and copy slots on the control thread, then publishes the schedule. Structural
  values reach every module copy as part of the schedule swap. Failure
  keeps the old graph value and running schedule. Ordinary parameters still
  use the next-buffer command path. `Controller::set_params_across` stages edits
  to multiple modules together, including every copy of a stack. One capacity
  check and one compile publish a live edit, or all values stay unchanged.
  Fresh modules and a graph with pending structural edits keep the values staged
  until the caller commits the whole graph. Initial values reach fresh instances
  before reset. MIDI feedback is reconciled before staging values and is not
  imported again while compiling that snapshot. Writing
  an unchanged rounded structural value does not compile or change the graph
  revision; ordinary values in that group still take effect. A count edit is
  one undo step.
- Structural values are saved and may be exposed on sub-patch surfaces. Their
  controls remain editable, but MIDI learn and modulation cannot target them,
  including through an exposed name. A patch file with a MIDI mapping on such
  a value is refused; its error names the controller and parameter.
- A source reads its authoritative capacity from `PortIo::output_width`, not
  from an independently delivered parameter. `PortIo::voice_input` and
  `voice_output` address a zero-based voice and side. `ProcessContext::voice`
  is one-based; `voice_count` names the module's copy count, and `channel` still
  names its zero-based stereo side. `PortIo::uses_default` identifies unconnected
  and padded inputs without changing `is_connected`'s cable-only meaning. This
  lets padded `OwnPhase` inputs run their own phase rather than hold at zero.
- Copy slots and delay state are keyed by voice and side separately. New voices
  start reset; a new right side copies its own voice's left side. Seed words
  reserve separate bits for voice, stereo side, and stream number. Voice 1
  keeps its previous sequences; routing randomness stays matched across sides.

### Allocation

18. **One allocator, in `loom-dsp`.** A fixed table of 16 slots, each with a pitch, a gate state, the frame its note started, and a serial number for order. It allocates, steals, and releases without allocation, locks, or waiting (hard rule 1). Every source uses it. *Why one:* the cost of per-voice signals is that every source allocates. Paying it once in an algorithm with its own tests (ADR-0035) keeps the sources thin and keeps them agreeing.

19. **A note takes the voice that has been free the longest.** *Why:* the voice whose release tail is oldest is the least audible to cut, and round-robin reuse spreads wear across voices.

20. **Mode** is a parameter on every source, `mode`, with four choices, shown by the panel only where the count uses them (ADR-0030):

    | Count | Choices | Meaning |
    |---|---|---|
    | 2 to 16 | **Oldest** (default), **Quietest** | What to steal when no voice is free. Oldest steals the voice whose note started first. Quietest steals the voice with the lowest value on the source's `level` input, a polyphonic input the player patches from the envelope's output; with `level` unconnected, Quietest acts as Oldest. |
    | 1 | **Legato** (default), **Retrigger** | Last-note priority. Legato keeps the gate high while any note is held and only moves the pitch, so the gate rises again only after every note was released: this is what Note input does today, so it is the default. Retrigger drops the gate for one sample on each new note. |

    *Why one parameter:* a player sees one control, Mode, and only the choices that mean something at the current count. *Why `level` is an input:* the engine does not know which signal is a voice's loudness; the player does.

21. **A stolen or retriggered voice's gate is low on the note's frame and high from the next frame.** A note on a free voice raises its gate on its own frame. *Why:* a trigger input counts rising edges, so a restart needs one low sample, and that sample cannot be put into the past. The one-sample delay is deterministic and the same at every block size.

22. **A note already sounding takes its own voices again.** A note-on for a note that is held retriggers its voices rather than taking others. A note is identified by its pitch, and on Note input by its pitch and MIDI channel, so two MPE member channels can play one pitch on two voices with their own expression and note-offs. On Voice assigner a note is identified by the input voice it arrives on (see [Voice assigner](#voice-assigner-mt-voice-04)), because a gate on one input voice carries one note at a time. With unison (Voice assigner), one note holds several voices: all are taken together, retriggered together, and released together. *Why:* a keyboard cannot play one key twice at once, and a source that re-sends a note means "again", not "also".

23. **A chord change is note-offs for the pitches that left and note-ons for the pitches that arrived.** A source never re-sends a pitch that stays. With rules 19 and 22 this gives voice leading for free: a pitch two chords share keeps its voice and its gate, so its envelope does not restart (UC10, step 2). *Why a rule for sources:* the allocator cannot tell "the same note continuing" from "the same note again" except by what the source sends.

24. **Note-off drops the gate on its frame and frees the voice at once.** The voice holds its pitch, velocity, and expression until its next note, so a release tail stays in tune. If the voice is reused during the tail, the tail is cut by the retrigger. *Why hold the pitch:* an envelope's release stage reads the pitch after the gate fell.

25. **More notes than voices steal by the mode.** A chord sequencer with a 5-note chord into 4 voices plays 4 notes and the fifth steals the oldest. An unused voice holds its gate low and its last pitch. *Why:* the same rule as a keyboard, so a source needs no rule of its own.

26. **A note-on and a note-off on the same frame give a one-sample gate.** A note-off followed by a note-on for the same pitch on one frame is a retrigger (rule 21). Events on one frame are applied in the order they arrived. *Why:* every order gives one answer, so the output is deterministic (hard rule 3).

27. **Timing is at the sample.** A note that arrives for frame `i` changes the gate at frame `i`, whatever the block size. Note input already lands MIDI events on their frame. A stored clip reads its notes from the bar phase, so a note that starts between two samples starts on the first sample at or after its time. *Why:* hard rule 2. A gate on its sample is the whole of the note model's timing; there is no event queue to drain.

28. **Per-voice expression (FR-VOICE-04).** A source gives each voice a pitch (0.1 per octave, with bend added), a gate, a velocity (0 to 1), a pressure (0 to 1), and a timbre (0 to 1, the slide dimension). With MPE (FR-MIDI-03), each note's channel carries its own bend, pressure, and timbre, and they reach that voice only. Without MPE, channel bend and channel pressure reach every voice on the channel, and polyphonic key pressure reaches its note's voice. *Why these five:* they are the per-note dimensions MIDI 1.0 and MPE carry. Release velocity is out of scope.

### Cost

29. **A bundle costs one module per voice.** Eight voices of an oscillator, a filter, and an envelope are 24 module runs, as 8 mono chains would be. The NFR-PERF-01 reference patches assume this. SIMD across voices (NFR-PERF-03) is a later engine optimisation that runs copies in lockstep; it needs no change here. *Why not optimise now:* the M2 exit criterion is 8 voices within budget on the reference machine, and per-copy processing is expected to meet it: the [sub-patch modes benchmark](../benchmarks/2026-11-09-sub-patch-modes.md) measured 8 copies of a three-module voice at under 1% of one core. The benchmark of [the shipped example](#the-shipped-example) measures it.

    **How it is measured.** The first reference patch is `examples/patches/polysynth.ron` ([NFR-PERF-01](../product/non-functional-requirements.md#performance-perf)). The second is `examples/patches/polysynth-large.ron`, about 100 modules at 8 voices ([the large reference patch](#the-large-reference-patch)), measured the same way. The cost is the engine's mean time per block, with 8 notes held, at 48 kHz and 128 frames, as a share of the block's real time (2.67 ms). The app's UI is not part of it, so the number needs no display. An app run on #643 stays a separate sanity check. Anyone can run the benchmark and get a number for their own machine. Only the number on the reference machine counts against the budget, and a person checks it (Manual QA: M2, [#643](https://github.com/brylie/LoomDSP/issues/643)). *Why the mean:* the budget is a share of a core over time, which is what leaves room for the host and other plugins. Missing the deadline of a single block is a dropout, which the real-time rules (NFR-RT) and the M2 soak tests cover.

30. **The compile report lists the width of every output,** voices and sides, so the canvas and the inspector can show it ([stereo](stereo.md), rule 19).

## Voice modules (M2)

The [rules for M2 modules](p2-modules.md#rules-for-all-modules-here) apply.

### Note input (MT-IO-03)

`loom.midi.note` gains voices. Its existing ports keep their names.

| | |
|---|---|
| Inputs | `level` (modulation, `Channels::Voices`, default 0): the loudness of each voice, for Quietest stealing. The module reads every voice of it in one instance (rule 4), so one allocator compares them. |
| Outputs | `pitch`, `gate`, `velocity` as today, and `pressure`, `timbre` (modulation, 0 to 1). All `Channels::Voices`. |
| Parameters | `channel` as today. `voices`: 1 to 16, default 1 (rule 12). `mode`: Oldest, Quietest, Legato, Retrigger (rule 20; the default is Oldest above one voice and Legato at one). `bend-range` as today. |
| Panel | `voices` as a number. `mode` as a segmented control showing the choices the count uses. |

- With `voices` at 1 in Legato mode the module behaves exactly as it does today: last-note priority, and the gate high while any note is held. *Why:* existing patches and reference renders do not change.
- Notes from several MIDI channels (`channel` at 0) share the voices. Which channel a voice came from is not an output; a patch that needs channels apart uses one Note input per channel. Routing by channel inside a note effect (MT-NOTE-07) is specified with that module.
- `pressure` and `timbre` are the catalogue's Expression input (MT-IO-05), on this module rather than a separate one. *Why:* expression has to reach the voice that plays its note, and only the module that allocated the voice knows which that is.
- MPE: the module follows the MPE zone on its channel when the device announces one, and each member channel's bend, pressure, and timbre reach the voice playing that channel's note.

### Note input decisions for #674 (Agreed)

- A note is identified by MIDI input, channel, and pitch. Input 0 is a distinct
  source too. Each input's real note-offs reach modules; disconnect synthesizes
  that input's tagged note-offs. Repeated note-ons retrigger the same identity.
- Output width is the module's capacity. Modes resolve without rewriting the
  stored value: Oldest or Quietest at one voice acts as Legato; Legato or
  Retrigger above one voice acts as Oldest. Returning to a held key on note-off
  does not retrigger. Released voices freeze pitch, velocity, and expression.
- Editing the count across one voice switches the mode to Legato or Oldest,
  respectively, in the same undo step. The control channel uses this same edit
  path. A mode edit alone may select an incompatible stored mode; processing
  uses the fallback above and the controls show its effective choice.
- Choice filtering is limited to one integer parameter being one or above one.
  Choice IDs stay stable, and rebuilding the panel follows that integer edit.
- Channel 0 listens to every channel and both announced MPE zones. Channel 1
  includes the active lower zone; channel 16 includes the active upper zone.
  Other settings select exactly that channel. Input 0 never learns an MPE zone.
- An MPE input starts with no zone until RPN 6 announces one. The table belongs
  to the rig's MIDI ingress and outlives patch sessions. New, Open, a new Note
  input, and a module becoming scheduled again see current configuration.
  Disconnect or disabling an input clears it. A new connection starts empty;
  a rapid replug that polling cannot distinguish retains the existing identity.
  If the controller announced before the app opened its input, toggle its MPE
  mode or reconnect it to announce again. There is no manual zone control.
- The engine uses the same pure RPN decoder as ingress and exposes configuration
  at each event's sample, including inside feedback loops. Manager bend starts
  at 2 semitones and member bend at 48. RPN 0 replaces the relevant sensitivity;
  a member's update applies to that zone's members. Manager and member bends
  add; notes on the manager apply its bend once. RPN 6 value zero disables a
  zone, and a newer conflicting zone announcement takes precedence.
- A zone change releases notes and resets expression controls on channels entering
  or leaving MPE control, following RP-053 section 2.1.4. Unaffected channels keep
  playing. This is separate from disconnect releases, whose final values hold.
- Manager and member pressure and timbre combine by maximum. Ordinary channel
  pressure and CC74 update every held voice on that input/channel. Polyphonic
  key pressure updates its note. CC74 remains available for MIDI mappings too.

### Voice merge (MT-VOICE-01)

`loom.voice.merge`: one signal from a bundle.

| | |
|---|---|
| Inputs | `in` (any type, `Channels::Voices`), `gate` (gate, `Channels::Voices`, default 1): which voices are playing, for the Last, Lowest, and Highest modes. |
| Outputs | `out` (one voice; stereo if `in` is stereo), `gate` (gate, one voice; stereo with two equal sides when `in` is stereo): whether the voice `out` follows is playing; in Sum and Average, whether any voice is. Its description: "High while the voice Out follows is playing. In Sum and Average, while any voice plays." |
| Parameters | `mode`: Sum (default), Average, Last, Lowest, Highest |
| Panel | A segmented control for `mode`. |

- **Sum** adds the voices. **Average** divides the sum by the voice count. *Why both:* a sum is right for audio, and an average is right for a modulation signal that should stay in range.
- **Last**, **Lowest**, and **Highest** pass one voice through: the voice whose gate rose most recently, or the playing voice with the lowest or highest `in` value. With `gate` unconnected its default of 1 makes every voice count as playing, and Last passes voice 1 until a gate edge arrives. The chosen voice changes only on a gate edge, so the output does not jump between voices while they sustain. *Why `in` and not pitch:* the module does not know what `in` is; the player patches the pitch bundle to get the lowest note.
- A one-voice `in` passes through unchanged in every mode.
- **The `gate` output** lets a patch play the voice `out` follows, with its own envelope ([#793](https://github.com/brylie/LoomDSP/issues/793)):
  - In **Last**, **Lowest**, and **Highest** it carries the `gate` input of the chosen voice. When the choice moves from one held voice to another (the lowest key is released while others are held), it stays high and does not retrigger. When every voice is low, it is low, and `out` keeps the last choice, as above. *Why legato:* this is what Note input does at one voice in its default mode, so a bass line taken from a chord plays like one played on a mono keyboard.
  - In **Sum** and **Average** it is the largest `gate` value among the voices, so it is high while any voice plays. *Why:* no single voice is chosen, and "any voice playing" is what a gate after a sum means.
  - With the `gate` input unconnected, every voice reads its normal of 1, so the output is 1 in every mode.
  - A one-voice bundle passes its gate through.
  - The value is passed as it arrives, not turned into 0 or 1: the module does not judge a gate, as it does not judge `in`. A stereo voice's two gate sides combine by maximum, as for the selection.
- **A value that is not a number, or is infinite,** never wins Lowest or Highest over a finite one ([#794](https://github.com/brylie/LoomDSP/issues/794)). A playing voice whose `in` is NaN or infinite (on either side, for a stereo voice) ranks below every playing voice with a finite value, wherever it sits in the bundle. When every playing voice is non-finite, the tie rule below picks the lowest voice index, and its value passes through unchanged, because cables are never cleaned ([ADR-0007](../adr/0007-polymorphic-signal-model.md)); if it reaches Audio Out, the engine's output stage turns it into 0. Sum, Average, and Last do not compare values, so they do not change. *Why:* one broken voice should not hold the choice while other voices carry real notes.

Selection decisions for #675's Voice merge (Agreed): changing into Last, Lowest,
or Highest reselects an active voice immediately, including effective mode changes
from modulation. Removing the selected voice reselects among surviving active
voices; a selection still in range is preserved across width changes. All-low gates keep the last
selection, initially voice 1. Ties choose the lowest voice index. Stereo voices
are compared by their mean and both sides move together. Missing gate voices
read the declared normal, including beyond a narrower bundle. Within each voice,
stereo gate sides combine by maximum and stereo Level sides by mean, following
the control-signal conventions of the stereo specification.

How the `gate` output and the non-finite ranking are built (technical design, 2026-11-15, [#793](https://github.com/brylie/LoomDSP/issues/793) and [#794](https://github.com/brylie/LoomDSP/issues/794)):

- **Port type.** `gate` is the second output, after `out`, so `out` keeps its index: `SignalType::Gate`, `PortDefault::Value(0.0)`, `PortRange::Signal`, `Channels::PerChannel`. Its index is the associated constant `GATE_OUT`, from `MERGE.output_index("gate")`. In the same change the input constant `GATE` becomes `GATE_IN`, so the two cannot be confused (AGENTS.md: inputs end in `_IN`, outputs in `_OUT`); `IN` and `OUT` already follow the rule. *Why Gate:* a cable from it is re-patched as a gate, so it switches and does not ramp ([live patching](live-patching.md), rule 9).
- **Its width.** It is one voice. Its stereo sides follow rule 5: every `PerChannel` output of a bundle-aware module takes its sides from the first `Voices` input, here `in`. So a stereo `in` makes `gate` stereo too, and the module writes the same value on both sides. No `Channels` value gives an always-mono output here: `Downmix` is refused on an output, `Stereo` is always two sides, and `Voices` needs a `voices` parameter. *Why accept two equal sides:* every gate input reads the same value from it. A `Downmix` gate input takes the maximum of the two sides ([stereo](stereo.md), rule 11), which is that value. A `PerChannel` input runs one copy per side, and both copies read that value. Voice merge combines gate sides by the maximum. A rule just for this port would be a special case in the width pass (`crates/loom-core/src/width.rs`, `outputs`). The `loom-test` harness lays out outputs by the same rule, so its tests see the same widths.
- **Which voices count.** The voices are those of `in`, from `input_width(IN).voices`, as for `out`. A gate voice past `in`'s last voice is not read. A gate bundle narrower than `in` (and wider than one voice) pads with the `gate` normal of 1, as the selection already does, so those voices count as playing in every mode.
- **Each frame,** after the selection step and for each side of `gate`'s output width:
  - Last, Lowest, Highest: `max(left, right)` of the `gate` input of the voice now selected, the same value the selection read. When the choice moves on a gate edge, the new choice is made on that frame before the output is written. So if another voice is high, the output is high on the frame the old one falls, and never has a low sample. When every voice is low, the choice holds and its gate is low.
  - Sum, Average: the largest value over the voices and their sides, folded with `f32::max` from voice 1's left side. `f32::max` returns the other operand when one is NaN, so a NaN gate on one voice does not hide the others. If every value is NaN, the output is NaN, passed as it arrived.
  - No state is added: the output is a function of this frame's inputs and the selection, so it is the same at every block size, and `reset` and `copy_state_from` are unchanged.
- **The non-finite ranking (#794).** In the selection loop, Lowest and Highest compare with one helper, `better(mode, candidate, previous) -> bool`, on the values `Merge::value` returns. A non-finite candidate is never better. A finite candidate is always better than a non-finite previous. Two finite values compare as today, with the strict `<` or `>`, so a tie keeps the lower voice index. `Merge::value` is the mean `left * 0.5 + right * 0.5`, which is finite exactly when both sides are finite: each half of a finite value is at most `f32::MAX / 2`, so the sum cannot overflow. So one `is_finite` test on the mean is the "either side" rule. The shared stereo-mean helper (see Voice assigner, Stereo inputs) keeps exactly this form. *Why:* `(left + right) / 2` overflows to infinity for two large finite values of one sign, which would break this argument, and it rounds differently, which would change today's reference renders. When every high voice is non-finite, no later voice is better than the first one, so the lowest index wins, and `out` passes its value unchanged.
- **The ranking runs when a choice is made,** on a gate edge or a mode change, like the comparison it replaces. A chosen voice whose value turns non-finite while the gates hold keeps the choice until the next edge, as a chosen voice whose value moves keeps it today. *Why:* the choice changes only on a gate edge (above), so the output does not jump between voices while they sustain.
- **Tests,** in `crates/loom-modules/tests/voice_merge.rs`, with `check_block_sizes` (block sizes 1, 77, and 512): `gate` in each of the five modes; the hand-off from the lowest held voice to the next with no low sample on the hand-off frame; low when every gate is low; 1 with `gate` unpatched; a one-voice `in` passing its gate; a stereo `in` giving equal `gate` sides. For #794, voice 1 of 4 holds a non-finite value with all gates high, and Lowest and Highest must pick the right finite voice: NaN in both modes, minus infinity in Lowest, and plus infinity in Highest. Each of these fails on `main`: a first NaN voice is never replaced, and an infinity of the winning sign wins every comparison. All four voices NaN gives voice 1, with its NaN on `out`. Where a finite voice wins, every output is finite, so those cases use `check_block_sizes`. Only the all-NaN case has a NaN on `out`, and `check_block_sizes` fails any non-finite output, so it renders with `Harness::render_with_block` for each of `loom_test::BLOCK_SIZES` and compares the bits, then checks that `gate` is finite and `out` is NaN. `check_non_finite_recovery` and the reference render cover the new output too: `tests/reference.rs` renders `gate` beside `out`.

### Voice split (MT-VOICE-02)

`loom.voice.split`: the voices of a bundle as separate outputs.

| | |
|---|---|
| Inputs | `in` (any type, `Channels::Voices`) |
| Outputs | `out1` to `out8` (one voice each) |
| Parameters | `first`: 1 to 16, default 1 |
| Panel | `first` as a number. |

- `out1` is voice `first`, `out2` is voice `first + 1`, and so on. An output past the bundle's last voice holds the input's default. *Why eight outputs:* a tile with sixteen ports is unusable; a second Split with `first` at 9 reaches the rest.
- The catalogue's "copies one signal to every voice" needs no module (rule 6). "Sets a fixed value per voice" is Voice spread.

How it is built (technical design, 2026-11-14, [#675](https://github.com/brylie/LoomDSP/issues/675)):

- **Port types.** `in`: `SignalType::Audio`, `Channels::Voices`, default 0. `out1` to `out8`: `SignalType::Audio`, `Channels::PerChannel`, one voice each. Their stereo sides follow rule 5: a bundle-aware module's `PerChannel` outputs take their sides from its first `Voices` input, here `in`. So a stereo bundle gives stereo outputs, with no stereo copies. The module loops each output's sides over `output_width(OUT).sides`.
- **`first`** is a whole-number parameter (`ParamSpec::integer`), 1 to 16, default 1. It does not shape the graph: the outputs are one voice each whatever it is set to. So it can be modulated, as other whole-number parameters can ([modulation](modulation.md), rule 5: the engine rounds after the clamp). The module reads it per frame with `Param::map_or`, so a modulated `first` moves the outputs on the sample, with no smoothing and no crossfade. A whole-number parameter reports no glide ([modulation](modulation.md), rule 9), so `param_glide` is the trait default. *Why it switches:* it picks which voices reach the outputs, a choice rather than a level.
- **Each frame:** `outK` reads the zero-based voice `first - 1 + (K - 1)`, computed with saturating arithmetic (`saturating_sub`, `saturating_add`), so a bad value cannot wrap. Before any `voice_input` call, the module compares that voice with `input_width(IN).voices`. At or past it, the output writes the port's declared default, never a literal (ADR-0043), and `voice_input` is not called. *Why the check comes first:* `PortIo::voice_input` fans a one-voice cable out to every voice, as rule 6 does for copies, so calling it for voice 2 of a mono cable would return voice 1. With the check, a one-voice `in` is one voice: `out1` carries it when `first` is 1, and the other outputs hold the default. An unpatched `in` reads its port default, zeros, as one voice, so every output is 0. `first` at 2 on a mono cable gives the default on every output, and `first` at 16 on a 16-voice bundle gives voice 16 on `out1` and the default on `out2` to `out8`. *Why not feed every output:* a split's outputs name voices, and a mono cable has only voice 1.
- No state, so `copy_state_from` and `reset` are the trait defaults.

### Voice spread (MT-VOICE-03)

`loom.voice.spread`: a different value per voice from one control.

| | |
|---|---|
| Inputs | `in` (any type, `PerChannel`, default 0) |
| Outputs | `out` |
| Parameters | `amount`: -10 to 10, default 0. `mode`: Even (default), Series, Random. `seed` ([M2 modules](p2-modules.md), rule 1) |
| Panel | A knob for `amount`, a segmented control for `mode`. `seed` in the inspector only, and shown only in Random, the one mode that uses it. |

- The module runs once per voice (rule 4) and reads its voice `i`, from 1 to `V`, and the count `V` (rule 17).
- **Even:** `out = in + amount × offset`, with offsets evenly spaced from -1 to 1 (voice 1 at -1, voice V at +1, one voice at 0 when V is odd). This is the stack's spread formula ([sub-patches](sub-patches.md#modes), TR-04). **Series:** `out = in + amount × (i - 1) / (V - 1)`, from 0 up to `amount`. **Random:** `out = in + amount × r_i`, where `r_i` is a draw from -1 to 1 from the seed and the voice index.
- With one voice, `out = in`. *Why:* a spread of nothing is nothing, so the module can sit in a patch before it is polyphonic.
- **What changes the offsets.** Random's offsets are set by the seed: the same seed gives the same offsets on every load and at every block size, and a voice keeps its offset when the voice count changes, as long as the count stays above 1 (one voice always gives `out = in`). Even and Series space their offsets by the count, so changing the count re-spaces them. *Why:* a player who raises the count from 4 to 8 should not hear the four voices already tuned move.

How it is built (technical design, 2026-11-14, [#675](https://github.com/brylie/LoomDSP/issues/675)):

- **Port types.** `in`: `SignalType::Modulation`, `Channels::PerChannel`, default 0. `out`: `SignalType::Modulation`, `Channels::PerChannel`. *Why Modulation:* the module adds an offset to any signal, as Add does.
- **`amount`** is smoothed over 5 ms (`.smooth(0.005)`, a `loom_rt::Smoothed`) and implements `param_glide`, as Add's `offset` does. Its [control ranges](control-ranges.md) are -0.01 to 0.01, -0.1 to 0.1, -1 to 1, and -10 to 10, and a new Voice spread starts on -0.1 to 0.1. *Why the finer range:* a detune of 10 cents is about 0.00083, which is 0.4% of a -0.1 to 0.1 knob; on -0.01 to 0.01 a 1% arrow-key step is 2.4 cents, and the Shift fine steps go further: Shift with an arrow key moves 0.1% (0.24 cents), and Shift-drag is 10 times finer than a drag ([panels](panels.md)). *Why -0.1 to 0.1 first:* it is ±1 octave on a pitch cable, so a Spread just added to a pitch path stays safe. The smoother ticks every frame in every mode, so a glide takes the same time at any block size.
- **`mode`** has three choices, Even (default), Series, and Random, shown as a segmented control. The module turns its value into a `Mode` with a `match` that has a fallback arm, and never uses it as an array index, so a value out of range cannot panic.
- **Offsets,** for voice `i` of `V` (`ProcessContext::voice` and `voice_count`). When V is 1, every mode gives 0. Otherwise:
  - Even: `loom_dsp::blend::spread_offset(i - 1, V)`, the function the stack's spread uses, so the two agree bit for bit.
  - Series: `(i - 1) / (V - 1)`.
  - Random: `2 × d - 1`, where `d` is the first draw of `crate::rand::stream(seed, (i - 1) × 2, N)`: an `Rng` from `loom_dsp::random`, started from the seed, the voice with its side bit at 0, and a stream number `N` that no other module uses. `N` is a named constant beside `stream` in `rand.rs`, with a comment that lists the numbers already taken (Random's outputs, Chance's lanes, and Selector all start at 0). *Why its own number:* `stream(seed, ch, 0)` is already Random's first output and Selector's stream, so at the same seed Voice spread's offsets would repeat their first values. It lies in [-1, 1). It depends on the seed and the voice, not on the side ([ADR-0045](../adr/0045-a-seeded-value-of-a-voice-is-the-same-on-both-sides.md)). Its only dependence on the count is the 0 at one voice.
- **`seed`** is the shared seed parameter (`seed_param`, [M2 modules](p2-modules.md), rule 1). It is shown only while `mode` is Random (`ParamOptions::shown_when`, ADR-0030), the one mode that reads it. The module has no `reset` input: [M2 modules](p2-modules.md) rule 2 does not apply, because the offsets are one draw per voice, not a sequence that moves on.
- **When the offsets are computed.** The three offsets are cached with the voice, the count, and the rounded seed they came from. The cache starts at an impossible voice, so the first call fills it, and it is filled again when any of the three changes. With a modulated seed, the rounded seed is compared per frame and a new draw is made only when it changes. So the per-sample path normally reads one cached value. `mode` can be modulated, so it picks among the three per frame.
- **Each frame:** an offset of exactly 0 writes `in` unchanged (one voice, or Even's middle voice when V is odd), so `out = in` holds bit for bit. Otherwise `out = hold_finite(in + amount × offset)` for a finite `in` (ADR-0037), and the plain sum for a non-finite one.
- **Count changes.** Even and Series offsets step to their new spacing on the swap frame, with no glide: a count change is an edit (rule 15). Random offsets do not move while the count stays above 1.
- **Stereo.** `in` is `PerChannel`, so a stereo voice runs as two copies, and both draw the same offset (ADR-0045). A new right side takes its left side's state as it is (`loom_core::copy_state`). A stereo modulation source assigned to `amount` or `seed` still gives each side its own value ([stereo](stereo.md), rule 17), as it does on Selector.

How Voice split and Voice spread are tested (technical design, 2026-11-15, [#675](https://github.com/brylie/LoomDSP/issues/675)):

- **The harness sets the voice and the side.** `Harness::voice(i, count)` and `Harness::channel(c)` set the voice and the side a module runs as, in every render and check. Without them a Voice spread would run as voice 1 of 1, where every offset is 0, which would hide a formula bug. A module with no `Voices` port runs through `loom_core::testing::run_with_context` (`crates/loom-core/src/module.rs`), which takes a `ProcessContext` built with `ProcessContext::with_voice` and `with_channel`. The default is voice 1 of 1 on side 0, so tests that set neither run as before.
- **Per-voice and per-side claims** are tested through those setters, at block sizes 1, 77, and 512, or through an engine test.
- **The engine test for ADR-0045** feeds one 8-voice `test.voices` source into both inputs of Join (`loom.mix.join`), so the stereo bundle has equal sides. It runs that through Voice spread in Random mode with a non-zero `amount`, then through Voice split. For each voice it asserts that `out - in` is not zero, that it differs from the other voices' `out - in`, and that it is the same on both sides. *Why `out - in`:* `test.voices` already gives each voice a different value, so "the voices differ" would pass with every offset at 0.

### Voice assigner (MT-VOICE-04)

`loom.voice.assign`: notes from any source, re-allocated to this module's own voices.

| | |
|---|---|
| Inputs | `pitch` (pitch, `Channels::Voices`), `gate` (gate, `Channels::Voices`, default 0), `velocity` (modulation, `Channels::Voices`, default 1), `pressure` and `timbre` (modulation, `Channels::Voices`, default 0), `level` (modulation, `Channels::Voices`, default 0) |
| Outputs | `pitch`, `gate`, `velocity`, `pressure`, `timbre` (`Channels::Voices`) |
| Parameters | `voices`: 1 to 16, default 1. `mode` (rule 20). `unison`: 1 to 4, default 1. |
| Panel | `voices` as a number, `mode` as a segmented control, `unison` as a number, shown only in the modes that use it. A `gate` light, as on Note input. |

- Each rising edge on an input voice's `gate` is a note-on with that voice's `pitch` and `velocity`; each falling edge is its note-off. The allocator (rule 18) gives it an output voice. While the note sounds, its output voice's `pitch`, `velocity`, `pressure`, and `timbre` follow the input voice it came from, so expression moves with the note; after the note-off they hold (rule 24). *Why:* a mono sequencer into an 8-voice synth gets release tails that overlap, and a 4-voice source into 8 voices gets more.
- **Unison** gives each note `unison` output voices, taken, retriggered, and released together (rule 22). The voices then differ only by what Voice spread adds after the module. *Why:* unison is a spread of copies of one note, which is what the catalogue asks of this type.
- A module with both `Voices` inputs and `Voices` outputs runs as one instance (rule 3).

What the player gets (Agreed 2026-11-16, [#676](https://github.com/brylie/LoomDSP/issues/676)):

- **A note is an input voice.** Each input voice of `gate` carries at most one note at a time, so the module knows a note by the input voice it arrives on, not by its pitch (rule 22). Expression then follows exactly the input voice the note came from. Two input voices on the same pitch are two notes on two output voices, as two MPE member channels are on Note input. *Why not by pitch:* a source upstream already decided these are separate notes; merging them by pitch would cut one off.
- **The pitch follows while the gate stays high.** When an input voice's `pitch` moves while its `gate` stays high (a glide, or a mono source playing legato), its output voice's `pitch` follows, with no new note and no retrigger. Only a rising edge on `gate` is a new note. *Why:* this is what "follows the input voice while the note sounds" means, and it keeps a glide smooth.
- **A stolen note stays silent.** When a note's output voice is stolen, the note does not come back while its input gate is still high. Its note-off later changes nothing. Its input voice plays again on its next rising edge. *Why:* a returning note would sound like a new note the player did not play.
- **How many input voices can play.** The number of input voices is the largest width among the five note inputs, `pitch`, `gate`, `velocity`, `pressure`, and `timbre`, as rule 4 takes the largest voice count for a copied module. `level` does not count, because it is indexed by output voice. The input count does not have to match `voices`. Inside it, rule 6 applies: a mono cable feeds every input voice, and a narrower one pads with its default. So:
  - a mono `pitch` and a mono `gate` give one note on each rising edge, on rotating voices;
  - a 4-voice `pitch` bundle with a mono `gate` gives a 4-note chord on each edge, as the same two cables into an oscillator give 4 voices;
  - with no cable on `gate`, every input voice reads its default of 0, so nothing plays;
  - a 16-voice input into 4 voices steals by the mode.

  *Why the largest width:* it is the rule every other module follows, so the same two cables mean the same number of notes wherever they go. Input voices past that width do not exist, so a mono patch never becomes 16 notes on one edge.
- **Unison larger than the voice count** is clamped: a note takes at most `voices` voices. With 8 voices and `unison` 3, two notes sound together (6 voices), and a third steals by the mode; the 2 voices left over stay free. *Why:* the allocator takes whole groups (see [Allocator interface](#allocator-interface-implemented-673)), so a note is never split.
- **`level` is by output voice.** For Quietest, voice *k* of `level` is the loudness of output voice *k*: the player patches it back from the envelopes after the module. With no cable on `level`, Quietest acts as Oldest (rule 20), even though `level` has a default of 0. *Why:* the module steals output voices, so it needs their loudness, not that of its input.
- **At one voice,** the module is a mono source with last-note priority across its input voices, in Legato or Retrigger (rule 20). Releasing the newest note returns to the one held before it, without a retrigger, as on Note input.
- **`unison` is hidden where it does nothing** (ADR-0030). It shows in Oldest and Quietest and hides in Legato and Retrigger, the one-voice modes. The hiding follows the mode the module plays, the one the **Mode** control shows, not the stored value: at 8 voices with a stored Legato, the module plays Oldest (see [Note input decisions](#note-input-decisions-for-674-agreed)), so `unison` shows. *Why:* a control that hides while it still changes the sound would mislead the player. *Why hide it at all:* one voice cannot take two voices, so the control would do nothing there.
- **The `gate` light** is lit while any output voice is high, with the words "Lit while a note is held." *Why "note":* the notes can come from any source, not only a keyboard. *Why the light:* the player can see at a glance why nothing plays: no cable on `gate`, or a stolen note.
- **Words the player reads** (descriptor labels and descriptions, ADR-0016; they can be reworded later):

  | Item | Description |
  |---|---|
  | Module | Gives notes from any source to this module's own voices, with unison. |
  | `gate` input | Each rising edge starts a note on this input voice. Nothing plays without a cable here. |
  | `level` input | The loudness of each output voice, patched back from its envelope, for Quietest stealing. |
  | `gate` output | High while this voice holds a note. |
  | `unison` | How many voices each note plays. |
  | `gate` light | Lit while a note is held. |

How it is built (technical design, 2026-11-15, [#676](https://github.com/brylie/LoomDSP/issues/676)):

- **Where.** `ASSIGN: ModuleDescriptor` and its `Assign` state live in `crates/loom-modules/src/voice.rs`, beside Merge, Split, and Spread, and the registry lists it with them (`crates/loom-modules/src/lib.rs`). Every port and parameter index is an associated constant from `ASSIGN.input_index`, `output_index`, and `param_index`, named `PITCH_IN`, `GATE_IN`, and so on, and `PITCH_OUT`, `GATE_OUT`, and so on.
- **Ports.** Every port is `Channels::Voices`, `PortRange` as on Note input's matching output:

  | Port | Signal | Default | Range |
  |---|---|---|---|
  | in `pitch` | Pitch | 0 | Signal |
  | in `gate` | Gate | 0 | Signal |
  | in `velocity` | Modulation | 1 | Span 0 to 1 |
  | in `pressure`, `timbre` | Modulation | 0 | Span 0 to 1 |
  | in `level` | Modulation | 0 | Signal |
  | out `pitch` | Pitch | 0 | Signal |
  | out `gate` | Gate | 0 | Signal |
  | out `velocity`, `pressure`, `timbre` | Modulation | 0 | Span 0 to 1 |

  The registry accepts this: every input is `Voices` (rule 4), and the `Voices` outputs have a structural `voices` parameter from 1 to 16 (the [structural edit rules](#structural-parameter-edits-implemented-672)). The width pass treats any module with a `Voices` port as one instance, so the assigner is never copied per voice or per side (rules 3 and 4). Its `Voices` outputs are mono by rule 5, so each output voice has one side.
- **Parameters.** `voices` and `mode` are the same `ParamSpec`s as Note input's: `voices` an integer from 1 to 16, default 1, `.shapes_graph()`; `mode` the four choices Oldest, Quietest, Legato, Retrigger, default 2 (Legato, since one voice is the default), with `.choices_by_count("voices", 2)`. Both modules build them from one shared `const fn` in `loom-modules`, with the stored-value-to-`loom_dsp::voice::Mode` mapping beside it, so the two sources cannot drift. Because the filter is generic, the app's count edit (`crates/loom-app/src/modes.rs`) switches the mode to Legato or Oldest when `voices` crosses one, in the same undo step, with no code for this module. `unison` is a `ParamSpec` built with `.integer()`, 1 to 4, default 1. It does not shape the graph, so it can be modulated. The module reads it per frame with `Param::map_or`, and both that value and the one `set_param` gives become a count with `v.round().clamp(1.0, 4.0)`, with NaN taken as the default of 1 before the cast. *Why the NaN case:* `NaN.round().clamp(..)` is still NaN, and the cast to `usize` would quietly give 0. The engine never sends NaN (it fills a modulated value through `ParamSpec::denormalize`), but a direct `set_param` can. A whole-number parameter has no glide, so `param_glide` is the trait default.
- **Panel.** `Panel::new(12, 6, …)`. The top two rows are Note input's: `Widget::segmented(0, 0, 12, "mode")`, `Widget::number(0, 2, "voices")`, and `Widget::light(9, 2, "gate")`. `Widget::number(0, 4, "unison")` is on its own row below. A number is 5 by 2 grid units and a light 3 by 2, so voices, unison, and the light take 13 units and do not fit in one row of 12. The widgets are declared in that order (mode, voices, gate, unison), because the accessibility tree reads a tile's controls in the order of the list. *Why this layout:* a player who knows Note input finds Mode, Voices, and the light in the same places. `close_up` (`crates/loom-app/src/scene.rs`) removes grid rows that only hidden widgets cover, so the panel is 12 by 4 at the default (1 voice, Legato) and 12 by 6 at 2 or more voices in Oldest or Quietest. The first two rows never move. Either way, the tile's height is set by its six inputs, so the tile does not change size on the canvas. The segmented control shows the choices for the count (ADR-0030), as on Note input.
- **`unison` hides by the effective mode.** It is declared with `.shown_when("mode", &[0, 1])` (Oldest and Quietest). Today `ModuleDescriptor::is_shown` (`crates/loom-core/src/descriptor.rs`) compares the stored value of `mode`. At 8 voices a stored Legato plays as Oldest, so `unison` would hide while it still works. The change, in `loom-core`:
  - a new `ModuleDescriptor::effective_value(index, values) -> f32`: for a parameter with `count_choices`, the stored value when it lies in `control_spec(index, values)`'s range, and that range's `min` otherwise; for any other parameter, the stored value. This is the rule the inspector (`crates/loom-app/src/inspector.rs`, the `count_choices` value) and the canvas (`crates/loom-app/src/scene.rs`, `control_position`) already apply in their own copies, and both call the new function instead. It also matches the allocator's fallback (`Mode::for_capacity`): Legato or Retrigger above one voice is Oldest, the first choice offered there, and Oldest or Quietest at one voice is Legato.
  - `is_shown` reads the controlling parameter through `effective_value`, then compares with the choice list as now (`value - min` of the unfiltered parameter, since choice IDs are stable).
  - `is_widget_shown` (`crates/loom-core/src/descriptor/panel.rs`) reads a display widget's controlling value through `effective_value` too, so one rule decides for controls and display widgets.
  - The inspector's "Not used in {choice}" text (`crates/loom-app/src/inspector.rs`, the `unused` row) names `choice_name(effective_value(..))`, not the stored value. So a patch saved at 1 voice with a stored Oldest reads "Not used in Legato", the mode the module plays and the Mode control shows.

  Note input declares no `shown_when`, and no other parameter is `shown_when` a choice that has `count_choices` (Voice spread's `seed` follows a `mode` without it), so nothing else changes on screen. A unit test in `crates/loom-core/src/descriptor/tests.rs` covers `effective_value`, `is_shown`, and `is_widget_shown` at both counts, and the app test below checks the cases the player meets.
- **The `gate` light.** A `DisplaySpec` named `gate`, as on Note input: label "Gate", "Lit while a note is held.", `DisplayKind::Level`, `DisplayRange::UNIT`. `Module::displays` writes 1 when any voice in `allocator.voices()` has its gate high and 0 otherwise, once per block from the state after the last frame, as Note input does.
- **The voice count.** The module reads its capacity from `io.output_width(PITCH_OUT).voices` at the start of each call, never from the `voices` value `set_param` gives (structural edit rules). The compiler sets that width from the parameter (`crates/loom-core/src/width.rs`, `outputs`), and a count edit reaches the module as a schedule swap. So the module has no code of its own for the count: it calls `Allocator::configure(capacity, mode, unison)` once per frame, as Note input does, and the allocator keeps or forgets notes as [its interface](#allocator-interface-implemented-673) says. A note on a voice that a lower count removes is forgotten (rule 15). Its input voice stays silent until its next rising edge, as for a stolen note.
- **How many input voices.** Once per call the module reads `n`, the largest `io.input_width(port).voices` over `PITCH_IN`, `GATE_IN`, `VELOCITY_IN`, `PRESSURE_IN`, and `TIMBRE_IN`, not `LEVEL_IN` ("How many input voices can play" above, rule 4). An unpatched input has width 1, so it never widens `n`. Only input voices `0..n` are read. *Why the bound comes first:* `PortIo::voice_input` gives voice 0 of a one-voice cable for every voice index, as rule 6 does for copies. Reading all 16 input voices of a mono pitch and gate would see 16 rising edges on one frame. Inside `0..n`, `voice_input` gives rule 6 with no extra code: a one-voice cable feeds every input voice, and an input voice past a narrower bundle reads the port's normal. So a 4-voice `pitch` with a mono `gate` gives 4 rising edges on one frame, a chord; a mono `pitch` and `gate` give one; and with no `gate` cable every input voice reads 0, so nothing plays.
- **Stereo inputs.** The outputs are mono, so each input voice reads one value from its sides, by the stereo downmix of its signal type ([stereo](stereo.md), rule 11): the maximum for `gate`, the mean for `pitch`, `velocity`, `pressure`, `timbre`, and `level`. One small helper reads a voice this way. Voice merge's `value` and Note input's `level` read already compute this mean, and they use the same helper, which keeps exactly `left * 0.5 + right * 0.5` (see Voice merge for why).
- **State,** all fixed-size, so nothing allocates after `new` (hard rule 1, `tests/no_alloc.rs`):
  - `allocator: loom_dsp::voice::Allocator`;
  - `high: [bool; 16]`, whether each input voice's gate was high on the last frame;
  - `last: [Values; 16]`, each input voice's latest finite `pitch`, `velocity`, `pressure`, and `timbre`, starting from the ports' declared defaults (read from `ASSIGN`, as Voice split reads its `DEFAULT`, never written as literals);
  - the stored `mode` and `unison` from `set_param`.
- **A note's identity** is `NoteId(v)` for input voice `v`, from 0 to 15. One input voice carries one gate, so it holds at most one note at a time. A gate cannot fall and rise on one frame, so the allocator's same-frame retrigger by identity never applies here.
- **Each frame,** in this order:
  1. Read `mode` and `unison` (modulated or stored), and call `configure(capacity, mode, unison)`.
  2. Read `level` for output voices `0..capacity` into a `[f32; 16]`, and call `begin_frame(Some(&levels[..capacity]))` when `level` reads a cable (`!io.uses_default(LEVEL_IN)`, as Note input does), else `begin_frame(None)`, so Quietest acts as Oldest. A mono `level` cable gives every output voice the same level, so Quietest then ties and falls back to Oldest by the allocator's tie rule. A narrower `level` bundle pads its missing voices with the default 0, which reads as quietest.
  3. Read each input voice's gate for `v` in `0..n`, high when `signal::gate_is_high` (above 0.5; NaN is low). Compare with `high[v]` to find the falls and the rises. On the first frame of a call, every `v` from `n` to 15 whose `high[v]` is set is a fall too, so when `n` shrinks (a narrower or removed cable on any of the five note inputs), the notes of the input voices it drops are released, not held for ever.
  4. Call `note_off(NoteId(v))` for every fall, in rising `v`.
  5. Call `note_on` for every rise, in rising `v`, with the input voice's values on this frame.
  6. Call `update_note` for every input voice whose gate is high, with its values on this frame. The allocator changes only voices that still hold that note, so a stolen note's input changes nothing (see "A stolen note stays silent" above).
  7. Write output voices `0..capacity` from `allocator.voices()`: `gate` 1 or 0; `pitch`, `velocity`, `pressure`, and `timbre` from the voice's last note. A voice that has never had a note writes what Note input writes for one (pitch -0.5, the rest 0), from one shared constant, so the two sources agree on an unused voice.

  *Why note-offs first:* the input voices of one frame have no order of arrival, so rule 26 does not order them. With the note-offs first, a note that ends frees its voice before a note that starts on the same frame looks for one. A full assigner then does not steal a held note while another is ending. The new note takes the freed voice with a one-sample low gate first, as the allocator does for any voice freed on its own frame (rule 21). *Why rising `v`:* it is deterministic and the same at every block size (hard rules 2 and 3).
- **Values a note takes.** Steps 5 and 6 take each value from `last[v]`, after updating it with this frame's value only when that value is finite. *Why:* a released voice holds its values until its next note (rule 24). A NaN on `pitch` at its note-off would otherwise stay on that output voice until the voice is reused, which breaks the harness rule that a module must not keep a non-finite input in its state ([module test harness](module-test-harness.md), rule 8). This is how Sample and hold treats a non-finite input. `level` needs no guard: the allocator reads a non-finite level as 0. A non-finite `gate` is low.
- **At one voice,** the allocator keeps its held list of up to 16 notes, so releasing the newest input voice returns to the one before it without a retrigger (rule 20). Step 6 then updates the returned note from its own input voice, velocity included. So a returned note plays at its own input velocity, not at the velocity of the last note-on as on Note input. *Why:* on this module every value follows the input voice the note came from ("A note is an input voice").
- **`unison` changes** apply from the next note-on; the voices a note already holds stay its own until it ends. The allocator clamps `unison` to the voice count and takes whole groups, so a count that `unison` does not divide leaves its remainder free.
- **`reset`** resets the allocator, clears `high`, and puts `last` back to the port defaults. It keeps `mode` and `unison`. **`copy_state_from`** uses `loom_core::copy_state`, as Note input and Voice merge do. The engine never copies this module, but the method keeps the harness's `check_copy_state` meaningful and follows the rule for every module with state. **`param_glide`** is the trait default (above).
- **The `level` loop.** Cabling the envelopes after the assigner back to its `level` closes a feedback loop through every module between, which then runs one sample at a time. The compiler inserts its one-sample delay by the same rule as for Note input, so it may land on another cable ([#733](https://github.com/brylie/LoomDSP/issues/733)).
- **Normals (ADR-0043).** No input is switched. An unpatched `velocity` reads 1, an unpatched `gate` reads 0, and an unpatched `level` makes Quietest act as Oldest, which a cable of zeros also does by the tie rule. So `tests/normals.rs` checks every input with no exception.

How it is tested (technical design, 2026-11-15, [#676](https://github.com/brylie/LoomDSP/issues/676)):

- **The harness at a count.** `Harness::param("voices", 8.0)` sets the output width, and `bundle_input` sets an input bundle (`crates/loom-test/src/lib.rs`, `output_layout`), so the module tests run through `check_block_sizes` at block sizes 1, 77, and 512 without an engine. `Render::voice_output` reads one output voice.
- **New file** `crates/loom-modules/tests/voice_assign.rs`, one test per open acceptance criterion: a mono gate and pitch into 8 voices (distinct output voices, tails that overlap); `unison` 2 (two voices rise together, both fall on the note-off, a pressure change on the input voice reaches both); a pitch glide with the gate held (pitch follows, gate stays high on every frame); two input voices on one pitch (two output voices high); 8 voices with `unison` 3 (two notes sound, a third steals by the mode, two voices stay free); Quietest with a `level` bundle in which the quietest voice is not the oldest, so Oldest would steal another voice, and with `level` unpatched; a mono gate into 8 voices (one rising output gate per input edge, never more), a 4-voice `pitch` with a mono `gate` (4 output gates rise on one edge), and 16 input voices into 4 output voices; a note stolen while its input gate stays high (silent until its input gate rises again). And for the behaviour above:
  - **Note-offs before note-ons.** At full capacity, on one frame, an input voice at a lower index rises while one at a higher index falls, and the falling note is not the oldest. The new note must take the ending note's voice, and the oldest note must keep sounding. Processed in one pass by rising index, the rise would come first and steal the oldest, so the test fails without the order.
  - **NaN does not stick.** NaN on an input voice's `pitch` on every frame up to and including its last high gate frame, then the gate falls and `pitch` is finite again. The released output voice's `pitch` must be finite after the fall, and equal to the last finite value before the NaN. Without the guard, step 6 writes the NaN on the last high frame and the released voice holds it. A NaN that ends before the fall would pass without the guard, because step 6 would overwrite it.
  - **`unison` from a direct `set_param`**, in `tests/non_finite.rs` beside the input checks: NaN gives 1 voice per note, minus infinity 1, and plus infinity 4, with no panic and finite output.

  Each test fails without the behaviour it names: for example, the mono-gate test with all 16 input voices read sees 8 output gates rise on the first edge.
- **Engine test for a shrinking input count** in `crates/loom-modules/tests/polyphony.rs`, because the harness keeps one bundle width for a whole render: an 8-voice `test.voices` source into `pitch` and `gate` with all 8 held, then the cables swapped for 4-voice ones. Output voices holding the notes of input voices 5 to 8 must fall on the swap frame, and the other four stay high.
- **Reference render** in `tests/reference.rs`: a mono pitch and gate stepping through 6 notes into 8 voices, so the overlap of release tails is in the file. The module joins the list in `every_dsp_module_has_a_reference_test` there.
- **Shared suites** pick it up from `loom_modules::ALL` and the registry with no edit: `tests/no_alloc.rs`, `tests/normals.rs`, `tests/param_glide.rs`, `tests/non_finite.rs`, `tests/panels.rs`, and the registry's descriptor and panel checks. These run it at its default of one voice, so `tests/no_alloc.rs` also gets one engine case at 8 voices, `unison` 2, with an 8-voice Note input into it, a count edit from 8 to 4, and more notes than voices, so stealing runs too.
- **App and screen reader** (`crates/loom-app/src/polysynth_tests.rs` style, with the `Ui` harness): the assigner's tile shows `unison` at 8 voices in Oldest, hides it at 1 voice in Legato, and shows it at 8 voices with a stored Legato; a screen reader finds its Mode, Voices, and Unison controls and the Gate light by name. While `unison` is hidden, the tile's Unison control is not in the tree and Tab skips it, while the inspector row is named "Unison, Not used in Legato" ([panels](panels.md), rule 5). At 1 voice with a stored Oldest, that row is named "Unison, Not used in Legato" too.
- **Lists the build extends:** `examples/dev/all-modules.ron` gets the assigner once, placed with no overlap (the scene test needs every built-in module); `crates/loom-modules/src/tests.rs` adds `("loom.voice.assign", "mode")` to its choice widgets, `voices` and `unison` to its number widgets, and `&["gate"]` to its display list.
- **Help page** `docs/modules/loom.voice.assign.md` from `mise run help-pages`.

## Chords

A chord is not a value on a cable. On a cable it is V pitches in a bundle with their gates. Stored, it is a symbol: a root, a shape, and a voicing, in a chord lane of a pattern ([module data](module-data.md#room-for-notes-and-chords)). The Chord module (MT-NOTE-02, M3) turns a symbol into pitches when the patch plays, against the patch scale when the root is written as a scale step ([scale and tuning](scale-and-tuning.md)). Inversion and octave spread are Chord's parameters. *Why a symbol:* a step written as "the fifth chord of the scale" must follow a scale change (UC10, step 6), and only a symbol can. *Why pitches on the cable:* then a chord is nothing new to the rest of the patch.

## Three sources end to end

These check the model. Only the first is built in M2.

### A keyboard into an 8-voice synth (M2)

Note input (`voices` 8, Oldest) `pitch` → Saw `pitch` → SVF `in` → Amplifier `in` → Voice merge `in` (Sum) → Audio Out. Note input's `gate` goes to an Envelope's `gate`, whose `out` goes to the Amplifier's `level` input and to Note input's `level`. These are today's modules with today's ports; the `pitch`, `gate`, and `level` inputs are `Downmix`, which rule 3 leaves as it is.

The `level` cable closes a loop. The compiler inserts a one-sample delay and
reports it. Its tie rule can choose a different cable when module IDs change.
The acceptance test creates Note input before Envelope and asserts that the
inserted delay is exactly Envelope `out` to Note input `level`. This loop runs
one sample at a time, including the eight envelopes. Choosing `level` regardless
of creation order is follow-up [#733](https://github.com/brylie/LoomDSP/issues/733),
outside this change.
The shipped example leaves this cable out, so its chain has no loop
([the shipped example](#the-shipped-example)); the 8-mono-chains test keeps it,
with its delay on `level`.

1. The compiler finds a `Voices` output on Note input, so the Saw, SVF, Envelope, and Amplifier run as 8 copies each (rule 4), and Voice merge and Note input, which have `Voices` inputs, run as one.
2. Eight keys down: the allocator gives each a voice in the order the notes arrived. Each gate rises on its note's frame and each pitch holds. Velocity is output per voice too; this chain does not patch it, but a second Amplifier with `velocity` on its `level` input would scale each voice by it.
3. A ninth key: no voice is free. Oldest steals the voice whose note started first; its gate is low on that frame and high on the next with the new pitch. Switching Mode to Quietest would steal the voice whose envelope has decayed furthest instead, because Note input reads all 8 levels in one instance.
4. A key up: its gate drops, the envelope releases, the voice holds its pitch, and the voice is free. The next key takes the voice free longest.
5. A mono LFO into the SVF's `cutoff` input: every copy reads it (rule 6). A Voice spread between Note input's `pitch` and the Saw's `pitch`: each copy adds its own detune.
6. Voice merge sums the 8 amplifiers' outputs into one audio cable into Audio Out.

#### The shipped example

`examples/patches/polysynth.ron` is this synth as an example patch ([#677](https://github.com/brylie/LoomDSP/issues/677)), and it is the first reference patch of NFR-PERF-01 ([Cost](#cost), rule 29). What the player finds when they open it:

- **The chain above, with two additions #677 asks for.** A **Voice Spread** sits on the pitch path, as in step 5, with a small detune: Random with the default seed, at `amount` 0.0003, about ±3.6 cents, a value the display shows exactly. *Why Random:* each voice drifts by its own small amount, as the voices of a polysynth do, rather than in an even fan. A **Phase** module sits before the Saw, so each voice's phase is a cable the player can see and re-patch, and the Saw plays from it instead of its own phase ([oscillators](oscillators.md)). **MIDI Note** is at 8 voices in Oldest mode, and its `level` is left unpatched, because that cable closes a loop (above).
- **How to try Quietest.** The patch comment says how: cable the Envelope's `out` to MIDI Note's `level` and set Mode to Quietest. The status bar then shows "1 feedback loop, 1 automatic 1-sample delay (dashed)", and the dashed cable is the one from the Envelope to `level`.
- **A gentle filter sweep.** One mono LFO moves the SVF's `cutoff` on every voice, through a Multiply that sets its depth to about ±0.5 octave. *Why the Multiply:* an LFO straight into `cutoff` would sweep about ±10 octaves. The patch comment names the LFO rate, the depth, and the SVF's cutoff and resonance, so a player knows what to turn.
- **The knobs are each module's own.** The M2 exit criterion's 6-knob panel waits for the panel editor for patches ([#76](https://github.com/brylie/LoomDSP/issues/76)).
- **It shows its width.** Hover text on an output port in the voice chain says `8 voices`; after **Voice Merge** the cable is one voice.
- **Eight held notes stay clean.** The Amplifier's gain leaves headroom for all 8 voices at their loudest, so a full chord peaks under -1 dBFS and is not pushed into the output limiter ([output safety](output-safety.md)). A test holds 8 notes for several seconds and checks the peak. The patch comment shows the arithmetic. A single note then plays well below full scale, as on most polysynths; the player can raise the Amplifier's gain when playing fewer notes. *Why 8:* the goal of this spec is to hold a chord and play over it, so the full count is the case to keep clean.
- **Chords need a source of several notes.** **Hold note** in the toolbar plays one note, and `loom render --note 60 --note 64 --note 67` plays the notes one after another, not together. To hear a chord, play a MIDI keyboard. With no hardware on macOS or Linux, run `loom midi-port "Loom Pad"`, before or after starting the app: the app connects to it on its own. Then type `on 60`, `on 64`, and `on 67`. On Windows, use a MIDI keyboard. The patch comment says so.
- **It loads in the app and in the CLI** (`loom check`, `loom render`) like every other example.

**Its reference render plays a chord.** Every other example patch plays one note in its reference render, which tests one voice. This one plays four notes, C, E, G, and B up from middle C (MIDI notes 60, 64, 67, and 71). They start on different frames inside one block and are released on different frames, so four voices sound at once and their release tails overlap. The other examples keep their one note, so their references do not change.

**Its benchmark prints a number on any machine.** `cargo bench -p loom-modules --bench engine` runs the example with 8 notes held, at 48 kHz and 128 frames, and prints one line beside criterion's own results, in this form:

```text
polysynth: 8 voices, 8 notes held, 48 kHz, 128 frames: T µs per block, P% of real time (budget 25%)
```

`T` is the mean time per block after a warm-up, and `P` is `T` as a share of the block's 2.67 ms. A second line for the Quietest variant, with the `level` cable, is printed too. A line reports and never fails, because the number depends on the machine. CI compiles the benchmark on every change but does not run it: shared runners are too noisy to judge time. The result on the reference machine is recorded by hand in `docs/benchmarks/`, with the machine named (model, CPU, and OS), the patch's module count, and its module runs per block. The check against the budget is on Manual QA: M2 ([#643](https://github.com/brylie/LoomDSP/issues/643)).

How it is built (technical design, 2026-11-14 and 2026-11-15, [#677](https://github.com/brylie/LoomDSP/issues/677)):

- **No `level` cable.** The cable from the Envelope's `out` to Note input's `level` closes a feedback loop that runs one sample at a time, with the eight envelopes in it ([the walk-through](#a-keyboard-into-an-8-voice-synth-m2)). Every cable is an edge of the graph (`successors` in `compile/steps.rs`), so the compiler cannot tell that Note input reads `level` only to choose a voice to steal. Which cable gets the inserted delay follows the module IDs, which is [#733](https://github.com/brylie/LoomDSP/issues/733). So the example leaves `level` unpatched and stays in Oldest mode. A comment in the file says how to try Quietest (cable the Envelope's `out` to `level`, and set Mode to Quietest), that this costs more, and that the status bar will then report one feedback loop.
- **The patch.** Module IDs run from Note input (1) left to right. Voice spread is in Random mode with the default seed, at `amount` 0.0003 (about ±3.6 cents), saved with the range -0.01 to 0.01. It shows exactly with 4 decimals ([control ranges](control-ranges.md#showing-the-value)).
- **The filter sweep.** LFO `out` → Multiply `a`, with `gain` 0.05 saved on the -0.1 to 0.1 range → SVF `cutoff`. The LFO's output spans -1 to 1, and the cutoff input is in pitch units (0.1 per octave), so a gain of 0.05 gives about ±0.5 octave around the knob's cutoff.
- **Headroom by arithmetic.** The Amplifier's `gain` is chosen so that 8 × saw peak × envelope peak × the SVF lowpass's peak gain at its resonance × `gain` ≤ 0.891 (-1 dBFS). The saw and the envelope peak at 1. The SVF's damping is `k = 2 - 1.98 × resonance` (`crates/loom-modules/src/filter.rs`), so `Q = 1/k`, and a second-order lowpass peaks at `Q / √(1 - 1/(4Q²))` when Q is above 1/√2, and at 1 below it. The patch comment shows the numbers. The estimate is not a bound: a resonant filter can ring past its peak gain on a saw's edge, and 8 detuned voices rarely line up. So a test holds it.
- **The headroom test** (`crates/loom-modules/tests/polyphony.rs`). It loads the example with output safety Off, holds the benchmark's 8 notes for several seconds (at least one full LFO cycle after the envelopes reach sustain), and checks with `loom_analysis::peak` that both sides stay under 0.891. *Why Off:* the limiter would hide an overshoot, and the engine's final clip at 1 would hide a clip.
- **Reference notes.** `crates/loom-modules/tests/patches.rs` gets `fn notes(patch: &Path) -> &'static [Note]`: `CHORD` for `polysynth.ron`, `[NOTE]` for every other patch. `CHORD` starts notes 60, 64, 67, and 71 on frames 240, 243, 247, and 251, all inside the engine block from frame 128 to 255, and releases them on frames 4,800, 5,600, 6,400, and 7,200.
- **Compile-report test** (`crates/loom-modules/tests/polyphony.rs`). The loaded example has no feedback loops, no skipped modules, and no stereo copies. Every output from Note input through the Amplifier is 8 voices, mono, and the outputs of Voice merge, the LFO, and the Multiply are one mono voice. The variant with the `level` cable is already tested on `main`: `acceptance_synth_matches_eight_mono_chains` in `tests/polyphonic_midi.rs` builds it with Note input first and asserts the one loop, with the delay on the cable into `level`.
- **The benchmark.** `crates/loom-modules/benches/engine.rs` replaces `criterion_main!` with its own `main`: the criterion groups, then `polysynth_share()`, then `Criterion::default().configure_from_args().final_summary()`. That is the macro's body with one call added, which `harness = false` allows. `polysynth_share` runs only when the arguments hold `--bench`, hold neither `--list` nor `--test`, and hold either no name filter or one that matches `polysynth`, read the way criterion reads its own filter. So `cargo test --benches`, a listing, and `cargo bench -- modes` stay quick. It:
  - uses `EngineConfig::default()`: 48 kHz, 128 frames, and output safety at Low latency, as the app plays;
  - sends 8 note-ons on frame 0 (MIDI notes 48, 52, 55, 59, 60, 64, 67, and 71);
  - asserts that the compile report has no loops and 8 voices on Note input's `pitch`;
  - plays 375 blocks (1 s) to warm up, and asserts the output is not silent, so a broken patch cannot report a false low number;
  - times 3,750 blocks (10 s) with one `std::time::Instant` around the loop, with `black_box` on the output, and prints the line above;
  - does the same for the patch with the `level` cable and Mode at Quietest, asserting one loop, and prints a second line, `polysynth (Quietest, level loop): ...`.

  `polysynth.ron` also joins criterion's `patch` group, with the same 8 notes. The module doc comment's list of groups names both.

#### The large reference patch

`examples/patches/polysynth-large.ron`, shown to the player as **Layered polysynth (benchmark)**, is the second reference patch of NFR-PERF-01 ([#787](https://github.com/brylie/LoomDSP/issues/787)). It answers a different question from the polysynth: what a full instrument costs, not a minimal one. Agreed 2026-11-16:

- **About 100 modules in the patch file,** built from shipped modules. It is a realistic layered 8-voice synth, not 100 copies of one module, so the cost is spread across the kinds of modules a player uses. *Why count the file:* the polysynth's record counts modules in the patch (12), so the two numbers compare. The module runs per block are recorded beside it.
- **What is in it.**
  - Note input at 8 voices into a Voice assigner at 8 voices. This is there so the benchmark measures the assigner; it is not a pattern to copy, since Note input already gives each note its own voice.
  - Per voice, about 55 modules: four sound layers (saw, pulse, triangle, and sine), each with its own phase, detune, filter, filter envelope, amplitude envelope, amplifier, and its own LFO with a slew and a depth. Then a layer mixer, a filtered noise layer, saturation, a final filter, and an output amplifier.
  - Shared, about 40 modules: the two sources and the Voice merge, global LFOs and macros, the math for modulation, and a stereo effect chain after the merge (delays, diffusion, EQ, dynamics, and a wet and dry mix), then Audio Out.
- **Its header** is in the polysynth's form, so a reader can find their way in a file of 100 modules. It says, in this order:
  - what the patch is for: the second reference patch of NFR-PERF-01, a full instrument measured against the budget;
  - how it is laid out: one chain per row, left to right in signal order, with module IDs in that order;
  - how to play it: a MIDI keyboard, or `loom midi-port` with no hardware, as for the polysynth;
  - what to turn: the knobs that change the sound most;
  - that Note input into a Voice assigner at 8 voices is there to measure the assigner, and is not a pattern to copy;
  - that the file is generated by `scripts/gen-polysynth-large.py`: to change it, edit the script and regenerate, rather than edit the file by hand;
  - which notes the benchmark holds;
  - how to run its benchmark.
- **It reads in signal order.** Each chain sits on its own row, left to right in the order the signal flows, and module IDs follow that order. *Why:* Tab and a screen reader's tile order then follow the sound, so a player who cannot see the canvas meets each chain from its source to its end.
- **It is a benchmark first.** The patch need not be a preset a player would choose, but it should sound like an instrument, so that a broken patch is easy to hear.
- **Its benchmark line** is printed by the same `cargo bench -p loom-modules --bench engine`, beside the polysynth's line and in the same format: mean time per block and percent of real time, with 8 notes held.
- **A result over budget is a finding.** If it costs more than 25% of a core on the reference machine, the benchmark record says so. The patch is not cut down to pass. *Why:* a trimmed patch would measure nothing new.
- **It loads in the app and in the CLI** (`loom check`, `loom render`) like every other example, and has a reference render.

How it is built (technical design, 2026-11-15, [#787](https://github.com/brylie/LoomDSP/issues/787)):

- **Widths.** Note input is at 8 voices in Oldest mode, and its `pitch`, `gate`, `velocity`, `pressure`, and `timbre` go to the Voice assigner, also at 8 voices in Oldest mode with `unison` 1. Every per-voice module sits after the assigner, so it runs as 8 copies. Nothing stereo reaches the voice chain: a stereo cable there would double every copy after it (rule 4, inside each voice). The stereo effect chain starts with a Join after the Voice merge. *Why Note input into the assigner:* the benchmark sends MIDI, and the new module is to be measured. With 8 notes held and no stealing, the assigner gives each input voice its own output voice.
- **No feedback loop.** Each echo is a Delay line mixed with the dry signal. No cable runs back into an earlier input. `loom check` then reports no feedback loops, and every module runs a block at a time. *Why:* a loop's cost depends on which cable gets the inserted delay ([#733](https://github.com/brylie/LoomDSP/issues/733)), and the polysynth's Quietest line already measures a loop. A loop would make this number measure that one choice.
- **No `level` cable** on either source, for the same reason as the polysynth's.
- **Module IDs** run in the order of the comment at the top of the file: Note input 1, Voice assigner 2, then the per-voice layers, then the shared modules. The benchmark and tests name modules by these IDs, as they do for the polysynth.
- **Headroom.** The patch's last Amplifier leaves the output under -1 dBFS with the benchmark's 8 notes held, measured rather than worked out: with this many layers, the polysynth's arithmetic would not be a bound. The headroom test of `tests/polyphony.rs` runs on it too, with output safety Off.
- **Its benchmark line.** `crates/loom-modules/benches/engine.rs` gets `polysynth_large_share()`, called from `main` after `polysynth_share()`. It runs only when `runs("polysynth-large", &args)` is true, so `cargo bench -- polysynth` prints all three lines and `cargo bench -- polysynth-large` prints only this one. It loads the example with `EngineConfig::default()`, asserts that the compile report has no loops and that the Voice assigner's `pitch` is 8 voices, and calls `time_polysynth("polysynth-large", parts)`, which holds `POLYSYNTH_NOTES`, warms up, checks for sound, and prints:

  ```text
  polysynth-large: 8 voices, 8 notes held, 48 kHz, 128 frames: T µs per block, P% of real time (budget 25%)
  ```

  `polysynth-large.ron` also joins criterion's `patch` group, with `POLYSYNTH_NOTES`. The module doc comment names it.
- **Its reference render** comes from `crates/loom-modules/tests/patches.rs`, which already plays every example in `examples/patches`. `notes` gives `CHORD` for `polysynth-large.ron` too, so four voices sound in the reference. The file is `tests/reference/patches/polysynth-large.wav`, written with `LOOM_UPDATE_REFERENCES=1`. The render is bit-identical at buffer sizes 1, 77, 256, and 1,000, as for every example.
- **`loom check`** gets `check_passes_on_the_polysynth_large_example` in `crates/loom-cli/tests/check.rs`, beside the polysynth's: success, the module and cable counts the file has, "no feedback loops", nothing "not processed", no unknown module, and nothing on standard error.
- **Compile-report test** in `tests/polyphony.rs`: no loops, nothing skipped, no stereo copies before the Voice merge, every output from the Voice assigner to the Voice merge 8 voices, and every output after it one voice.
- **The generator.** `scripts/gen-polysynth-large.py`, plain Python with no dependencies, writes the patch. It runs on the system's Python 3, and no check runs it. The RON header says the file is generated by that script, and that a change is made by editing the script and regenerating, so a breaking change during the early alpha does not mean hand-editing 100 modules. The committed RON is what the tests load. The build pull request confirms that the script reproduces the committed file byte for byte.
- **In the app** (`crates/loom-app/src/polysynth_tests.rs`): the patch opens with the `Ui` harness, and hover text on an output in its voice chain says `8 voices`.
- **The record.** `docs/benchmarks/2026-11-DD-polysynth-large.md`, in the polysynth record's form: the machine (model, CPU, OS), the build, the module count, the module runs per block, the benchmark lines printed for the polysynth and for this patch, and how far the load on the machine can be trusted. The module runs per block are counted from the compile report (modules times their voice count), not estimated.

### A chord sequencer into the same synth (M3)

Chord sequencer (MT-SEQ-07: sequencer lanes for root and shape, then Chord) replaces Note input. The synth is unchanged.

1. The Chord module is a source: it has `voices` (8) and `mode`, and a `Channels::Voices` output. On each step it reads the chord symbol, resolves it to pitches against the patch scale, and sends note-offs for the pitches that left and note-ons for the pitches that arrived (rule 23).
2. C major to A minor 7: C, E, and G stay, since G is the seventh. Their voices keep their gates high and their envelopes do not restart. A arrives and takes the voice free longest.
3. A 5-note chord into a count of 4 plays four notes; the fifth steals the oldest (rule 25).
4. A Strum between Chord and the synth is a note effect: it reads the bundle on `Channels::Voices` inputs and re-sends each note-on later by its offset through the same allocator, so it is a source too.
5. The last step written as a scale step: when a trigger switches the scale, the next step resolves to the new scale's pitches. Steps written by name do not change.

### A note cycle with overlapping notes (M3)

A polyphonic Note cycle (MT-ARR-01, `voices` 8) into the synth.

1. The cycle reads the bar phase and its stored clip ([module data](module-data.md#room-for-notes-and-chords)). A note whose start falls between two samples starts on the first sample at or after it (rule 27). Two notes that overlap take two voices; the clip's note order within one frame is its stored order (rule 26).
2. Two notes of the same pitch that overlap: the second retriggers the first's voice (rule 22). A clip that wants both sounding uses a different pitch or a different cycle. *Why:* a keyboard cannot do it either, and one rule is better than a special case in the clip reader.
3. A tied note across the loop point: the clip reader sends no note-off and no note-on at the loop, so the voice keeps sounding.
4. Nine overlapping notes in an 8-voice cycle: the ninth steals by the mode, as on a keyboard.
5. Recording into the cycle, quantise, and overdub are OQ-15 and the arrangement spec; launching cycles in sync is the arrangement spec. Nothing here changes for them.

### MIDI out ([#510](https://github.com/brylie/LoomDSP/issues/510))

A MIDI note output module has `pitch`, `gate`, and `velocity` inputs on `Channels::Voices`. A rising edge on a voice's gate sends a note-on with that voice's pitch rounded to a MIDI note and its velocity; a falling edge sends the note-off. Its own spec comes with its issue.

## Acceptance criteria

Bundles and copies:

- [x] Given Note input at 8 voices into the chain of [the first source](#a-keyboard-into-an-8-voice-synth-m2), with today's descriptors unchanged, when 8 notes play, then the four modules run as 8 copies each, Voice merge runs as one, and the output is bit-identical to 8 mono chains fed one note each and summed (test). The test keeps the `level` cable, which runs Note input and the envelopes one sample at a time with the delay on `level`; the shipped example leaves the cable out.
- [x] Given a mono LFO into a modulation input of a copied module, then every copy reads the same values (test).
- [x] Given a 4-voice pitch and an 8-voice gate into one oscillator, then it runs as 8 copies and copies 5 to 8 read the pitch input's default (test).
- [x] Given a bundle into a `Downmix` or `Stereo` input, then the module runs per voice and no voice is averaged or dropped; given a stereo cable inside one voice into that `Downmix` input, then the stereo downmix of the two sides still applies (test).
- [x] Given a bundle into Audio Out, then the output is the sum of the voices on each side (test).
- [ ] Given a stereo source inside an 8-voice chain, then the chain runs as 16 copies, and each side of each voice is bit-identical to a mono chain fed that side (test).
- [x] Given a bundle through a feedback loop, then every module in the loop runs per voice, and the inserted delay has state per voice (test).
- [x] Given a patch with no source wider than one voice, then it compiles to the same schedule as before: the same operations, buffers, and instances (test).
- [x] **Engine:** Given a count raised from 4 to 8 while 4 voices run, then voices 1 to 4 keep their state and voices 5 to 8 start from reset; given a count lowered from 8 to 4, then four copies are returned to the control thread; neither allocates on the audio thread (`no_alloc` test).
- [x] Given a seeded module running as 8 voices, then no two voices play the same sequence (test).
- [x] Given a descriptor with `Voices` inputs and `Voices` outputs (Voice assigner), then the registry accepts it and the module runs as one instance with its outputs at its `voices` count; given `Downmix` on an output, then the registry rejects it, as it does today (test).
- [x] Given a port with an 8-voice cable, then its hover text says `8 voices` (`Ui` harness).

Allocation, at block sizes 1, 77, and 512 with scripted events:

Checked **Algorithm** items below are covered by `loom-dsp` tests. MIDI routing,
level cables, and Note input processing at these block sizes are covered by
`polyphonic_midi.rs`. Voice assigner criteria stay open.


- [x] Given 8 voices and a ninth note-on, then in Oldest mode the voice whose note started first has its gate low on the note's frame and high on the next frame with the new pitch (reference test).
- [x] **Algorithm:** Given Quietest mode with `level` patched, then the voice with the lowest level is stolen; with `level` unconnected, the oldest (test).
- [x] Given one voice, then Legato keeps the gate high while any note is held and Retrigger drops it for one sample on each note (reference test).
- [x] **Algorithm:** Given a note-on for a note already sounding, then its voices retrigger and no other voice changes; given the same pitch on two MPE member channels, then it takes two voices (test).
- [ ] Given Voice assigner with `unison` 2, then a note takes two voices together and its note-off releases both; given a pressure change on the input voice, then both output voices' `pressure` follow it (test).
- [ ] Given Voice assigner with an input voice's `pitch` moving while its `gate` stays high, then its output voice's `pitch` follows and its `gate` does not retrigger (reference test).
- [ ] Given Voice assigner with two input voices on the same pitch, both with gates high, then two output voices sound (test).
- [ ] Given Voice assigner at 8 voices and `unison` 3, then two notes sound together and a third steals by the mode (test).
- [ ] Given Voice assigner in Quietest with `level` patched, then the output voice with the lowest `level` is stolen, in a case where it is not the oldest; with `level` unconnected, the oldest (test).
- [ ] Given a mono `pitch` and a mono `gate` into Voice assigner at 8 voices, then each rising edge starts one note, never more; given a 16-voice input into 4 voices, then notes steal by the mode (test).
- [ ] Given a 4-voice `pitch` bundle and a mono `gate` into Voice assigner at 8 voices, then one rising edge starts 4 notes on 4 output voices; given no cable on `gate`, then nothing plays (test).
- [ ] Given Voice assigner at 1 voice, then `unison` is hidden; at 2 voices or more in Oldest or Quietest, it is shown; at 8 voices with a stored Legato, it is shown (test).
- [ ] Given Voice assigner with a note held, then its `gate` light is lit, and with every output voice low it is dark (test).
- [ ] Given Voice assigner whose note was stolen while its input gate stays high, then that note does not sound again until its input gate rises again (test).
- [ ] Given Voice assigner at full capacity with an input voice at a lower index rising on the same frame as one at a higher index falls, and the falling note not the oldest, then the new note takes the ending note's voice and the oldest note keeps sounding (test).
- [ ] Given Voice assigner with 8 input voices held and its note inputs narrowed to 4 voices, then the notes of input voices 5 to 8 are released on the swap frame and the others keep sounding (engine test).
- [ ] Given Voice assigner with NaN on an input voice's `pitch` up to and including its last high gate frame, then after the fall the released output voice's `pitch` is the last finite value (test, and `check_non_finite_recovery`).
- [ ] Given Voice assigner with `unison` hidden, then its tile control leaves the accessibility tree and Tab skips it, while its inspector row is still read, named "Unison, Not used in Legato" ([panels](panels.md), rule 5) (`Ui` harness).
- [x] **Algorithm:** Given note-offs and note-ons for a chord change with one shared pitch, then the shared pitch's voice keeps its gate high across the change (test).
- [x] Given a note-on at frame `i`, then the gate rises at frame `i` at every block size (reference test).
- [x] **Algorithm:** Given a note-off, then the voice holds its pitch and velocity until its next note (test).
- [x] **Algorithm:** Given a note-on and a note-off on one frame, then a fresh voice has a one-sample gate on that frame; a stolen or retriggered voice has its low sample first, a one-sample gate on the next frame, then goes low (test).
- [x] **Algorithm:** Given the same events in the same order, then the same voices are chosen on every run (test).

Voice modules:

- [x] Voice merge: Sum and Average over 8 voices match the arithmetic; Last, Lowest, and Highest follow the gates and change only on a gate edge; a one-voice input passes through in every mode (reference tests).
- [x] Voice split: `out1` to `out8` are voices `first` onward, and an output past the last voice holds the default (test).
- [x] Voice spread: Even gives the stack's offsets, Series gives 0 up to `amount`, and Random differs per voice and repeats per seed, each rendered at a voice past 1 with `Harness::voice`; one voice gives `out = in` bit for bit in each of Even, Series, and Random (reference tests).
- [x] Voice spread in Random: raising the voice count from 4 to 8 leaves the offsets of voices 1 to 4 unchanged (test).
- [x] Voice split: a modulated `first` moves the outputs on the sample; a stereo bundle gives stereo outputs (test).
- [x] Voice split: a mono cable gives `out1` only, `first` at 2 on a mono cable gives the default on every output, and `first` at 16 on a 16-voice bundle gives voice 16 on `out1` (test).
- [x] Voice spread: given one 8-voice source fed into both inputs of Join, so each voice is stereo with equal sides, into Voice spread in Random mode, which then runs as one copy per side of each voice, and then Voice split, then for each voice `out - in` is the same on the left and right sides, is not zero, and differs from the other voices' (engine test, ADR-0045).
- [ ] Voice assigner: a mono gate and pitch into 8 voices gives overlapping release tails on distinct voices (reference test).
- [x] Voice merge `gate`: in Last, Lowest, and Highest it follows the chosen voice's gate, stays high with no retrigger when the choice moves between held voices, and is low when every voice is low; in Sum and Average it is the largest gate value; with `gate` unconnected it is 1 in every mode; a one-voice bundle passes its gate through (reference tests at block sizes 1, 77, and 512).
- [x] Voice merge: with NaN on voice 1 and finite values on voices 2 to 4, all playing, Lowest and Highest choose the right finite voice; with every playing voice non-finite, the lowest voice index is chosen and its value passes through (test at block sizes 1, 77, and 512).
- [x] Every example patch and test that uses Voice merge still loads after the `gate` output is added (test).
- [x] Voice merge with a stereo `in`: its `gate` output has two equal sides (test).
- [x] Note input at one voice in Legato mode gives the same output as before this feature on every existing reference render (reference tests).

The shipped example:

- [x] `examples/patches/polysynth.ron` opens in the app, where hover text on an output port in its voice chain says `8 voices`, and a screen reader finds Voice Spread's `amount` and `mode` and Voice Merge's `mode` by name (`Ui` harness).
- [x] `loom check` passes on it (CLI test).
- [x] Its reference render of the four-note chord is bit-identical at buffer sizes 1, 77, 256, and 1,000 (reference test).
- [x] It compiles with no feedback loop and nothing skipped, with 8 voices from Note input through the Amplifier and one voice after Voice merge (test). The loop that the `level` cable would make is already tested on `main` (`tests/polyphonic_midi.rs`).
- [x] With 8 notes held for several seconds and output safety Off, its output peaks under -1 dBFS (test).
- [x] The benchmark prints the line above on any machine that runs it, and CI compiles it (bench).

The large reference patch:

- [ ] `examples/patches/polysynth-large.ron` has about 100 modules, built from shipped modules, with a comment at its top that says what it is for and how it is laid out (review).
- [ ] `loom check` passes on it (CLI test), and it has a reference render (reference test).
- [ ] The engine benchmark prints a line for it beside the polysynth's, in the same format (bench).
- [ ] It compiles with no feedback loop and nothing skipped, 8 voices from the Voice assigner to the Voice merge and one voice after it, and with 8 notes held and output safety Off it peaks under -1 dBFS (test).

Cost:

- [ ] `examples/patches/polysynth.ron`, with 8 notes held, runs within the NFR-PERF-01 budget on the reference machine, and the result is recorded in `docs/benchmarks/` with the machine named (benchmark; manual, on the machine, Manual QA: M2 [#643](https://github.com/brylie/LoomDSP/issues/643)).
- [ ] `examples/patches/polysynth-large.ron`, with 8 notes held, is measured on the reference machine and the result is recorded in `docs/benchmarks/` with the machine named, the module count, and the module runs per block, whether or not it is within the budget (benchmark; manual, Manual QA: M2 [#643](https://github.com/brylie/LoomDSP/issues/643)).

## Out of scope

- **Event cables.** Decided against in ADR-0038. A later ADR can add them beside bundles.
- **SIMD across voices** (NFR-PERF-03). A later engine optimisation; the rules here do not change for it.
- **Release velocity**, and per-note expression stored in clips (an open point of [module data](module-data.md#room-for-notes-and-chords)).
- **The M3 sources:** the chord sequencer (MT-SEQ-07), the note cycle (MT-ARR-01), the piano roll (MT-SEQ-03), Chord, Strum, Arpeggiator, and the other note effects. They are followed above only to check the model. Each gets its ports and parameters in its own spec, on these rules.
- **Recording notes** (OQ-15) and **launching cycles** ([arrangement](arrangement.md)).
- **MIDI out** beyond the sketch above ([#510](https://github.com/brylie/LoomDSP/issues/510)).
- **Retuning MIDI notes to the tuning** ([scale and tuning](scale-and-tuning.md#out-of-scope)).
- **A voice count on a patch or sub-patch.** FR-VOICE-01 is met by the count on the source (rule 12).
- **A playable keyboard** (MT-UI-04): a source like Note input, specified with its widget.
- **A voice's MIDI channel as a signal.** Note input does not output it; MT-NOTE-07's routing by channel is specified with that module (M3).
- **A retrigger on Voice merge's `gate` when its choice moves between held voices.** Legato is the default a player expects from a mono line; a retrigger option can come later as a choice, without changing this one.

## Allocator interface (implemented, #673)

- `voice::Allocator` owns 16 fixed slots and a 16-note mono held list. Call
  `begin_frame` once per sample, apply events in arrival order, then read `voices`.
  There is no block clock or MIDI dependency. `NoteId` is an opaque caller key.
- `voices_for` finds the held voices of a note; `update_note` changes their pitch
  and expression without changing gates or order. A Voice assigner can call it
  every sample. Released voices hold their last values.
- `configure` changes capacity, mode, and unison. Capacity is clamped to 1 to 16;
  unison is clamped to 1 to capacity. New voices start silent; removed voices
  forget their notes. Reducing to one voice remembers only its surviving note.
- Invalid modes have a runtime fallback, without rewriting the stored mode:
  Oldest and Quietest at one voice act as Legato; Legato and Retrigger above one
  act as Oldest. A later capacity change resolves the stored mode again.
- A return to a held mono key does not retrigger: it is caused by a note-off, not
  a new note-on. Its pitch returns and its gate carries on. Velocity holds the
  last note-on's value, preserving the original mono Note input behavior.
- Unison takes whole groups. If capacity is not divisible by group size, stealing
  releases whole note groups until a new group fits. Any remainder stays free.
  Quietest compares the maximum level of each group's held voices. Ties use
  oldest start order, then lowest voice index. Non-finite levels read as zero;
  a short level slice is padded with zero. Without levels, Quietest is Oldest.
- A different note taking a voice freed on this same frame retriggers it:
  its gate is low on frame f and high from f+1. A gate that was high on the
  previous frame also needs a low output sample before the voice can restart.
  Otherwise an off followed by an on can leave the emitted gate high throughout,
  so the envelope never sees a rising edge. Shared chord notes keep their gates.
- A stolen or retriggered note that also ends on frame f keeps its required
  low sample on f, gives a one-sample high gate on f+1, then stays low from f+2.
  A fresh voice still gives its on/off pulse on f itself. A later note-on on a
  different voice does not cancel either pulse. These timing cases are explicit
  exceptions to rule 24's immediate gate drop, as the fresh pulse in rule 26 is.
- Notes begun or released on one frame are ordered by a serial number; free
  voices use release order, then voice index. Timing and order counters saturate
  instead of wrapping. At 48 kHz the frame counter lasts over 12 million years.
- The keyboard, chord-change, and scripted note-cycle cases are covered by
  algorithm tests. Replays in chunks of 1, 77, and 512 check deterministic event
  order; the allocator has no block API. Block-size invariance and reference
  renders are tested in the source modules when those modules exist.

## Open points

- **Lockstep copies for SIMD.** Whether the schedule format needs a per-voice stride for NFR-PERF-03, or the optimisation can stay inside the engine. Not before M2's exit criterion is measured.
- **Where the `level` loop's delay goes.** The inserted delay of the loop a `level` cable makes follows module IDs (see [A keyboard into an 8-voice synth](#a-keyboard-into-an-8-voice-synth-m2)). Choosing `level` whatever the order is [#733](https://github.com/brylie/LoomDSP/issues/733).
- **Any-type ports fade by their declared type.** Voice merge declares `in` as Audio, and Voice split and Voice spread declare Audio and Modulation, so a gate cable re-patched into them ramps for 5 ms, as one into Selector does. The rule for the editor's mixers (a cable fades by what it carries past them, [live patching](live-patching.md), rule 9) could extend to these modules.
