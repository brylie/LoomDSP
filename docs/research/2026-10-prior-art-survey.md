# Prior-Art Survey: Modules in Modular Audio Environments

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-10-17. Sources were read on this date.
- **Used by:** [Module catalogue](../product/module-catalogue.md), [ADR-0017](../adr/0017-primitives-traits-and-abstractions.md), [ADR-0018](../adr/0018-clean-room-module-development.md)

This survey records which kinds of module 15 other environments offer, so the module catalogue rests on evidence. It is a dated snapshot. Do not update it to follow later product releases; write a new snapshot instead, so this one keeps showing what we knew when we planned.

## How to use this document

- It names products and their modules as facts, and summarises them in our own words. It does not copy descriptions from vendor manuals.
- It is the only product document that may name other products' modules in detail (ADR-0018). Normative documents (the catalogue, specifications, help pages) use LoomDSP's own vocabulary.
- Module lists here are representative, not complete. The sources hold the full lists.
- Entries marked † come from the author's general knowledge and were not checked against a source page on the survey date.

## Method

For each environment we read its public module list or manual index, release notes, and reviews. We recorded how it groups modules, how many it has, and ideas worth learning from. We did not read any source code.

The findings were sorted into families of module, then merged into module types for the [catalogue](../product/module-catalogue.md) using the rule in ADR-0017.

## Findings

1. **About 27 families cover everything.** Across all 15 environments, every module fits one of about 27 families, from I/O and phase to arrangement and state. The differences between products lie elsewhere.
2. **The big differences are in time, structure, and panels.** How time and notes travel (phase signals, separate event signals, control-only "peer" machines), how much arrangement logic lives in the graph, and how panels and containers are built.
3. **Much variety is one job in several forms.** Samplers, slicers, and granular players all read a buffer. Unison and stereo spread are the same idea. This led to the primitives, traits, and abstractions model (ADR-0017).
4. **Swappable sections are popular.** Several products let the user change one part of a device (the filter, the oscillator) and keep the rest. This led to roles and slots.
5. **Utility collections are where routing and logic live.** The deepest sets of switches, matrix routers, attenuverters, flip-flops, and clock tools come from community collections, not from the core of any product.
6. **Generative and arrangement tools are growing.** The newest environments ship seeded melody and rhythm generators and song "conductor" modules inside the patch.
7. **Many environments keep a second signal kind** for events or control (Reaktor's event ports, Pure Data and Max messages, MNodes' MIDI and data signals). This is evidence for OQ-16.

## Environments

### Bitwig Studio: Grid, modulators, devices

Bitwig splits modularity into Grid modules (16 categories, over 200 modules), modulators that attach to any device (8 categories), and devices. Every Grid signal is stereo, and phase is its own category of signal.

