# Glossary

| Term | Meaning |
|---|---|
| **AudioWorklet** | The browser API that runs custom audio code on a dedicated audio thread. It always processes blocks of 128 frames. |
| **Block / buffer** | A group of samples processed together. Hosts choose the size (often 32 to 1024). |
| **Cable** | A connection from an output port to an input port. |
| **CLAP** | CLever Audio Plug-in. An open plugin format with a permissive licence. |
| **Copyleft** | A licence (such as GPL or AGPL) that requires derived works to use the same licence. Not allowed in LoomDSP dependencies. |
| **Control thread** | Any non-audio thread, usually the UI thread. Allowed to allocate and block. |
| **COOP / COEP** | HTTP headers that make a page "cross-origin isolated". Required for SharedArrayBuffer in browsers. |
| **Counter** | A module that counts triggers and outputs the count as a phase, so a step sequencer or Step Select can follow it ([arrangement specification](../design/arrangement.md)). |
| **Cycle** | A module that plays a stored note pattern or audio loop, read from the clock's bar phase. Outputs an end trigger when its loop ends. LoomDSP's looper and clip ([arrangement specification](../design/arrangement.md)). |
| **Encapsulation** | Collapsing part of a patch into a sub-patch with its own ports and panel. |
| **Engine** | The Rust code that runs a patch and produces audio. The same engine runs in every target. |
| **Front panel** | The user-facing control surface of a patch or sub-patch. Separate from the cable view. |
| **Grid** | LoomDSP's patch canvas, where every feature is a module. Named after the idea in Bitwig Studio's Grid. |
| **Hold note** | A toolbar button that plays one note, for testing a patch without a MIDI keyboard. |
| **Gate** | A signal used as on/off. Values above 0.5 are high. |
| **Group** | A set of loopers or other playable modules that take turns according to their next action. |
| **Inspector** | The side panel that shows every parameter of the selected module. Tiles on the canvas show only key controls. |
| **Level of detail** | Drawing less when zoomed out: tiles drop their text and live views, so large patches stay readable and fast. |
| **Living musical system** | A patch that plays, evolves, and arranges itself through modulation, randomness, sequencing, looping, and logic. What LoomDSP is for. |
| **Looper** | A module that records and plays a loop of audio or MIDI, synced to the clock. |
| **Lock-free** | Code that lets threads communicate without one thread waiting for another to release a lock. |
| **Module view** | A custom display drawn inside a module's tile on the canvas: scope, step grid, envelope shape, clip lane. |
| **Modulation** | Changing a parameter over time with another signal (LFO, envelope, MIDI). |
| **Module** | One DSP unit on the canvas, such as an oscillator or filter. Built-in or from the SDK. |
| **Next action** | What a group does when a member finishes its loop: play the next member, a random one, or follow an input. Taken from Bitwig's clip launcher, but patchable. |
| **Note effect** | A module that transforms notes rather than audio: arpeggiator, chord, transpose, quantise. |
| **Panic (MIDI)** | An action that silences all voices and clears stuck notes. Not the same as a Rust `panic!`. |
| **Patch** | A complete graph of modules, cables, sub-patches, panels, and settings. Saved as one file. |
| **Performance mode** | A locked view that shows only the front panel, for live use. |
| **Phase signal** | A ramp from 0.0 to 1.0 that wraps. It drives oscillators, LFOs, and sequencers. |
| **Pitch signal** | A signal that represents musical pitch on a fixed scale (see OQ-04). |
| **Polymorphic signal** | A signal that can connect to any input, whatever its type. Types only change colour and default ranges. |
| **Polyphony** | Playing several notes at once. Each note uses its own voice. |
| **Port** | An input or output point on a module. |
| **Polymetric** | Patterns of different lengths playing together, so they shift against each other over time. |
| **Round robin** | Playing the members of a group one after another, then starting again. |
| **Real-time safe** | Code that always finishes in a bounded time. No allocation, locks, or system calls. |
| **RON** | Rusty Object Notation. A text format that maps directly to Rust data types through Serde. Used for patch files. |
| **Scene** | A stored state of chosen modules, recalled by a trigger, optionally with a morph. Not the same as `loom_canvas::Scene` in the code, which is what the canvas draws. |
| **Schedule** | The ordered list of module calls the engine runs each block. Built on a control thread. |
| **SharedArrayBuffer (SAB)** | Browser memory that the main thread and the audio thread can both read and write. |
| **SIMD** | Single Instruction, Multiple Data. One CPU instruction processes several values at once. |
| **SPSC queue** | Single-producer, single-consumer queue. One thread writes, one thread reads, no locks. |
| **Step Select** | A module with N gate outputs, one high at a time, chosen by a phase input. A step sequencer whose steps are "which module plays" ([arrangement specification](../design/arrangement.md)). |
| **Strongly connected component (SCC)** | A part of a graph where every node can reach every other node. In audio terms: a feedback loop. |
| **Sub-patch** | A patch nested inside another patch. Also called a macro or abstraction in other tools. |
| **Tile** | A module as drawn on the canvas: a compact box with inputs on the left edge and outputs on the right. |
| **Trigger** | A short event on a gate cable, such as "loop ended". Inputs count rising edges. How long a trigger output stays high is open (OQ-19). |
| **Voice** | One copy of a polyphonic sub-patch, playing one note. |
| **VST3** | Steinberg's plugin format. Widest DAW support. The SDK is MIT-licensed since version 3.8. |
| **WAM2** | Web Audio Modules 2.0. A plugin standard for browser-based audio hosts. |
| **WASM** | WebAssembly. A portable binary format that runs at near-native speed in browsers. |
