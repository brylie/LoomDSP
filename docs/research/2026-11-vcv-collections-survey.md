# Eleven VCV Rack Collections Against Our Catalogue

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-28. The listings were read on this date.
- **Baseline:** the [module catalogue](../product/module-catalogue.md) as merged with pull request 935 (187 types), and the 66 modules `loom modules` lists.
- **Builds on:** the [prior-art survey](2026-10-prior-art-survey.md) of 2026-10-17, which already covered VCV's core set, Bogaudio, and Count Modula in outline. This snapshot does not repeat it.
- **Used by:** nothing yet. It is evidence for planning M3, "a robust module collection".

Brylie named eleven favourite collections, and asked which generic kinds of module they hold that LoomDSP could offer, in our own design language. This snapshot sorts their modules into kinds, maps each kind to our catalogue, and lists what is left over.

## How to use this document

- It names other products' modules as facts. Only research snapshots, "Prior art" sections of ADRs, and explainers in `docs/learn/` may do that (ADR-0018). The catalogue, specifications, and help pages use our own words.
- It records the **kind** of each module: what it is for. It records no panel layout, control name, parameter set, or help text, and a LoomDSP module must not be designed from one of these modules. Design from the purpose and the theory, and cite the references.
- No source code was read. Every collection is listed as open source in the VCV Library. The 2026-10 survey notes that Bogaudio and Count Modula are GPL-3.0, which ADR-0006 does not allow us to use; we did not check the others, and the rule is the same for all: listings only.
- Module lists here are examples, not complete lists.
- Entries marked † rest on the author's general knowledge of the module, because its listing did not say it. Treat them as unchecked.

## Method

For each collection we read one page: its listing in the VCV Library, filtered to open-source modules. A listing gives each module's name, its tags, and a one-line description. We read nothing else: no manuals, no module pages, no code.

| Collection | Modules listed |
|---|---|
| Count Modula | 136 |
| Bogaudio | 111 |
| HetrickCV | 79 |
| VCV (core set) | 50 |
| Surge XT | 46 |
| Befaco | 34 |
| Impromptu | 32 |
| Squinky Labs | 29 |
| Sapphire | 24 |
| Audible Instruments | 20 |
| Stochastic Telegraph | 15 |
| **Total** | **576** |

Each module was classed from its name, tags, and description only. Where those did not say what a module does, it is marked **unclear** below and not guessed. The Audible Instruments listings say only which hardware module each is based on, so those are classed by their tags alone.

Each kind is then given one status:

- **Built:** `loom modules` lists it.
- **Planned:** the catalogue has an entry; the ID is given.
- **Covered by a rule:** a trait, an engine rule, or a decision makes a separate module unnecessary.
- **Abstraction:** a sub-patch of modules we have or plan would do it (ADR-0017).
- **Gap:** nothing in the catalogue clearly covers it.
- **Out of scope.**

Each gap that touches a built module was also checked against that module's descriptor (`loom modules -v`): Phase, Phase Reset, Random, Noise, and Chance. Gaps in planned types rest on the catalogue's one-sentence entries, so a spec written later may already close them.

## Findings