- **Grid categories:** I/O, Display, Phase, Data (lookups read by phase), Oscillator, Random, LFO, Envelope, Filter, Shaper, Delay/FX, Mix, Level, Pitch, Math, Logic.
- **Representative modules:** Phasor, Step Access, Ø Scaler, Ø Counter; Steps, Gates, Probabilities, Array; Union, Wavetable, Bite, Phase-1, Scrawl, Swarm; Chance, Dice; Segments, Curves; Sallen-Key, Vowels, Comb, Dome; Wavefolder, Chebyshev, Transfer; Voice Stack Mix, Poly→Mono; Clock Quantize, Gate Repeat, N-Latch.
- **Modulator categories:** audio-driven, envelope, interface, LFO, modifier, note-driven, sequence, voice stacking.
- **Device categories:** synths and drum voices, note effects (over 20), audio effects, containers (layers, selectors, multiband splits), spectral splits.
- **Ideas worth learning:** phase as a signal type; sequencers read by phase; drawn curves shared across oscillator, LFO, envelope, and sequencer; devices whose sections can be swapped for Grid modules (Polymer, Filter+, Sweep).
- **Sources:** [Grid modules](https://www.bitwig.com/userguide/latest/grid_modules), [Modulators](https://www.bitwig.com/userguide/latest/modulator), [The Grid](https://bitwig.com/the-grid).

### Ableton Live 12

Ableton is semi-modular through Racks (parallel chains with macros and chain selectors) and Max for Live, not through cables.

- **Groups:** audio effects (about 40), instruments, MIDI effects, clip-level MIDI generators and transformations †, Max for Live modulators †, racks.
- **Representative devices:** Auto Filter, Beat Repeat, Corpus, Drum Buss, Echo, Erosion, Grain Delay, Hybrid Reverb, Roar †, Spectral Time †; Meld †, Granulator III †.
- **Ideas worth learning:** effects that carry their own LFO and envelope follower; devices that follow the clip's scale; a delay with a choice of how it reacts when its time changes.
- **Sources:** [Live audio effect reference](https://www.ableton.com/en/manual/live-audio-effect-reference/).

### VCV Rack, with Bogaudio and Count Modula

VCV Rack has a small core, a free starter set of 39 modules, and a large community library. Every library module carries tags from a fixed list, which works as a shared taxonomy.

- **Core and starter set:** audio and MIDI interfaces; VCO, Wavetable VCO, VCF, VCA, LFO, ADSR, Delay, Mix, 8vert, Mutes, Scope, SEQ 3, sequential switches, and more.
- **Library tags:** grouped in our words as sound sources, processors, effects, modulation, time and sequencing, utility and routing, interface, and properties such as polyphonic or expander.
- **Bogaudio (111 modules):** strong on routing and mixing (1:8 and 8:1 sequential switches, matrix switches up to 16 by 16, many mixers), logic (boolean, window comparator, edge detector, flip-flop), and polyphony tools.
- **Count Modula (136 modules):** strong on logic and timing (flip-flops of several kinds, gate delays, burst generators, dividers, counters), many sequencers, Bernoulli gates, shift registers, an event arranger, and a value stack.
- **Ideas worth learning:** the tag list; sequential switches that step, follow an address, or pick at random; utilities as a first-class part of the library.
- **Licence note:** both collections are GPL-3.0. Under ADR-0006 and ADR-0018 we read only their public listings, never their code.
- **Sources:** [VCV Free](https://library.vcvrack.com/Fundamental), [manifest tags](https://vcvrack.com/manual/Manifest), [Bogaudio](https://library.vcvrack.com/Bogaudio), [Count Modula](https://library.vcvrack.com/CountModula).

### Softube Modular

A Eurorack-style plugin. The base set has component-modelled Doepfer modules (oscillator, filter, dual amplifier, envelope, noise, LFO, later a seventh) and over 50 stock utilities, sequencers, and performance panels. Licensed add-ons come from Buchla, Intellijel, Mutable Instruments, 4ms, and Doepfer, and some Softube effects load as modules.

- **Ideas worth learning:** performance modules for building a playable front panel; an add-on store.
- **Sources:** [Softube Modular](https://softube.com/modular), [MusicRadar review](https://www.musicradar.com/reviews/tech/softube-modular-640853).

### Cherry Audio Voltage Modular

A Eurorack-style plugin with about 105 core modules and 15 drum modules, grouped in its store as sources, processors, effects, controllers, and utilities. Two modules host other plugins. A module designer lets users write their own modules, and a store sells third-party modules.

- **Ideas worth learning:** pop-up multiples on every jack; signal-flow indicators on cables; an in-app store with third-party developers.
- **Sources:** [Core and Electro Drums](https://www.pluginboutique.com/products/5033-Voltage-Modular-Core-Electro-Drums), [first-year modules](https://www.pluginboutique.com/products/5595).

### Native Instruments Reaktor

Three building levels: Blocks (rack-style modules), Primary (graphical patching of events and audio, with macros, instruments, and ensembles), and Core (a compiled, sample-level language). Red event ports and black audio ports form a two-rate signal model.

- **Primary categories:** Panel, Math, Signal Path, Oscillator, Sampler, Sequencer, LFO and Envelope, Filter, Delay, Shaper, Audio Modifier, Event Processing, Auxiliary, Terminal, MIDI In, MIDI Out, OSC. Module names inside each category are †.
- **Blocks:** 30 at the launch of version 6 in four lines, plus 23 more in a later set.
- **Ideas worth learning:** snapshots of panel state; per-voice information as signals; shared data tables across levels.
- **Sources:** [Reaktor](https://www.native-instruments.com/products/reaktor), [Reaktor 6 press release](https://native-instruments.com/en/press-area/music-production/reaktor-6), [module index](https://en.wikibooks.org/wiki/Category:Book:Reaktor).

### AudioMulch

Modules are called contraptions. Parameters live in a separate pane, apart from the patch layout, and patches can be edited while audio runs.

- **Categories:** signal generators (bassline, drums, arpeggiator, loop player, file granulator, endless-glissando tones), effects (delay-line granulator, live looper, canon looper, ring modulator, spatialiser), filters (comb bank, granular filter, pattern-triggered low-pass), dynamics, input and output, mixing and buses.
- **Ideas worth learning:** coarse, musical modules for live processing; a canon looper; morphing between parameter snapshots on a 2D surface †.
- **Sources:** [contraptions guide](http://www.audiomulch.com/help/contraption-ref/index), [contraption facts](http://www.audiomulch.com/help/getting-started-contraptions-facts), [Sound On Sound review](https://www.soundonsound.com/reviews/audiomulch-20).

### Jeskola Buzz

A tracker with modular routing. Machines are generators, effects, or control machines. Control machines (peer machines) have no audio; they write to other machines' parameters, for example as an LFO, an envelope, a chord player, or a parameter snapshot.

- **Ideas worth learning:** control as modules that write parameters, the same idea as LoomDSP's modulation assignments and scenes.
- **Sources:** [Wikipedia](https://en.wikipedia.org/wiki/Jeskola_Buzz), [machine archive](https://buzz.robotplanet.dk/search.php?u=BTDSys).

### Retronyms Tabletop (iPad)

A skeuomorphic studio of nearly 40 devices patched by cable: pad sampler, keyboards, tone matrix, bassline and drum pattern machines, delay, filters, reverb, mastering dynamics, turntable, mixers, splitter, sequencer, and recorders. Devices are whole products, not primitives.

- **Sources:** [user guide](https://retronyms.com/userguide/userguide.html), [overview](https://retronyms.com/overview.html).

### Mario Nieto MNodes (2026)

A plugin and standalone node environment released in July 2026, with over 330 nodes: 31 generators, 66 effects, 48 CV and sequencing, 33 MIDI tools, 32 UI widgets, and 71 logic, math, and data nodes. It has four colour-coded signal types (audio, CV, MIDI, data), subgraphs, polyphony up to 16 voices, a front panel designer, a plugin host, and a patch generator.

- **Ideas worth learning:** microtonal quantisers; a separate data signal type; a generator that builds a playable patch.
- **Sources:** [Isotonik Studios](https://isotonikstudios.com/product/mnodes-by-mario-nieto/), [KVR](https://www.kvraudio.com/product/mnodes-by-mario-nieto-world), [Synth Anatomy](https://synthanatomy.com/2026/07/mario-nieto-mnodes-node-based-modular-environment-and-infinite-sound-tools.html).

### Modular Fiasco (announced for Q4 2026)

A node-based DAW and plugin, not yet released on the survey date. Its module set is the most arrangement-heavy in this survey: seeded generative sequencers with genre settings, step and grid sequencers, MIDI tools, modulation and gate utilities, classic synth emulations, samplers and slicers, 35 effects, and a conductor module that switches sections and patterns.

- **Ideas worth learning:** generators that share a common set of controls (seed, clock, swing); a conductor module inside the patch.
- **Sources:** [Modular Fiasco](https://www.phasefiasco.com/modularfiasco).

### Alpha Forever Modular

A Windows plugin since 2018, with audio-rate signals everywhere, one cable type, and up to 32 voices. A container node holds other nodes, and its inner input and output nodes become its ports. No public node index was found; the list is assembled from release notes: several zero-delay-feedback filters, waveshapers, a Karplus-Strong-style resonator, Euclidean rhythm, drawn envelopes, reverbs, dynamics, EQ, and extra math functions.

- **Sources:** [container node](https://www.afmodular.com/nodes/designer), [update 2021/01](https://www.afmodular.com/devlog/update-202101), [update 2022/01](https://www.afmodular.com/devlog/update-202201).

### Pure Data and plugdata

Pure Data vanilla has about 290 objects in its original categories: dataflow glue, math, time, MIDI, tables, audio glue, audio math, oscillators and tables, filters, delays, subwindows, and data structures. Objects ending in `~` process audio; the others pass messages. plugdata wraps Pure Data with a new interface and bundles the ELSE library (over 570 objects) and cyclone (over 220 Max-compatible objects).

- **Ideas worth learning:** a full set of small primitives under the friendly modules; abstractions as the unit of reuse.
- **Sources:** [Pd manual](https://msp.ucsd.edu/Pd_documentation/resources/chapter1.htm), [FLOSS Manuals object list](https://booki.flossmanuals.net/pure-data/ch072_introduction.html), [plugdata objects](https://www.mintlify.com/plugdata-team/plugdata/objects/overview).

### Max/MSP †

Max splits control objects from audio (MSP) objects, adds sample-level programming (gen~), polyphony and multichannel containers, and a built-in set of rack-style modules (BEAP). RNBO compiles patches to plugins and code. No source page was opened for Max in this survey.
