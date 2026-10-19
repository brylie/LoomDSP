# Glossary

| Term | Meaning |
|---|---|
| **Abstraction** | An open patch made from primitives that works as one module. Built-in abstractions ship with LoomDSP; users save their own the same way. You can open any abstraction to learn from it or change it (FR-LIB-24, ADR-0017). |
| **AudioWorklet** | The browser API that runs custom audio code on a dedicated audio thread. It always processes blocks of 128 frames. |
| **Block / buffer** | A group of samples processed together. Hosts choose the size (often 32 to 1024). |
| **Cable** | A connection from an output port to an input port. |
| **CLAP** | CLever Audio Plug-in. An open plugin format with a permissive licence. |
| **Clean room** | How LoomDSP modules are made: from first principles and published theory, without reading other projects' code or copying their text or designs ([ADR-0018](../adr/0018-clean-room-module-development.md)). |
| **Clock Divide** | A module that fires a trigger on every Nth incoming trigger ([clock specification](../design/clock.md)). |
| **Copyleft** | A licence (such as GPL or AGPL) that requires derived works to use the same licence. Not allowed in LoomDSP dependencies. |
| **Control thread** | Any non-audio thread, usually the UI thread. Allowed to allocate and block. |
| **COOP / COEP** | HTTP headers that make a page "cross-origin isolated". Required for SharedArrayBuffer in browsers. |
| **Counter** | A module that counts triggers and outputs the count as a phase, so a step sequencer or Step Select can follow it ([arrangement specification](../design/arrangement.md)). |
| **Cycle** | A module that plays a stored note pattern or audio loop, read from the clock's bar phase. Outputs an end trigger when its loop ends. LoomDSP's looper and clip ([arrangement specification](../design/arrangement.md)). |
| **Default input** | What an unconnected input reads: a fixed value, or a transport signal such as bar phase. A cable overrides it. |
| **Direct manipulation** | Editing an object where you see it, through controls it shows itself, rather than in a separate panel. LoomDSP's tiles work this way (ADR-0013). |
| **Display value** | A number a module publishes once per block for its panel to show, such as a sequencer's current step. Internal state, not a port. |
| **Downmix** | Turning a stereo signal into mono. LoomDSP takes the mean of left and right for audio, modulation, and pitch, the higher side for gates, and the mean the shorter way round for phases. What a control input does when a stereo cable arrives, unless another input already makes its module run once per side. Then each side reads its own channel ([stereo specification](../design/stereo.md)). |
| **Encapsulation** | Collapsing part of a patch into a sub-patch with its own ports and panel. |
| **End of cycle (EOC)** | A trigger that marks the end of a cycle. It fires on the first sample of the next cycle, which is the same moment. The name comes from Eurorack. See [Phase and cycles](../learn/phase-and-cycles.md#when-it-fires). |
| **Engine** | The Rust code that runs a patch and produces audio. The same engine runs in every target. |
| **Front panel** | The panel of a patch or sub-patch: the controls and displays that hide its wiring. A sub-patch's front panel is also what its tile shows in the parent patch. |
| **Grid** | LoomDSP's patch canvas, where every feature is a module. Named after the idea in Bitwig Studio's Grid. |
| **Hold note** | A toolbar button that plays one note, for testing a patch without a MIDI keyboard. |
| **Gate** | A signal used as on/off. Values above 0.5 are high. |
| **Group** | A set of loopers or other playable modules that take turns according to their next action. |
| **Inspector** | The side panel that shows every parameter of the selected module, for exact values and settings that do not fit on the tile. |
| **Level of detail** | Drawing less when zoomed out: tiles drop their text and live views, so large patches stay readable and fast. |
| **Living musical system** | A patch that plays, evolves, and arranges itself through modulation, randomness, sequencing, looping, and logic. What LoomDSP is for. |
| **Looper** | A module that records and plays a loop of audio or MIDI, synced to the clock. |
| **Lock-free** | Code that lets threads communicate without one thread waiting for another to release a lock. |
| **Modulation** | Changing a parameter over time with another signal (LFO, envelope, MIDI). |
| **Modulation assignment** | A link from any output to a parameter in the same patch level, with a depth, made without a cable ([modulation specification](../design/modulation.md)). |
| **Modulation matrix** | A table of every modulation assignment in a patch or sub-patch. |
| **Module** | One DSP unit on the canvas, such as an oscillator or filter. Built-in or from the SDK. |
| **Module catalogue** | The plan for the module library: every module type, trait, and role, with stable IDs such as `MT-FILTER-02` ([module catalogue](module-catalogue.md)). |
| **Module type** | What a module does, as listed in the module catalogue. One type can be built as more than one module ID. |
| **Next action** | What a group does when a member finishes its loop: play the next member, a random one, or follow an input. Taken from Bitwig's clip launcher, but patchable. |
| **Note effect** | A module that transforms notes rather than audio: arpeggiator, chord, transpose, quantise. |
| **Panel** | The controls and displays of a module, sub-patch, or patch, built from one shared set of widgets ([panels specification](../design/panels.md)). A module's tile is its panel. |
| **Panic (MIDI)** | An action that silences all voices and clears stuck notes. Not the same as a Rust `panic!`. |
| **Patch** | A complete graph of modules, cables, sub-patches, panels, and settings. Saved as one file. |
| **Performance mode** | A locked view that shows only the front panel, for live use. |
| **Phase Scale** | A module that multiplies or divides a phase while staying locked to it. Used for half-time, double-time, and polyrhythm. |
| **Phase signal** | A ramp from 0.0 to 1.0 that wraps. It drives oscillators, LFOs, and sequencers. See [Phase and cycles](../learn/phase-and-cycles.md). |
| **Phase Trigger** | A module that fires a trigger each time its input phase wraps. With Phase Scale in front, it multiplies a clock ([clock specification](../design/clock.md)). |
| **Pitch signal** | A signal that represents musical pitch on a fixed scale (see OQ-04). |
| **Polymorphic signal** | A signal that can connect to any input, whatever its type. Types only change colour and default ranges. |
| **Polyphony** | Playing several notes at once. Each note uses its own voice. |
| **Port** | An input or output point on a module. |
| **Polymetric** | Patterns of different lengths playing together, so they shift against each other over time. |
| **Primitive** | A module with one job, such as a buffer reader or a selector. Abstractions are made from primitives (ADR-0017). |
| **Role** | A contract of port names and core parameter names, such as "filter" or "envelope". A module or sub-patch that fills a role can go in any slot for that role (ADR-0017). |
| **Round robin** | Playing the members of a group one after another, then starting again. |
| **Real-time safe** | Code that always finishes in a bounded time. No allocation, locks, or system calls. |
| **RON** | Rusty Object Notation. A text format that maps directly to Rust data types through Serde. Used for patch files. |
| **Scene** | A stored state of chosen modules, recalled by a trigger, optionally with a morph. Not the same as `loom_canvas::Scene` in the code, which is what the canvas draws. |
| **Schedule** | The ordered list of module calls the engine runs each block. Built on a control thread. |
| **SharedArrayBuffer (SAB)** | Browser memory that the main thread and the audio thread can both read and write. |
| **SIMD** | Single Instruction, Multiple Data. One CPU instruction processes several values at once. |
| **SPSC queue** | Single-producer, single-consumer queue. One thread writes, one thread reads, no locks. |
| **Slot** | A place inside an abstraction that holds any part filling a given role. Swapping the part keeps the cables and carries values across by name (FR-ENCAP-08). |
| **Stack** | A sub-patch mode that runs N copies of its contents, each with its own spread values. How LoomDSP does unison for any module, not only oscillators (FR-ENCAP-10). |
| **Step Select** | A module with N gate outputs, one high at a time, chosen by a phase input. A step sequencer whose steps are "which module plays" ([arrangement specification](../design/arrangement.md)). |
| **Strongly connected component (SCC)** | A part of a graph where every node can reach every other node. In audio terms: a feedback loop. |
| **Sub-patch** | A patch nested inside another patch. Also called a macro or abstraction in other tools. |
| **Tile** | A module as drawn on the canvas: a compact box with inputs on the left edge and outputs on the right. |
| **Transport** | The patch's one source of musical time: tempo, time signature, play state, and position. From the app, a plugin host, or MIDI clock. |
| **Trait** | A shared option, such as timebase, seed, or spread, defined once in the module SDK and used by many modules, so it works the same everywhere (FR-SDK-07). |
| **Trigger** | An event on a gate cable, such as "loop ended". An output is high for one sample; inputs count rising edges (ADR-0007). |
| **Voice** | One copy of a polyphonic sub-patch, playing one note. |
| **VST3** | Steinberg's plugin format. Widest DAW support. The SDK is MIT-licensed since version 3.8. |
| **WAM2** | Web Audio Modules 2.0. A plugin standard for browser-based audio hosts. |
| **WASM** | WebAssembly. A portable binary format that runs at near-native speed in browsers. |
| **Wrap** | The moment a phase passes the point where 1 meets 0 and starts a new cycle. Detected as a jump of more than 0.5 between two samples, in either direction ([Phase and cycles](../learn/phase-and-cycles.md#finding-the-end-of-a-cycle)). |