1. **Almost every kind is already built or planned.** Of the kinds found in 576 modules, 21 are gaps. Of those, 7 are a change to a planned type, 6 are a mode of a module we have built, 2 are abstractions, and 6 may need a new type. The three examples in the request are all planned: a burst generator is Repeater (MT-GATE-03), a shift register is MT-LOGIC-06, and a gate sequencer is a gate lane of the Multi-lane sequencer (MT-SEQ-01).
2. **A large share of each collection is variants that our rules fold away.** Step counts, channel counts, matrix sizes, polyphonic and dual and quad copies, "voltage controlled" versions, expanders, multiples, and blank panels. See [What folds away](#what-folds-away). This is the main lesson for the design system: our catalogue stays small because the engine and the traits do this work, so each type must show its size, its voices, and its modulation well.
3. **One collection is built around the same idea as ours.** Of HetrickCV's 79 modules, 36 have a phase ramp in their name, and they read or shape one. They are independent evidence for "phase is life", and they show which phase tools players reach for. See [Phase tools](#phase-tools).
4. **Utilities are what players miss first.** Count Modula and Bogaudio are mostly switches, logic, gate tools, and level tools. Most of ours are planned for M3. Building them early gives a familiar toolkit at low DSP cost.
5. **The gaps cluster in four places:** phase tools that change timing inside a cycle, random sources (weighted choice, and impulses at a density), patterns and live recording in sequencers, and control signals stored in buffers.

## What folds away

These kinds appear many times in the collections and need no module of their own in LoomDSP.

| Kind in the collections | Why we need no separate module |
|---|---|
| Multiples, and polyphonic multiples | One output feeds any number of cables. |
| Polyphonic, dual, and quad copies of a module | Channel count is inferred per cable (ADR-0020). A sub-patch in stack mode gives N copies. |
| "Voltage controlled" versions of a utility | Any parameter takes a modulation assignment (ADR-0014). |
| Expanders that add outputs, channels, or control inputs | Modes and counts are parameters, and a panel hides what a mode does not use (ADR-0030). |
| The same module at 8, 16, 32, or 64 steps; matrices at 4, 8, or 16 | Length and size are parameters. |
| A module that passes an input unless something is plugged in | Unpatched means normalled (ADR-0043). |
| Mono to polyphonic converters | A mono cable feeds every voice. |
| A modulation matrix module | The app's modulation matrix. |
| MIDI mapping modules | MIDI learn. |
| Unison helpers | Voice assigner, built; stack mode with spread. |
| Blank panels, rack ears, cable colour pickers | No rack. Text notes belong to comments (FR-CANVAS-07). |
| Range converters between a collection's own phase range and others | Our phase is always 0 to 1. |

## Kinds by catalogue section

"Seen in" names collections, in short: CM Count Modula, Bog Bogaudio, Het HetrickCV, VCV, Surge, Bef Befaco, Imp Impromptu, Squ Squinky Labs, Sap Sapphire, AI Audible Instruments, ST Stochastic Telegraph. The face is the archetype from ADR-0053 that the kind would most likely take; it is a guess for the designer, not a decision.

### Time, transport, and phase

| Kind | Seen in | Status | Face |
|---|---|---|---|
| Master clock with swing and several outputs | Imp | Built in part: Clock (MT-CLOCK-02); swing and more outputs planned | Phase ring |
| Clock divider and multiplier | CM, Bog, Bef, Het | Built: Clock Divide, Phase Scale | Phase ring |
| A set of dividers for polyrhythms | CM | Abstraction of Clock Divide; also Euclidean rings (MT-GATE-04) and polymeter lanes (MT-SEQ-01) | Phase ring |
| Clock to phase, and phase to clock | Het | Specified (MT-PHASE-09); Phase Trigger built | Phase ring |
| Hold a trigger until the next clock | Imp | Planned: Quantise to clock (MT-CLOCK-04) | Event lanes |
| Reset hub, and a delayed start after loading | CM | Planned: Start and reset hub (MT-CLOCK-05) | Event lanes |
| Count triggers, then pass or block a signal | ST, CM | Abstraction: Counter (MT-LOGIC-04) and Mute (MT-ROUTE-03) | Event lanes |

### Phase tools

HetrickCV's phase family, kind by kind. This is the closest prior art to our own model.

| Kind | Status |
|---|---|
| Phase source with synced gates | Built in part: Phase (MT-PHASE-01), Clock |
| Reset, shift, divide and multiply | Built |
| Outputs at fixed offsets (four, or eight) | Abstraction of Phase Shift; the LFO already has a quarter-cycle output |
| Split one cycle into sub-phases | Planned: Phase split (MT-PHASE-06) |
| Bend the ramp: curves, skew, mirror | Planned: Phase warp (MT-PHASE-05) |
| Swing on a phase | Swing is planned on Clock (MT-CLOCK-02). On a phase it is likely a preset of Phase warp over two steps; its entry does not name it |
| **Vary the slope a little each cycle, to loosen timing** | **Gap** |
| **Repeat part of a cycle: stutter and ratchet** | **Gap.** Repeater (MT-GATE-03) does this for gates, not for a phase |
| **Hold or freeze a phase** | **Gap**, small |
| **Run a phase N times on a trigger, then stop** | **Gap.** The retrigger trait restarts a phase; nothing stops it after N cycles |
| A random value, or a random offset, per step of a phase | Abstraction: Phase split or Phase Trigger, with Random |
| Send a phase to one of two outputs by chance | Gap, small. Chance (built) routes triggers, not phases |
| Add phases and wrap the sum | Abstraction: Add and Phase Wrap and Fold, both built |
| Report a phase's direction, speed, and wraps | Abstraction: Slope detector (MT-ENV-05) and Phase Trigger |
| Phase to gates, to steps, to a Euclidean rhythm | Built or specified: Euclidean Rhythm, Step select (MT-LOGIC-05), gate lanes |
| Phase to LFO shapes, and a phase-read wavetable | Built in part: oscillators and the LFO take a phase input; wavetables come with the shape source trait |
| A drum pattern chosen from a map of patterns by position | Possible algorithm for Generator (MT-SEQ-04). Based on a hardware module, so design from the idea only |

### Gates, logic, and rhythm

| Kind | Seen in | Status | Face |
|---|---|---|---|
| Boolean gates | CM, Bog, Het, VCV | Built: Logic | Event lanes |
| Flip-flops: set and reset, toggle, data | CM, Bog, Het | Built: Memory | Event lanes |
| Comparator, and window comparator | CM, Bog, Het, VCV | Built: Compare; window mode planned | Time trace |
| Edge detector; gate to trigger; trigger to gate | CM, Bog | Planned: Gate shaper (MT-GATE-01) | Event lanes |
| Gate length and gate delay | CM, Bog, Het | Planned: Gate shaper | Event lanes |
| **Gate delay with several taps** | CM | **Gap**, small: a mode of Gate shaper or Shift register | Taps |
| Burst generator | CM, Bef | Planned: Repeater (MT-GATE-03) | Event lanes |
| Random gate routing by chance | CM, AI, Het | Built: Chance | Event lanes |
| Clocked random gates | CM, Het | Abstraction: Clock and Chance | Event lanes |
| Event counter and countdown | CM | Specified: Counter (MT-LOGIC-04) | Event lanes |
| Binary counter; gates to a value and back; binary sequencers | CM, Het, Squ | Planned: Binary code (MT-LOGIC-09, M4) with Counter | Event lanes |
| Shift register for values or gates | CM, VCV, Het | Planned: MT-LOGIC-06. The entry does not say it can run backward | Event lanes |
| Shift register fed back through a comparator | Het | Abstraction: Shift register and Compare | Event lanes |
| Value stack, first in or last in | CM | Planned: MT-LOGIC-08 (M4) | Event lanes |
| Minimum and maximum; sums and differences | CM, Bog, Het | Planned: Function (MT-MATH-02); Add and Subtract built | Number line |
| Slope direction | CM, Het, Bef | Planned: Slope detector (MT-ENV-05, M4) | Time trace |
| Gate combiners and gate buses with switches | CM, Het | Planned: Matrix (MT-ROUTE-04) with Logic | none fits; see below |
| Euclidean rhythm | CM, Het | Built | Phase ring |

### Level, mixing, and routing

| Kind | Seen in | Status | Face |
|---|---|---|---|
| Attenuate, invert, offset, rescale | all utility sets | Built: Scale, Clamp, constants | Number line |
| Multiply two signals, with offset | Bef, AI, Bog | Built: Multiply and Add | Time trace |
| Amplifier, and many of them | VCV, Bog, Bef, AI | Built: Amplifier | Transfer curve |
| Crossfade and pan | VCV, Bog, Het | Built: Crossfade. Pan planned: Stereo tool (MT-MIX-03) | Levels |
| Mid and side | VCV, Het | Planned: Stereo tool | Levels |
| Mixer with level and mute | all | Built | Levels |
| **Mixer with sends, EQ per channel, and a master section** | Bog, Squ, Bef | **Gap as one type.** Could be an abstraction of Mixer, EQ, and Stereo tool. Mixer has no sends | Levels |
| Sequential, addressed, and random switches, both ways | CM, Bog, VCV, Bef | Built: Selector. Planned: Distributor (MT-ROUTE-02) | Event lanes |
| Scan across inputs with a crossfade | Het | Covered: Selector with the smoothing trait | Levels |
| Rotate inputs across outputs | CM, Het | Planned: a preset of Matrix | none fits |
| Matrix mixer and matrix switch | CM, Bog | Planned: Matrix (MT-ROUTE-04) | none fits |
| Blend four sources from one point in a plane | Het | Planned: XY and vector pad (MT-UI-03) | State space |
| Mute, and mutes with a fade | CM, Bog, VCV, Sap | Planned: Mute (MT-ROUTE-03) | Levels |
| Slew, lag, and glide | Bog, Bef, Squ | Built: Slew | Time trace |
| Sample and hold; track and hold | all | Built | Time trace |
| Manual values, buttons, and manual gates | CM, Bog, VCV, Het | Built: constants, Button, Macro | Number line |
| **Convert between X and Y, and angle and distance** | Het | **Gap**, small. Useful with our attractor and pendulum modulators, whose angles are phases | State space |
| Three-dimensional vector tools | Sap | Out of scope for now | |

### Voices

| Kind | Seen in | Status |
|---|---|---|
| Split, merge, and sum voices | VCV, Bog | Built |
| A value per voice: offsets and constants | Bog | Built: Voice Spread |
| Reassign voices; unison with detune | Bog | Built: Voice Assigner |
| Split or filter notes by voice, and drop repeated notes | Imp, Squ | Planned: Note filter and split (MT-NOTE-07) |
| Show every voice of a cable | VCV | The voice lanes face. Showing every voice is the design's intent (Brylie, 2026-11-28; [module faces](../design/module-faces.md)); the engine's per-voice display data is still to come |

### Sound sources

| Kind | Seen in | Status |
|---|---|---|
| Basic oscillators | all | Built in part (MT-SRC-01) |
| Wavetable oscillator and LFO | VCV, Surge, Het | Planned: the shape source trait (TR-01) |
| Several detuned saws at once | Squ | Covered: stack mode with spread |
| FM operators and FM pairs | Bog, Squ, Surge | Planned: MT-SRC-07, MT-SRC-12 |
| Additive oscillator | Bog, Bef | Planned: MT-SRC-03 (M4) |
| Oscillator through a shaper that picks harmonics | Squ, Bef | Abstraction: Sine and Shaper (MT-SHAPE-01) |
| Subharmonics | Squ, CM | Planned: MT-SHAPE-04 (M4) |
| Drawbar organ | Squ | Abstraction of sines; or Additive oscillator |
| Plucked string; tuned delay | Surge | Planned: String model (MT-SRC-10); Delay Line built |
| Modal and resonator voices | AI, Surge | Planned: MT-SRC-06, MT-FILTER-09 |
| Swept sine | Bog | Planned: MT-SRC-05 (M4) |
| Noise colours | all | Built |
| **Random impulses at a set density** | Het, Sap | **Gap.** Noise has no impulse mode, and Chance needs a clock |
| Clocked and binary noise | Het | Abstraction: Noise and Sample and Hold, Compare |
| Chaotic sources and attractors | Het, Sap | Planned: MT-SRC-04, MT-MOD-06, MT-MOD-11 |
| **A network of masses and springs, or of particles, as a resonator** | Sap | **Gap.** Modal resonator and String model are the nearest. Fits our state space face and the "systems" line of modules |
| A tube as a waveguide | Sap | Gap, small: a relative of String model |
| Many synthesis models behind one module † | AI | Out of scope as one module. A sub-patch in select mode with a source slot does this |
| Kick drum voice | Bef | Planned: Drum voice (MT-SRC-09) |
| Endless rising ramps with shaped levels | CM | Planned for audio: Endless glissando (MT-SRC-11). The control-signal form is an abstraction of Phase Shift and a curve |
| Sample player for multi-sample files | Squ | Planned: Sampler (MT-SAMPLE-10), Zone map |

### Filters, shapers, dynamics, and effects

| Kind | Seen in | Status |
|---|---|---|
| Multimode and ladder filters | all | Built |
| Low-pass gate | Bog, AI | Planned: MT-FILTER-06 |
| Fixed filter bank; bank with a follower per band | Bog | Planned: Filter bank (MT-FILTER-09), Vocoder (MT-SPEC-04) |
| Formant and vocal filters | Squ | Planned: MT-FILTER-08 |
| Comb and resonator effects | Surge | Planned: MT-FILTER-05, MT-FILTER-09 |
| EQ, three bands and parametric | Bog, AI, Bef | Built in part: EQ |
| Compressor, limiter, noise gate, clipper | Bog, Squ, AI | Built: Dynamics, Clamp |
| Wave shaper, folder, rectifier | Squ, Bef, CM, Het, Surge | Planned: Shaper (MT-SHAPE-01) |
| Bit depth and sample rate reducer; bit shifting | CM, Het | Planned: MT-SHAPE-02 |
| Ring and amplitude modulation | Bog, Surge, AI | Planned: MT-SHAPE-03; Multiply built |
| Frequency shifter | Squ, Surge | Planned: MT-FX-05 (M4) |
| Delay, and delay with taps | VCV, Surge, Sap | Built: Delay Line. Planned: Delay (MT-DELAY-04) |
| Chorus, ensemble, flanger | Surge | Planned: Modulated delay (MT-DELAY-05) |
| Phaser, reverb, spring reverb, rotary speaker, vocoder | Surge, Bef, Sap | Planned: MT-FX-02, MT-FX-01, MT-FX-11, MT-SPEC-04 |
| Tape-style drive | Surge | Covered: the character trait on Shaper |
| Tremolo synced to a clock | Squ | Planned: MT-FX-03 |
| Granular texture effect | AI, Surge | Planned: Grain engine (MT-SAMPLE-06, M4) |
| **Exciter** | Surge | **Gap**, small: an abstraction of a filter, Shaper, and Crossfade |
| Compress and expand with a fixed law | Het | A curve of Shaper |
| **Swept-sine frequency response analyser** | Bog | **Gap.** It would show the real response of any chain, beside the response curve face. Good for learning |

### Envelopes, modulation, and randomness

| Kind | Seen in | Status |
|---|---|---|
| ADSR, AD, and envelopes with delay and hold | all | Built in part: Envelope (MT-ENV-01) |
| Envelope with drawn segments | AI | Planned: Segment envelope (MT-ENV-02) |
| Rise and fall generator that also slews | Bef, AI | Planned: Function generator (MT-ENV-03) |
| Envelope with a slow, lagging response | Bog | Covered: the character trait |
| Envelope follower | Bog, Sap | Planned: MT-ENV-04 |
| Pitch detector | Sap | Planned: MT-PITCH-04 (M4) |
| LFO with several phases | Bog, Surge | Built: LFO |
| Complex LFO from several parts | CM | Abstraction |
| Slow random signal | Squ | Planned: Fractal noise (MT-MOD-10); Random's walk mode is built |
| Random walk in one and two dimensions | Bog | Built: Random. Planned: Brownian (MT-MOD-08) |
| Random values with a chosen distribution | ST | Built in part: Random has two shapes, uniform and centred. More shapes would be a small addition |
| **Random choice among set values, each with its own chance** | ST | **Gap.** Markov (MT-SEQ-10) is the nearest, and is for notes |
| A value plus a little random change, sampled | Imp | Planned for notes: Note variation (MT-NOTE-05) |
| **A drawn line that changes a little each pass** | ST | **Gap.** Curve reader has no variation |
| Morph through stored shapes or keyframes | CM, AI | Planned: Curve reader by value (MT-MOD-02), Scene morph (MT-STATE-02) |

### Sequencers and note tools

| Kind | Seen in | Status |
|---|---|---|
| Step sequencer for values, with gates | all | Specified in part: Multi-lane sequencer (MT-SEQ-01) |
| Gate and trigger grids with many channels | CM, Imp | A gate lane of MT-SEQ-01 |
| Steps with repeat and chance | CM | Planned: step attributes of MT-SEQ-01 |
| Sequencer whose step is chosen by a control value | Bog, Bef | Planned: a read mode of MT-SEQ-01 |
| Sequencer read by a phase | Het | Planned: a read mode of MT-SEQ-01 |
| **Several stored patterns, and a chain of patterns** † | Imp | **Gap.** The listings say only "16-pattern" and the like. Our sequencer's data is one pattern of four lanes ([M2 modules](../design/p2-modules.md)). Conductor (MT-ARR-07, M4) switches patterns, so they must exist somewhere |
| **Record steps live from a keyboard or a button** † | Imp | **Gap for lanes.** Piano roll (MT-SEQ-03) says "drawn or recorded"; MT-SEQ-01 does not |
| Piano-roll sequencer | Squ | Planned: MT-SEQ-03 |
| A grid of sequences to launch † | Squ | Specified another way: Looper group and Song sequencer ([arrangement](../design/arrangement.md)) |
| Arpeggiator with patterns | CM, Bog | Planned: MT-NOTE-01 |
| Chords from a keyboard | Imp | Planned: Chord (MT-NOTE-02), Chord sequencer (MT-SEQ-07) |
| Random notes with a chance per key | Imp | Planned: Generator (MT-SEQ-04), if it takes a weight per note |
| Quantiser; quantiser that learns its notes from a reference | VCV, Imp | Specified: Pitch quantiser (MT-PITCH-02), which can follow held notes |
| Echo, loop, and filter for notes | Imp | Planned: Note repeat, Note cycle (MT-ARR-01), Note filter and split |
| Octave shift, transpose, detune a pair | VCV, Bog | Planned: Pitch math (MT-PITCH-01) |
| A small scripting language for sequences and maths | ST | Out of scope. Expression (MT-MATH-04) takes a formula only, by principle 7 |

### Buffers, displays, and controls

| Kind | Seen in | Status |
|---|---|---|
| **Record a control signal into memory, with several read and write heads** | ST | **Gap or unclear.** Buffer (MT-SAMPLE-01) "holds audio". Since every cable is the same kind of signal, a buffer could hold control signals too; the entry should say so |
| Layered looping delay | Sap | Planned: Audio cycle (MT-ARR-02), Buffer repeat (MT-FX-04) |
| Scope, XY scope, meters, spectrum | all | Built |
| Scope with several traces | CM | Gap, small: Scope shows one input |
| Note and chord viewer | Imp | Planned: Note monitor (MT-VIEW-04) |
| A log of values | ST | Planned: Probe's number mode (MT-VIEW-01) |
| Reference tone and tuner | Bog | Abstraction: Sine and Pitch Constant. Planned: Tuner (MT-VIEW-05) |
| Recorder; fade in and out with a recorder | VCV, CM | Planned: MT-SAMPLE-09, MT-ARR-06 |
| Touch faders, and a small keyboard | Imp | Built: Macro. Planned: Keyboard (MT-UI-04) |
| **A bank of pads, each sending a stored value** | Imp | **Gap.** Scene (MT-STATE-01) recalls module state; no control sends one of N stored values |
| **A point in a plane that drives many outputs by distance** | ST | **Gap**, near XY and vector pad (MT-UI-03) and Voronoi field (MT-MOD-09) |
| A computer key that fires a trigger | Imp | An app feature, not a module: a key mapping for Button, like MIDI learn |
| Notes sent out, and controllers sent out | VCV, Bef | Planned: MT-IO-06, MT-IO-07 |

### Unclear from the listing

Not classed, because the name, tags, and description did not say enough: Count Modula's Gated Comparator (tagged sequencer and random) and Event Arranger; Surge XT's Neuron and TreeMonster; Sapphire's Empath beyond "multi-stage filter". Audible Instruments' modules are classed by tags only.

## The gap list

The 21 kinds marked as a gap in the tables above, in the order we would look at them. "Mode" means a change to an existing entry; "type" means a possible new entry. None is decided here.

| # | Kind | Likely form | Why it matters |
|---|---|---|---|
| 1 | Stored patterns and pattern chains in sequencers | Mode of MT-SEQ-01, or a data question | Players expect it; Conductor already assumes it |
| 2 | Live recording into sequencer lanes | Mode of MT-SEQ-01 | Performance |
| 3 | Repeat part of a phase (stutter, ratchet) | Mode of Phase warp, or a type | Rhythm from phase, our core idea |
| 4 | Loosen a phase's timing a little each cycle | Mode of Phase warp with the seed trait | Same |
| 5 | A phase that runs N times and stops | Mode of Phase (built in part) | One-shot gestures |
| 6 | Hold or freeze a phase | Mode of Phase Reset (built) | Performance |
| 7 | Weighted choice among set values | Type, or a mode of Random | Generative patches |
| 8 | Random impulses at a density | Mode of Noise (built), or a type | Textures and triggers without a clock |
| 9 | Buffers that hold control signals | A sentence in MT-SAMPLE-01 | Every cable is the same kind of signal, so it may need little more than saying so |
| 10 | A curve that varies each pass | Mode of Curve reader | Fits the Curve work on the design canvas |
| 11 | A bank of pads sending stored values | Type, or a mode of Button | Performance surface |
| 12 | A point in a plane that drives many outputs by distance | Type, or a mode of XY and vector pad (MT-UI-03) | Performance surface; near Voronoi field |
| 13 | Mixer with sends and a master section | Abstraction | Familiar to every player |
| 14 | Frequency response analyser | Type, or an abstraction with Swept sine | Learning; pairs with the response curve face |
| 15 | Mass and spring network as a resonator | Type (M4) | Fits the "systems" modules and the state space face |
| 16 | Convert between X/Y and angle/distance | Mode of Convert (built) or Function | Makes the attractor modulators patchable as phases |
| 17 | Gate delay with taps | Mode of Gate shaper | Small |
| 18 | Send a phase one of two ways by chance | Mode of Chance (built) | Small |
| 19 | Scope with several traces | Mode of Scope (built) | Small |
| 20 | Exciter | Abstraction of a filter, Shaper, and Crossfade | Small |
| 21 | A tube as a waveguide | Type, a relative of String model (MT-SRC-10) | Small |

By likely form: 7 are a change to a planned type (rows 1 to 4, 9, 10, and 17), 6 are a mode of a module we have built (rows 5, 6, 8, 16, 18, and 19), 2 are abstractions (rows 13 and 20), and 6 may need a new type (rows 7, 11, 12, 14, 15, and 21).

Two smaller additions are not counted, because the tables do not mark them as gaps: swing on a phase, by name, which is likely a preset of Phase warp, and more distribution shapes on Random, which has two.

## For the design system

- **Three kinds have no archetype.** Matrix routers, gate and step grids with many channels, and gate combiners are all two-dimensional grids of cells. Count Modula, Bogaudio, and Impromptu all ship several. Our widget spec already sets some two-dimensional views aside as needing "a view of their own" ([panel widgets](../design/panel-widgets.md), Not in the baseline). Together these suggest a grid may be a missing thirteenth face.
- **Size is a parameter, so a face must scale.** Where the collections ship one module per size, we ship one type. A step lane must read at 8 steps and at 64, and a matrix at 4 by 4 and at 16 by 16.
- **"Voltage controlled" is not a module here, so modulation must be visible.** A player who used a voltage-controlled switch will look for the control input. The pink arc and the assignment marks have to answer that at a glance.
- **Presets may do the job of familiar modules.** A rotator, a mutable multiple, and swing are presets of planned types. A player searching the module browser for those words should find them. That is a question for the [module browser](../design/module-browser.md): search by what a module can do, not only by its name.

## Open

- Which gaps become catalogue entries or changed entries, and in which milestone. Nothing in the catalogue is changed here.
- Whether utilities should move earlier in M3, since they are cheap and widely missed.
- Whether the module browser should list presets and abstractions beside module types.
