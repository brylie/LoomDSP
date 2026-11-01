# UX Review: Self-Playing Systems and Long-Form Pieces

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01, on `main` at fd6f938 (after #485).
- **Follows:** [the producer review](2026-11-producer-ux-review.md) and [P1 UX audit, round 2](2026-11-p1-ux-audit-2.md).
- **Used by:** issues #507 to #510, and comments on #492 and #311. Decisions go in specs, ADRs, or the product docs.

This review reads the desktop app as Ilmari, the generative musician in the [personas](../product/personas-and-use-cases.md): someone who builds systems that play themselves, with sequencing, randomness, slow drift, and texture, and lets them run. Some of these pieces last an hour; some play all day in a gallery, every day for weeks, with nobody at the computer. Think of tape loops of different lengths drifting against each other for an album side, or a room that sounds different every hour. Do not update this snapshot later; write a new one after the fixes.

## Method

- Read the ranges of every module that sets time, rate, or randomness, from the generated module help pages (`docs/modules/`).
- Read the specs for random modules (`p2-modules.md`), autosave, the clock, arrangement, and output safety; the requirements and roadmap for M2 and M3; and the open issues for sequencers, random modules, loopers, and MIDI.
- Read the code for the headless player (`loom play`), the WAV recorder, crash recovery at start, and the app's command line options.

**Limits.** No patch was run for hours, and no service manager was set up. The findings on long runs come from the code and the format limits, not from a soak test.

Priority means: **High** blocks a task or misleads the user; **Medium** slows the user or looks broken; **Low** is polish.

## Summary

LoomDSP's core suits this kind of music well. Timing never drifts inside the app, every random source is seeded, live edits never drop out, and the transport, clocks, and logic modules are already the bones of a self-running system. Free LFOs and Phase Trigger can already make loops of unrelated lengths that drift against each other.

What fails is time scale and unattended running. Almost every slow control stops at seconds: the slowest LFO cycle is 100 seconds, envelopes and slews stop at 10 seconds, the delay at 2, and the scope shows 1 second at most. The headless player stops as soon as it runs without a terminal. A seeded patch plays exactly the same piece after every restart, with no way to ask for a new one. The sequencers, random modules, quantiser, counters, and loopers that this persona needs most are planned for M2 and M3 and already have issues.

| # | Area | Finding | Priority |
|---|---|---|---|
| 1 | Time | Slow controls stop at seconds: no drift over minutes or hours | High |
| 2 | Seeing | The scope shows 1 second at most; slow change cannot be watched | High |
| 3 | Unattended | `loom play` exits at once when run without a terminal | High |
| 4 | Variation | Every restart plays the same piece; no "new seed each run" | Medium |
| 5 | Unattended | The window cannot go full screen or hide the editor | Medium |
| 6 | Long runs | The soak test is one hour; galleries run all day for weeks | Medium |
| 7 | Long runs | A WAV recording cannot pass 4 GiB, about 3 hours | Medium |
| 8 | Output | No MIDI notes or controllers out, so no driving other instruments | Medium |
| 9 | Texture | Delay up to 2 s and all-pass up to 0.2 s; no reverb, looper, or grains yet | Medium |
| 10 | Pitch | Random pitch cannot be held to a scale until the quantiser lands | Low |

## Findings

### 1. Slow controls stop at seconds (High)

| Module | Slowest setting | So the longest cycle or move is |
|---|---|---|
| LFO, free | 0.01 Hz | 100 s |
| Envelope (attack, decay, release) | 10 s | 10 s |
| Slew (rise, fall) | 10 s per 1.0 of change | 10 s |
| Delay Line | 2 s | 2 s |
| Clock | 4 bars | 8 s at 120 BPM |
| Phase Scale, divide | 16 | 16 cycles of its input |
| Clock Divide | 64 | 64 triggers |
| Phase, with octave −10 and tune −48 st | about 0.016 Hz | about 63 s |

A piece that changes over 20 minutes needs chains of modules to slow things down: for example a Clock at 4 bars into two Phase Scales dividing by 16, which reaches 1024 bars, about 34 minutes at 120 BPM. That works, but it hides the musical idea ("this filter opens over half an hour") behind arithmetic, and it is tied to the tempo. A free slow drift has no such path past 100 seconds.

Direction: extend the ranges where the cost is small (LFO to periods of hours, envelope and slew times to minutes or hours, Phase Scale's divide), and keep the knobs usable with logarithmic mapping. The long delay is a memory question, so it belongs with the buffer and looper work (#311, #54). FR-LIB-16 (curves over bars or sections, M3) covers shaped long changes; this finding is about the plain building blocks.

### 2. Slow change cannot be watched (High)

The Scope's window is 1 ms to 1 s. Ilmari's need is "a canvas that shows what the system is doing", and in his music the interesting things move over minutes. A drift of 0.01 Hz shows on the scope as a flat line. The light and meter widgets show the present value only.

Direction: a slow view: a scope window of minutes, or a history display that plots a signal over the last minutes or hours, drawn from decimated telemetry so it costs little. The telemetry spec already sends scope chunks; a slow view needs a decimated stream, not more data.

### 3. `loom play` exits without a terminal (High)

The headless player is the natural way to run an installation: no window, no editor, nothing for a visitor to touch. It stops when its standard input ends (`play`, `crates/loom-cli/src/main.rs`: a thread reads one line from stdin and then stops the player). Under launchd, systemd, or a Windows service, standard input is usually empty, so the reader sees the end at once and the player exits at start, with success. Nothing reports why the room is silent.

Direction: run until a signal (Ctrl+C, SIGTERM) or until stdin gives a line, but never stop on end of input. Log the start, device changes, and dropouts with time stamps to stderr, so a log shows what happened overnight. Check that a lost device is handled as the app handles it.

### 4. Every restart plays the same piece (Medium)

Every random source is seeded, and the [random module rules](../design/p2-modules.md) say the same seed gives the same output on every load. Hard rule 3 (determinism) needs this, and it is right for tests, renders, and sharing. But an installation that restarts each morning plays exactly the same day as yesterday, and a listener who returns hears the same "random" piece. Today the only way to get a new piece is to change every seed by hand, and Noise is the only random source.

Direction (a spec change, before Random and Chance, #304 and #305, are built): a patch-level choice, "Same piece each run" or "New piece each run". With the second, the app picks a fresh base seed at load, mixes it with each module's seed, and shows it (and logs it, for `loom play`) so a good run can be played again exactly. Determinism holds: the same base seed gives the same output.

### 5. No full screen and no locked view (Medium)

The window cannot go full screen or hide the toolbar, inspector, and stats bar. In a gallery, the screen either shows the editor, which invites visitors to click, or is turned off. Performance mode (#492) will lock the panel; this is the same need without a performer: a view that shows the system playing and accepts no edits.

Direction: include an installation case in the performance mode spec (#492): full screen, no editing, the panel or canvas only, and a way out that a visitor will not find by accident.

### 6. The soak test is one hour (Medium)

NFR-REL-03 asks for a one-hour soak with zero dropouts and no memory growth. An installation runs eight to twelve hours a day for weeks. Problems that grow slowly (a counter that wraps, a float that loses precision as it grows, a queue that leaks one item an hour) do not show in an hour.

The core looks sound: the transport position is an `f64` from an anchor, and phases wrap. A long test would prove it.

Direction: a long-run check for a generative patch: twelve hours or more at faster than real time, with `loom render` or the engine directly, checking for stuck output, DC, NaN, and timing drift against the transport; plus a multi-hour live run before a release.

### 7. Recordings stop near 3 hours (Medium)

The recorder writes standard WAV with `hound`. A WAV file's size fields are 32 bits, so it cannot pass 4 GiB: about 3 hours 6 minutes of 32-bit float stereo at 48 kHz, less at 96 kHz. The recorder stops at its first write error and keeps what was written. A whole gallery day cannot be one file.

Direction: with in-app recording (#503), either split long recordings into numbered files at a size limit, or write a format without the 4 GiB limit (RF64 or W64), and say which in the spec. Note the limit in `--record`'s help until then.

### 8. No MIDI out (Medium)

FR-MIDI-04 (send notes, controllers, and clock, Should, M2) has an issue only for clock (#77). Generative systems often drive other instruments: a hardware synth, a player piano in a gallery, or a DAW that records the notes. Today the system's decisions can leave only as audio.

Direction: file MIDI note and controller output, with a spec: which modules send (a MIDI Note Out and a MIDI CC Out are the obvious pair), how gates become note on and off, and how output ports are chosen.

### 9. Texture tools are short (Medium)

Delay Line goes to 2 s, All-Pass to 0.2 s, and there is no reverb, looper, or grain module yet. Loops of 20 seconds or more, long feedback delays, and wide washes are the base of much ambient music. Reverbs and effects are FR-LIB-09 and FR-LIB-15; loopers and buffers are M3 (#54, #311).

Direction: when the buffer work starts (#311), make a long delay one of its first uses. No separate issue here.

### 10. Random pitch cannot follow a scale (Low)

Noise into Sample and Hold makes random pitch, but nothing holds it to a scale until the pitch quantiser (#308) and the scale and tuning work (#309) land in M2. Until then, random melodies are atonal or must be built from fixed Pitch Constants. Both issues exist; this review only confirms their weight for this persona.

## What works well

- **Determinism.** The same patch and seed give the same output, so a good run can be rendered again exactly, and a bug can be reproduced.
- **The transport** keeps its position from an anchor in `f64` beats, so clocks stay in step for hours.
- **Loops of unrelated lengths.** A free LFO into Phase Trigger fires every N seconds, with no tie to the tempo, so several of them drift against each other the way tape loops do.
- **Logic and clocks.** Compare, Logic, Memory, Clock Divide, and Sample and Hold already make simple rules: "every 7th bar", "if this and not that", "hold a value until the next trigger".
- **Live patching without dropouts** lets the system be changed while it plays, which is how these pieces are often found.
- **Output safety** guards the speakers against a feedback loop that grows while nobody is in the room.
- **Crash recovery** asks a question at start only when there were unsaved changes, so a saved installation patch restarts without one, and `--no-recovery` turns it off.

## Issues

Grouped to keep the backlog small. Priority and size are on the LoomDSP project board.

| Issue | Findings | Milestone | Priority | Size |
|---|---|---|---|---|
| [#507](https://github.com/brylie/LoomDSP/issues/507) Long time scales: slower ranges, and a slow view to watch them | 1, 2 | M3 | P1 | L |
| [#508](https://github.com/brylie/LoomDSP/issues/508) Unattended running: `loom play` without a terminal, a run log, a long soak, and long recordings | 3, 6, 7 | M2 | P1 | M |
| [#509](https://github.com/brylie/LoomDSP/issues/509) New piece each run: a patch-level seed choice, shown and replayable | 4 | M2 | P2 | S |
| [#510](https://github.com/brylie/LoomDSP/issues/510) MIDI note and controller output (FR-MIDI-04) | 8 | M2 | P2 | M |
| [#492](https://github.com/brylie/LoomDSP/issues/492) (comment): an installation case for performance mode | 5 | | | |
| [#311](https://github.com/brylie/LoomDSP/issues/311) (comment): a long delay as an early use of buffers | 9 | | | |
| Not filed here: the quantiser and scales (#308, #309) already exist | 10 | | | |

## Suggested order

1. `loom play` without a terminal (3, #508). It is a small fix and unblocks every headless installation.
2. The seed choice (4, #509), before Random and Chance (#304, #305) are built, so they follow it from the start.
3. Slower ranges (1, #507). Mostly descriptor changes, with reference renders updated.
4. The slow view (2, #507), with the telemetry spec.
5. The long soak and long recordings (6, 7, #508), with in-app recording (#503).
6. MIDI out (8, #510), after MIDI clock (#77), sharing its port choice.
