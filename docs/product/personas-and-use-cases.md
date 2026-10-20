# Personas and Use Cases

## Personas

### Mira, the sound designer

- Uses Bitwig and Reaktor. Comfortable with modular concepts. Does not write Rust.
- Wants to build unusual instruments fast and keep them as presets.
- Pain today: her Grid patches only work inside Bitwig. She cannot share them with people who use other DAWs.
- Needs: fast patching, good visual feedback (scopes, meters), modulation everywhere, reliable save and load.

### Tomas, the live performer

- Plays electronic sets with a laptop and MIDI controllers.
- Wants a small number of large, clear controls on stage. He does not want to see cables during a show.
- Pain today: complex setups are fragile. CPU spikes cause dropouts.
- Needs: performance mode with a locked panel, MIDI learn, fast preset switching without clicks, rock-solid audio.

### Aino, the instrument builder

*Deferred persona: Aino's main need, plugin export, is not scheduled. Her patching and panel needs are still served.*

- A developer who sells or gives away plugins. Knows some DSP.
- Wants to prototype by patching, then ship a plugin without rewriting it in C++.
- Pain today: RNBO costs money and needs Max. Writing JUCE code is slow for experimentation.
- Needs: export to CLAP, VST3, and standalone (WAM2 later); control over plugin metadata and parameters; a clean, branded panel; a stable patch format for version control.

### Leo, the module developer

- A Rust programmer interested in DSP.
- Wants to write a new filter or oscillator and have it appear in the module browser.
- Needs: a documented module SDK, a test harness, examples, and a clear contribution process.

### Sanna, the educator

- Teaches sound synthesis at a university or in workshops.
- Wants students to open a link in a browser and start patching. No installs.
- Needs: browser version, example patches, visible signal flow, the ability to open a sub-patch and see how it works.
- Note: Sanna is the main reason for the optional web target, which is deferred. Until then, she can use the desktop app with example patches.

### Ilmari, the generative musician

- Makes ambient and electronic music, both recorded and live.
- Builds systems that play themselves: loops that shift against each other, slow modulation, random variation, sections that change on their own.
- Pain today: in a DAW, arrangement lives on a fixed timeline. Clip launchers have next actions, but they cannot respond to logic, such as "after this group has looped four times, move on".
- Needs: loopers, polymetric sequencers, modulators, randomness, counters and logic, scenes, and a canvas that shows what the system is doing.

## Key use cases

Each use case links to the requirements that enable it.

### UC1. Build an instrument from primitives (Mira, Sanna)

1. Create a new patch.
2. Add a phase generator, an oscillator shaper, a filter, an envelope, and an output.
3. Connect them. Hear sound immediately while patching.
4. Add a feedback path from the filter output back to its input with a one-sample delay.
5. Save the patch.

Requirements: FR-GRAPH-*, FR-MOD-*, FR-CANVAS-*, FR-LIB-*, FR-FILE-01.

### UC2. Encapsulate and build a panel (Mira, Aino)

1. Select a group of modules and collapse them into a sub-patch called "Voice".
2. Expose four internal parameters as sub-patch inputs.
3. Open the panel editor. Place four knobs and a waveform display. Bind them to the exposed parameters.
4. Hide the cable view.

Requirements: FR-ENCAP-*, FR-PANEL-*.

### UC3. Perform live (Tomas). Highest priority use case

1. Open a saved set of instruments in performance mode.
2. The app recognises the connected MIDI controllers and loads their saved mappings.
3. Map any new knobs with MIDI learn.
4. Play notes. Sync tempo to external MIDI clock. Switch between presets and patches mid-set without clicks.
5. Tweak the patch structure during a break without any dropout.
6. If something goes wrong, hit "panic" to silence all notes instantly.

Requirements: FR-PERF-*, FR-MIDI-*, NFR-RT-*.

### UC4. Export to a desktop DAW (Aino). Deferred

1. Choose "Export > CLAP" or "Export > VST3".
2. Set the plugin name, vendor, and category. Pick target platforms.
3. Install the plugin. Load it in a DAW. Automate its parameters.

Requirements: FR-NATIVE-*.

### UC5. Export a standalone performance app (Aino). Deferred

1. Choose "Export > Standalone".
2. Get an app that opens straight into the instrument's front panel, with audio and MIDI settings.
3. Share it with a performer who does not use LoomDSP.

Requirements: FR-NATIVE-08.

### UC5b. Export to the browser (optional, Sanna)

1. Choose "Export > Web Audio Module".
2. Get a folder that can be hosted on a static web server.
3. Load it in a WAM2 host and play it.

Requirements: FR-WAM-*, NFR-WEB-*.

### UC6. Add a new module (Leo)

1. Create a module crate from a template.
2. Implement the processing trait and describe ports and parameters.
3. Run the provided test harness.
4. Register the module so it appears in the library.

Requirements: FR-SDK-*.

### UC7. Learn from a shared patch (Sanna)

1. Open a patch from a URL in the browser (deferred web target), or open a shared patch file in the desktop app.
2. Step into each sub-patch to see how it works.
3. Probe any cable with a scope.

Requirements: FR-FILE-*, FR-ENCAP-04, FR-VIS-*.

### UC8. Build a self-arranging system (Ilmari). Grid-centred arrangement

Goal: play group A round robin, each loop twice. After two rounds, play group B the same way. Then return to A. A clip launcher cannot do this.

1. Add a Looper Group for part A with four audio cycles, and record or load a short phrase into each.
2. Set it to play each loop twice, and to report "done" after two rounds.
3. Add a second Looper Group for part B, set up the same way.
4. Add a Song Sequencer (a counter and a step select) with two steps, A and B. Connect each group's "done" output to the song sequencer, and its outputs to the groups' "enable" inputs.
5. Add a polymetric step sequencer with lanes of 5, 7, and 16 steps, driving a synth voice and a filter.
6. Press play. Part A rotates through its loops, part B takes over after two rounds, and the song returns to A. The sequencer's lanes drift against each other throughout.
7. Open the Looper Group to see how it works: cycles, a clock divider, a counter, and a step select. Change its rule, for example to pick the next loop at random, and save it as your own abstraction.

Requirements: FR-LIB-14, FR-LIB-20 to FR-LIB-24. Decisions: ADR-0011, ADR-0012. Specification: [arrangement](../design/arrangement.md).

### UC9. Share a patch with credit and a licence (Mira)

1. Set "Your name" and a default licence once, in preferences. The licence chooser asks two plain questions: may others change it, and may they use it commercially.
2. Make a patch with a friend. Add the friend as a second author, and tags such as `drone` and `ambient`.
3. Post the patch file. Whoever opens it sees both authors, the tags, and the licence, with a link to what it allows.
4. A month later, filter the patch browser by the `drone` tag to find it again among many patches.
5. Someone else changes the patch and shares it on. Both original authors stay credited, and their name is added after them.

Requirements: FR-FILE-08 to FR-FILE-10, FR-SHARE-06, FR-SHARE-07, FR-NATIVE-09. Decision: ADR-0023. Specification: [patch metadata](../design/patch-metadata.md).
