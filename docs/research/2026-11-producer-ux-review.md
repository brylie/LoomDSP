# UX Review: Making a Track with LoomDSP

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01, on `main` at fd6f938 (after #485).
- **Follows:** [P1 UX audit, round 2](2026-11-p1-ux-audit-2.md), which read the app as a live performer.
- **Used by:** issues #502 to #506, and comments on #77 and #130. Decisions go in specs, ADRs, or the product docs.

This review reads the desktop app as a music producer: someone who makes finished, recorded tracks, and whose main tool is a DAW. They want LoomDSP for sounds no other tool makes, and then they want those sounds in their song, in time, at full quality. It asks one question at each step: can a producer get from an idea to a sound in their track today, and what stops them? Do not update this snapshot later; write a new one after the fixes.

## Method

- Walked the producer's path through the app: start a patch, make a sound, play it from the DAW, match the song's tempo, get the audio into the DAW, recall the sound later.
- Rendered the empty patch (New) and the example patches with `--screenshot-window` and `--dump-ui`.
- Read the module list (`loom modules`), the `loom render` and `loom-app` options, the parameter specs of Transport, Audio Out, and Audio In, the knob and slider step code, and the output safety, files, panel widgets, and audio input specs.
- Read the personas, use cases, requirements, and roadmap for what is planned, and searched the open issues for each gap.

**Limits.** No sound was played or recorded, and no DAW was connected. Routing between apps was judged from the code and specs, not tried.

Priority means: **High** blocks a task or misleads the user; **Medium** slows the user or looks broken; **Low** is polish.

## Summary

LoomDSP has strong foundations for a producer: deterministic, 32-bit float offline rendering (`loom render`), a sample-accurate transport, careful output safety, autosave and recovery, undo, and audio input for processing. What is missing is the path from a patch to a finished track. No persona or use case describes that path. Plugin export, the usual answer, is not scheduled. Inside the app, the producer cannot record what they hear, cannot set an exact tempo, cannot sync to the DAW, cannot play a chord, and starts every patch from an empty canvas.

Most of the parts already exist. The recorder is built (`--record`, a developer flag). The typed value entry is agreed in the panel widgets spec. MIDI clock is planned for M2 (#77). Voices and presets are M2 requirements. The cheapest large win is a Record button: it turns the app into an instrument a producer can use today, through any DAW, before plugin export exists.

| # | Area | Finding | Priority |
|---|---|---|---|
| 1 | Product | No producer persona, and no use case for getting a patch into a track | High |
| 2 | Capture | The app cannot record what it plays; bouncing is CLI only | High |
| 3 | Tempo | No exact tempo: arrow keys step 2.8 BPM, and typed values are not built | High |
| 4 | Sync | No MIDI clock and no other sync, so a patch drifts against the DAW | High |
| 5 | Voices | One note at a time: no chords | Medium |
| 6 | Output | The safety limiter can change a recording without saying so | Medium |
| 7 | Output | One stereo output: no stems, no separate parts to separate DAW tracks | Medium |
| 8 | Start | New gives only a Transport; six modules before the first note | Medium |
| 9 | Library | No sequencer, effects, sampler, or wavetable; the wavetable reader is an M1 Must | Medium |
| 10 | Recall | No presets or A/B compare; the patch file is the only snapshot | Medium |
| 11 | Levels | No output meter or master level in the window | Low |
| 12 | Text | The status bar says "1 modules" | Low |

## Findings

### 1. No producer persona or track use case (High)

The [personas](../product/personas-and-use-cases.md) cover a sound designer (Mira), a live performer (Tomas), an instrument builder (Aino, deferred), a module developer, an educator, and a generative musician. Mira and Ilmari make recorded music, but no use case says how their sound reaches a track. UC4, export to a DAW, belongs to the deferred Aino. FR-NATIVE-01 to 08 (plugin export, and a host plugin that loads any patch in a DAW) are "Later" and not scheduled. The requirements say why: LoomDSP first focuses on live and generative use and on sharing patches.

That is a fair choice, but it leaves the most common kind of electronic musician with no stated path. The gaps below follow from it: each step of the producer's path was designed for someone else.

Direction (a product decision): add a producer to the personas, or extend Mira, with a use case such as "make a sound in LoomDSP, play it from my DAW in time, and record it into my song". Then decide the order of the bridges to the DAW. A suggested order, cheapest first: record in the app (2), exact tempo (3), MIDI clock (4), more outputs (7), and plugin export last.

### 2. The app cannot record what it plays (High)

The recorder exists: `loom-app --record out.wav` saves what the engine plays as 32-bit float stereo, across Open, New, and device changes. It is a developer flag, and there is no Record button. In-app recording is FR-SHARE-03, a Should in M3.

`loom render` bounces a patch offline, faster than real time, to a 32-bit float WAV, with the same output at any block size. That is better than most tools offer. But it is a command line tool, and it plays only a list of notes of one length (`--note`, `--note-length`), so it cannot bounce a part played from the DAW or a patch the user is shaping by hand.

Today a producer must install a third-party loopback driver, route the app's output to it, and record in the DAW.

Direction: a Record control in the window that writes what the app plays to a file, with the file name shown while it runs. Move FR-SHARE-03 to M2. Later, let `loom render` play a MIDI file, so a part from the DAW can be bounced offline and bit-exact.

### 3. No exact tempo (High)

Transport's tempo runs from 20 to 300 BPM. A knob or the inspector slider works in a 0 to 1 position, so:

- an arrow key moves 1% of the range, 2.8 BPM, and Shift-arrow moves 0.28 BPM (`knob_step`, `crates/loom-canvas/src/controls.rs`);
- a drag moves 1.4 BPM per pixel, or 0.14 with Shift;
- the inspector slider has the same steps.

None of these lands on a whole BPM such as 128 except by chance, and the value text rounds, so it can say "128.0 BPM" and be 127.96. Over a 4-minute song at 128 BPM, an error of 0.04 BPM is about 0.16 of a beat: an audible flam against the DAW's grid.

The panel widgets spec already agrees that a focused knob takes a typed value, "such as a tempo of 128" ([panel-widgets.md](../design/panel-widgets.md#typing-a-value), Agreed, M2). It is not built, and no issue tracks it.

Direction: build typed values, starting with the knob and the inspector. Consider whole-number steps for tempo when Shift is not held.

### 4. No sync to the DAW (High)

The transport runs on its own clock. MIDI clock in and out is #77 (M2, FR-MIDI-07, Should). Nothing else is planned: no Ableton Link, and no transport start and stop from the DAW. Even with an exact tempo (finding 3), two free-running clocks drift apart, and the producer must start both by hand at the same moment.

Direction: keep #77 in M2, and have its spec cover the producer's case: follow the DAW's tempo, start, stop, and song position, so a patch lines up with bar 1. Decide whether Link belongs in the requirements.

### 5. One note at a time (Medium)

`MIDI Note` is monophonic, with last-note priority. A producer cannot play a pad, a chord, or a held note under a melody. Voices are FR-VOICE-01 to 04, Must in M2, but no issue tracks them yet.

Direction: file the voices work, with the spec first (how a patch or sub-patch becomes polyphonic, voice stealing, and how per-voice values reach the canvas).

### 6. The limiter can change a recording without saying so (Medium)

The output safety stage is on by default in the app, in Low latency mode, which "may distort briefly when it acts". Nothing in the window shows when it acts: limiter activity in the stats bar is [#130](https://github.com/brylie/LoomDSP/issues/130). `loom render` defaults to Off, so the same patch can sound different in the app and in a render.

For a performer this default is right: it protects speakers and ears. For a producer recording through a loopback device, it can flatten peaks in the final take with no sign.

Direction: show limiter activity (#130). When recording from the app (finding 2), say in the recording's status line which safety mode was on, or record before the safety stage as an option.

### 7. One stereo output (Medium)

`Audio Out` has one stereo input and plays on the device's first pair; the host writes silence to every other channel (`crates/loom-host-desktop/src/lib.rs`, the output callback). `Audio In` can read any of four pairs, so the asymmetry is clear. A producer cannot send a bass and a pad to separate DAW tracks, or print stems, through a multi-channel loopback device.

Direction: let `Audio Out` pick an output pair, as `Audio In` does, so several outputs can feed several DAW tracks. Plan this with the audio input spec, which already settled how pairs are chosen.

### 8. Starting from nothing (Medium)

New gives a patch with only a Transport. The first note takes six modules and six cables: MIDI Note, Phase, an oscillator, Envelope, Amplifier, and Audio Out. The phase-driven model adds one module per oscillator, which is a teaching strength but slow for someone who wants a bass sound now.

The six example patches are all synth demos. None shows processing audio (Audio In into Delay Line), and none plays a rhythm from the transport.

Direction: let New start from a short list of starters ("Empty", "Mono synth", "Audio effect"), each an ordinary patch that opens as an untitled copy, as examples already do (files.md). Add an audio effect example and a rhythm example. Sub-patches (M2) will later let a starter offer one "Voice" block instead of six modules.

### 9. Missing building blocks (Medium)

The library has no step sequencer (FR-LIB-08, M2), no reverb, chorus, compressor, or EQ (FR-LIB-09, M2, and FR-LIB-15), no sampler, and no wavetable reader. A producer can finish a sound in the DAW once it gets there (finding 2), so effects matter less than capture.

FR-LIB-02 lists a wavetable reader as an M1 Must, but `loom modules` has none, and the only wavetable issue is the M2 widget #315. Either the requirement or the M1 scope is out of date.

Direction: correct FR-LIB-02's milestone, or file the module. Give the M2 effects their issues when M2 planning starts.

### 10. No presets or A/B compare (Medium)

The patch file is the only way to keep a version of a sound. Presets (snapshots of parameter values, FR-PERF-02) are a Must in M2 with no issue. A producer shaping a sound wants to keep two versions of the knobs and switch between them to compare.

Direction: file the presets work, and have its spec include A/B compare: two snapshots, one key to switch, and click-free switching.

### 11. No output meter or master level (Low)

Gain staging needs a Level Meter module in the patch. `Audio Out` has no level control, and the window shows no output level. A producer records at a known level, so this is the first thing they look for. The stats bar already shows load and dropouts; an output peak meter fits beside them.

### 12. "1 modules" (Low)

The status bar shows "1 modules, 0 cables" for a new patch (`crates/loom-app/src/main.rs`, the stats bar). It should say "1 module".

## What works well

- **Offline rendering** is deterministic, 32-bit float, and the same at any block size: a better bounce than most tools give.
- **The transport** keeps its position from an anchor, not by adding steps, so timing does not drift inside the app.
- **Output safety** protects ears and speakers by default, and `loom render` defaults to Off, so a render is what the patch makes.
- **Autosave and recovery** keep work through a crash, and **undo** names every step.
- **Audio In** reads four input pairs, with gain and mute, so the app can already process audio from a DAW over a loopback device.
- **Preferences** show the buffer time in milliseconds, so a producer can see the latency they are choosing.
- **Examples open as untitled copies**, so a starter can never be overwritten by accident.

## Issues

Grouped to keep the backlog small. Priority and size are on the LoomDSP project board.

| Issue | Findings | Milestone | Priority | Size |
|---|---|---|---|---|
| [#502](https://github.com/brylie/LoomDSP/issues/502) Producer persona and a "patch to track" use case; order the DAW bridges | 1 | M2 | P1 | S |
| [#503](https://github.com/brylie/LoomDSP/issues/503) Record in the app, and say which safety mode a recording used | 2, 6 | M2 | P1 | M |
| [#504](https://github.com/brylie/LoomDSP/issues/504) Exact tempo: typed values on knobs and in the inspector | 3 | M2 | P1 | M |
| [#505](https://github.com/brylie/LoomDSP/issues/505) Producer starters: New from a starter, more examples, output meter, "1 module" | 8, 11, 12 | M2 | P2 | M |
| [#506](https://github.com/brylie/LoomDSP/issues/506) Output pairs on Audio Out, for stems and separate DAW tracks | 7 | M3 | P2 | M |
| [#77](https://github.com/brylie/LoomDSP/issues/77) (comment): follow the DAW's tempo, start, stop, and song position | 4 | | | |
| [#130](https://github.com/brylie/LoomDSP/issues/130) (comment): limiter activity matters for recording too | 6 | | | |
| Not filed here: voices, presets, effects, and the wavetable gap are requirement work for M2 planning | 5, 9, 10 | | | |

## Suggested order

1. Record in the app (2, 6, #503). The recorder exists; this is mostly a button, a file dialog, and a status line.
2. Exact tempo (3, #504). The spec is agreed; the knob and the inspector take typed values.
3. The persona and use case (1, #502). It sets the order for the larger bridges.
4. MIDI clock (#77), then voices and presets, from M2 planning.
5. Starters, the output meter, and the plural fix (8, 11, 12, #505), beside the others.
6. Output pairs (7, #506), when a producer asks for stems.
