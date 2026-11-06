# Polyphony and Voice Allocation Specification

How a cable carries one value per voice, how notes are given to voices, which modules make, merge, and spread voices, and what a module author has to do to become polyphonic (nothing, in most cases).

- **Kind:** living specification.
- **Status:** Agreed. Settled with the maintainer on 2026-11-06. The bundle rules, the voice count, allocation, and the four voice modules are M2. The three sources followed [end to end](#three-sources-end-to-end) are the test of the model; the chord sequencer and the note cycle themselves are M3. Nothing is built yet.
- **Decision behind it:** [ADR-0038](../adr/0038-notes-travel-as-per-voice-signals.md) (per-voice signals, bundle as inferred width), refining [ADR-0020](../adr/0020-inferred-channel-count.md) (inferred channel count) and [ADR-0007](../adr/0007-polymorphic-signal-model.md) (every cable is `f32`). [ADR-0022](../adr/0022-copies-take-state-from-the-running-instance.md) for the state of a new copy. [ADR-0030](../adr/0030-panels-hide-controls-a-mode-does-not-use.md) for the allocator's panel.
- **Use case and requirements:** UC10 (sequence chords and note clips), UC2 (live play). FR-SIG-08, FR-VOICE-01 to FR-VOICE-04, FR-MIDI-03 (MPE), FR-LIB-13, FR-LIB-20, NFR-RT-01, NFR-PERF-01, NFR-PERF-03.
- **Builds on:** [Stereo cables](stereo.md) (widths, copies, downmix), [Changing a running patch](live-patching.md) (schedule swaps), [Sub-patches](sub-patches.md) (stack mode is not voices), [Module data](module-data.md) (stored notes and chords), [Scale and tuning](scale-and-tuning.md) (chords as scale steps), [Arrangement](arrangement.md) (the note cycle).
- **Background:** [Voices and polyphony](../learn/voices-and-polyphony.md): what a voice is, why stealing exists, why a bundle is inferred, and why a gate carries the timing.
- **Prior art:** per-voice signals are how a modular grid such as Bitwig's Grid carries polyphony. Hardware polysynths settled oldest-note and quietest-note stealing decades ago. Named only to explain the ideas; the rules and modules below are our own.
- **Issues:** [#665](https://github.com/brylie/LoomDSP/issues/665) (this spec), [#666](https://github.com/brylie/LoomDSP/issues/666) (the note model). Users: [#510](https://github.com/brylie/LoomDSP/issues/510) (MIDI out), [#307](https://github.com/brylie/LoomDSP/issues/307) (multi-lane sequencer), [#50](https://github.com/brylie/LoomDSP/issues/50) (stored notes).
- **Code:** none yet. It will extend `crates/loom-core/src/width.rs` and `compile.rs` (widths and copies), `crates/loom-modules/src/midi.rs` (Note input), and add a voice allocator to `loom-dsp` (ADR-0035).

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
   | `Channels::Voices` | The module reads every voice of the bundle itself, and is not copied per voice. | The output has as many voices as the module's `voices` parameter says. The module is a source. A module may have both: it then runs as one instance (Voice assigner). |

   *Why voices do not follow the stereo split:* the stereo rules make control inputs `Downmix` so a stereo patch does not pay twice for a control signal that is the same on both sides ([stereo](stereo.md), rule 4). A voice's pitch and gate are the opposite case: they differ per voice by definition, and an envelope that read the maximum of eight gates could not play eight notes. The oscillators, Phase generator, Envelope, and Amplifier keep their `Downmix` control inputs and become polyphonic with no change. *Why one name for both sides of `Voices`:* it mirrors `Stereo`, which also means "reads both" on an input and "writes both" on an output.

4. **A module runs as one copy per voice when any input other than a `Voices` input receives a bundle.** Its voice count is the largest voice count among those inputs. A module with a `Voices` input never runs per voice, whatever its other inputs receive. Inside each voice, the stereo rules apply unchanged, so a voice can run as two copies. *Why the largest:* a 4-voice pitch and an 8-voice gate into one oscillator should give 8 oscillators, not 4; the narrower cable is padded (rule 6).

5. **Output widths.** A `Voices` output has the module's voice count. Any other output has as many voices as its module runs. A `Stereo` output is stereo per voice. So a module with no inputs is one voice wide unless it declares a `Voices` output, and voices flow only from sources, as stereo flows only from stereo sources ([stereo](stereo.md), rule 7).

6. **A narrower cable into a wider module is padded with the port's default.** Copies past the cable's last voice read the input's default: zero for a gate, the default pitch for a pitch. *Why:* a voice no source is playing should be silent, and the default is what an unconnected input reads already. Mono cables are the common case: a mono cable into a polyphonic module feeds every copy, as it feeds both stereo sides. That is the fan-out FR-VOICE-03 asks for, and it costs no module.

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

22. **A note already sounding takes its own voices again.** A note-on for a note that is held retriggers its voices rather than taking others. A note is identified by its pitch, and on Note input by its pitch and MIDI channel, so two MPE member channels can play one pitch on two voices with their own expression and note-offs. With unison (Voice assigner), one note holds several voices: all are taken together, retriggered together, and released together. *Why:* a keyboard cannot play one key twice at once, and a source that re-sends a note means "again", not "also".

23. **A chord change is note-offs for the pitches that left and note-ons for the pitches that arrived.** A source never re-sends a pitch that stays. With rules 19 and 22 this gives voice leading for free: a pitch two chords share keeps its voice and its gate, so its envelope does not restart (UC10, step 2). *Why a rule for sources:* the allocator cannot tell "the same note continuing" from "the same note again" except by what the source sends.

24. **Note-off drops the gate on its frame and frees the voice at once.** The voice holds its pitch, velocity, and expression until its next note, so a release tail stays in tune. If the voice is reused during the tail, the tail is cut by the retrigger. *Why hold the pitch:* an envelope's release stage reads the pitch after the gate fell.

25. **More notes than voices steal by the mode.** A chord sequencer with a 5-note chord into 4 voices plays 4 notes and the fifth steals the oldest. An unused voice holds its gate low and its last pitch. *Why:* the same rule as a keyboard, so a source needs no rule of its own.

26. **A note-on and a note-off on the same frame give a one-sample gate.** A note-off followed by a note-on for the same pitch on one frame is a retrigger (rule 21). Events on one frame are applied in the order they arrived. *Why:* every order gives one answer, so the output is deterministic (hard rule 3).

27. **Timing is at the sample.** A note that arrives for frame `i` changes the gate at frame `i`, whatever the block size. Note input already lands MIDI events on their frame. A stored clip reads its notes from the bar phase, so a note that starts between two samples starts on the first sample at or after its time. *Why:* hard rule 2. A gate on its sample is the whole of the note model's timing; there is no event queue to drain.

28. **Per-voice expression (FR-VOICE-04).** A source gives each voice a pitch (0.1 per octave, with bend added), a gate, a velocity (0 to 1), a pressure (0 to 1), and a timbre (0 to 1, the slide dimension). With MPE (FR-MIDI-03), each note's channel carries its own bend, pressure, and timbre, and they reach that voice only. Without MPE, channel bend and channel pressure reach every voice on the channel, and polyphonic key pressure reaches its note's voice. *Why these five:* they are the per-note dimensions MIDI 1.0 and MPE carry. Release velocity is out of scope.

### Cost

29. **A bundle costs one module per voice.** Eight voices of an oscillator, a filter, and an envelope are 24 module runs, as 8 mono chains would be. The NFR-PERF-01 reference patch assumes this. SIMD across voices (NFR-PERF-03) is a later engine optimisation that runs copies in lockstep; it needs no change here. *Why not optimise now:* the M2 exit criterion is 8 voices within budget on the reference machine, which per-copy processing meets by the arithmetic in [acceptance criteria](#acceptance-criteria).

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

### Voice merge (MT-VOICE-01)

`loom.voice.merge`: one signal from a bundle.

| | |
|---|---|
| Inputs | `in` (any type, `Channels::Voices`), `gate` (gate, `Channels::Voices`, default 1): which voices are playing, for the Last, Lowest, and Highest modes. |
| Outputs | `out` (one voice; stereo if `in` is stereo) |
| Parameters | `mode`: Sum (default), Average, Last, Lowest, Highest |
| Panel | A segmented control for `mode`. |

- **Sum** adds the voices. **Average** divides the sum by the voice count. *Why both:* a sum is right for audio, and an average is right for a modulation signal that should stay in range.
- **Last**, **Lowest**, and **Highest** pass one voice through: the voice whose gate rose most recently, or the playing voice with the lowest or highest `in` value. With `gate` unconnected its default of 1 makes every voice count as playing, and Last passes voice 1 until a gate edge arrives. The chosen voice changes only on a gate edge, so the output does not jump between voices while they sustain. *Why `in` and not pitch:* the module does not know what `in` is; the player patches the pitch bundle to get the lowest note.
- A one-voice `in` passes through unchanged in every mode.

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

### Voice spread (MT-VOICE-03)

`loom.voice.spread`: a different value per voice from one control.

| | |
|---|---|
| Inputs | `in` (any type, `PerChannel`, default 0) |
| Outputs | `out` |
| Parameters | `amount`: -10 to 10, default 0. `mode`: Even (default), Series, Random. `seed` ([M2 modules](p2-modules.md), rule 1) |
| Panel | A knob for `amount`, a segmented control for `mode`. `seed` in the inspector only. |

- The module runs once per voice (rule 4) and reads its voice `i`, from 1 to `V`, and the count `V` (rule 17).
- **Even:** `out = in + amount × offset`, with offsets evenly spaced from -1 to 1 (voice 1 at -1, voice V at +1, one voice at 0 when V is odd). This is the stack's spread formula ([sub-patches](sub-patches.md#modes), TR-04). **Series:** `out = in + amount × (i - 1) / (V - 1)`, from 0 up to `amount`. **Random:** `out = in + amount × r_i`, where `r_i` is a draw from -1 to 1 from the seed and the voice index, fixed for the session.
- With one voice, `out = in`. *Why:* a spread of nothing is nothing, so the module can sit in a patch before it is polyphonic.

### Voice assigner (MT-VOICE-04)

`loom.voice.assign`: notes from any source, re-allocated to this module's own voices.

| | |
|---|---|
| Inputs | `pitch` (pitch, `Channels::Voices`), `gate` (gate, `Channels::Voices`, default 0), `velocity` (modulation, `Channels::Voices`, default 1), `pressure` and `timbre` (modulation, `Channels::Voices`, default 0), `level` (modulation, `Channels::Voices`, default 0) |
| Outputs | `pitch`, `gate`, `velocity`, `pressure`, `timbre` (`Channels::Voices`) |
| Parameters | `voices`: 1 to 16, default 1. `mode` (rule 20). `unison`: 1 to 4, default 1. |
| Panel | `voices` as a number, `mode` as a segmented control, `unison` as a number. |

- Each rising edge on an input voice's `gate` is a note-on with that voice's `pitch` and `velocity`; each falling edge is its note-off. The allocator (rule 18) gives it an output voice. While the note sounds, its output voice's `pitch`, `velocity`, `pressure`, and `timbre` follow the input voice it came from, so expression moves with the note; after the note-off they hold (rule 24). *Why:* a mono sequencer into an 8-voice synth gets release tails that overlap, and a 4-voice source into 8 voices gets more.
- **Unison** gives each note `unison` output voices, taken, retriggered, and released together (rule 22). The voices then differ only by what Voice spread adds after the module. *Why:* unison is a spread of copies of one note, which is what the catalogue asks of this type.
- A module with both `Voices` inputs and `Voices` outputs runs as one instance (rule 3).

## Chords

A chord is not a value on a cable. On a cable it is V pitches in a bundle with their gates. Stored, it is a symbol: a root, a shape, and a voicing, in a chord lane of a pattern ([module data](module-data.md#room-for-notes-and-chords)). The Chord module (MT-NOTE-02, M3) turns a symbol into pitches when the patch plays, against the patch scale when the root is written as a scale step ([scale and tuning](scale-and-tuning.md)). Inversion and octave spread are Chord's parameters. *Why a symbol:* a step written as "the fifth chord of the scale" must follow a scale change (UC10, step 6), and only a symbol can. *Why pitches on the cable:* then a chord is nothing new to the rest of the patch.

## Three sources end to end

These check the model. Only the first is built in M2.

### A keyboard into an 8-voice synth (M2)

Note input (`voices` 8, Oldest) `pitch` → Phase generator `pitch` → Saw `phase` → SVF `in` → Amplifier `in` → Voice merge `in` (Sum) → Audio Out. Note input's `gate` goes to an Envelope's `gate`, whose `out` goes to the Amplifier's `level` input and to Note input's `level`. These are today's modules with today's ports; the `pitch`, `phase`, `gate`, and `level` inputs are `Downmix`, which rule 3 leaves as it is.

1. The compiler finds a `Voices` output on Note input, so the Phase generator, Saw, SVF, Envelope, and Amplifier run as 8 copies each (rule 4), and Voice merge and Note input, which have `Voices` inputs, run as one.
2. Eight keys down: the allocator gives each a voice in the order the notes arrived. Each gate rises on its note's frame and each pitch holds. Velocity is output per voice too; this chain does not patch it, but a second Amplifier with `velocity` on its `level` input would scale each voice by it.
3. A ninth key: no voice is free. Oldest steals the voice whose note started first; its gate is low on that frame and high on the next with the new pitch. Switching Mode to Quietest would steal the voice whose envelope has decayed furthest instead, because Note input reads all 8 levels in one instance.
4. A key up: its gate drops, the envelope releases, the voice holds its pitch, and the voice is free. The next key takes the voice free longest.
5. A mono LFO into the SVF's `cutoff` input: every copy reads it (rule 6). A Voice spread between Note input's `pitch` and the Phase generator's `pitch`: each copy adds its own detune.
6. Voice merge sums the 8 amplifiers' outputs into one audio cable into Audio Out.

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

- [ ] Given Note input at 8 voices into the chain of [the first source](#a-keyboard-into-an-8-voice-synth-m2), with today's descriptors unchanged, when 8 notes play, then the five modules run as 8 copies each, Voice merge runs as one, and the output is bit-identical to 8 mono chains fed one note each and summed (test).
- [ ] Given a mono LFO into a modulation input of a copied module, then every copy reads the same values (test).
- [ ] Given a 4-voice pitch and an 8-voice gate into one oscillator, then it runs as 8 copies and copies 5 to 8 read the pitch input's default (test).
- [ ] Given a bundle into a `Downmix` or `Stereo` input, then the module runs per voice and no voice is averaged or dropped; given a stereo cable inside one voice into that `Downmix` input, then the stereo downmix of the two sides still applies (test).
- [ ] Given a bundle into Audio Out, then the output is the sum of the voices on each side (test).
- [ ] Given a stereo source inside an 8-voice chain, then the chain runs as 16 copies, and each side of each voice is bit-identical to a mono chain fed that side (test).
- [ ] Given a bundle through a feedback loop, then every module in the loop runs per voice, and the inserted delay has state per voice (test).
- [ ] Given a patch with no source wider than one voice, then it compiles to the same schedule as before: the same operations, buffers, and instances (test).
- [ ] Given a count raised from 4 to 8 while 4 notes play, then voices 1 to 4 keep their state and voices 5 to 8 start from reset; given a count lowered from 8 to 4, then four copies are returned to the control thread; neither allocates on the audio thread (`no_alloc` test).
- [ ] Given a seeded module running as 8 voices, then no two voices play the same sequence (test).
- [ ] Given a descriptor with `Voices` inputs and `Voices` outputs (Voice assigner), then the registry accepts it and the module runs as one instance with its outputs at its `voices` count; given `Downmix` on an output, then the registry rejects it, as it does today (test).
- [ ] Given a port with an 8-voice cable, then its hover text says `8 voices` (`Ui` harness).

Allocation, at block sizes 1, 77, and 512 with scripted events:

- [ ] Given 8 voices and a ninth note-on, then in Oldest mode the voice whose note started first has its gate low on the note's frame and high on the next frame with the new pitch (reference test).
- [ ] Given Quietest mode with `level` patched, then the voice with the lowest level is stolen; with `level` unconnected, the oldest (test).
- [ ] Given one voice, then Legato keeps the gate high while any note is held and Retrigger drops it for one sample on each note (reference test).
- [ ] Given a note-on for a note already sounding, then its voices retrigger and no other voice changes; given the same pitch on two MPE member channels, then it takes two voices (test).
- [ ] Given Voice assigner with `unison` 2, then a note takes two voices together and its note-off releases both; given a pressure change on the input voice, then both output voices' `pressure` follow it (test).
- [ ] Given note-offs and note-ons for a chord change with one shared pitch, then the shared pitch's voice keeps its gate high across the change (test).
- [ ] Given a note-on at frame `i`, then the gate rises at frame `i` at every block size (reference test).
- [ ] Given a note-off, then the voice holds its pitch and velocity until its next note (test).
- [ ] Given a note-on and a note-off on one frame, then the gate is high for exactly that frame (test).
- [ ] Given the same events in the same order, then the same voices are chosen on every run (test).

Voice modules:

- [ ] Voice merge: Sum and Average over 8 voices match the arithmetic; Last, Lowest, and Highest follow the gates and change only on a gate edge; a one-voice input passes through in every mode (reference tests).
- [ ] Voice split: `out1` to `out8` are voices `first` onward, and an output past the last voice holds the default (test).
- [ ] Voice spread: Even gives the stack's offsets, Series gives 0 up to `amount`, Random differs per voice and repeats per seed; one voice gives `out = in` (reference tests).
- [ ] Voice assigner: a mono gate and pitch into 8 voices gives overlapping release tails on distinct voices (reference test).
- [ ] Note input at one voice in Legato mode gives the same output as before this feature on every existing reference render (reference tests).

Cost:

- [ ] The 8-voice reference patch runs within the NFR-PERF-01 budget on the reference machine (benchmark; manual, on the machine).

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

## Open points

- **The allocator's API** in `loom-dsp`: whether sources push events and pull per-voice state, or hand it a frame at a time. Decided in implementation, with the three sources above as its test cases.
- **Lockstep copies for SIMD.** Whether the schedule format needs a per-voice stride for NFR-PERF-03, or the optimisation can stay inside the engine. Not before M2's exit criterion is measured.
- **MPE zones.** How the lower and upper zones map onto Note input's `channel` setting, with FR-MIDI-03.
