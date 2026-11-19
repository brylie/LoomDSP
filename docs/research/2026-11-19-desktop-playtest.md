# Desktop play test, 2026-11-19

- **Kind**. Dated research snapshot.
- **Build**. macOS 27.0.1, Apple Silicon, release build of [`447bb01f`](https://github.com/brylie/LoomDSP/commit/447bb01fe64116141f712ea002b5c8e2739d2e15), the head of `main` when the session started.
- **Method**. Build in a live window using the module browser, cable drags, and panel controls driven by the [control channel](../design/control-channel.md). Play notes through a virtual CoreMIDI port, test Learn with an injected CC, and capture the actual CoreAudio loopback at 48 kHz and 512 frames. Input was disabled. This was a short desktop session on one setup.

The aim was to see how quickly a player familiar with modular instruments could make a playable synth, an evolving rhythm, and a reusable effect.

## Patches to try

The patch files and wiring notes are in [PR #833](https://github.com/brylie/LoomDSP/pull/833). These links pin the examples at the submitted commit so the checks can be repeated from a checkout or from downloaded files.

| Patch | What it exercised | How to play it |
|---|---|---|
| [Glass Chords](https://github.com/brylie/LoomDSP/blob/9846c8e7789d14b0cd15f8f13b93df5134c10416/examples/patches/glass-chords.ron) | Four voices through one oscillator, filter, envelope, and amplifier path. Voice Merge and Scope show the combined result. | Play C3, G3, E-flat4, and B-flat4 together on MIDI, or use **Hold note** for one voice. CC 74 on channel 1 controls Cutoff. |
| [Broken Pulse](https://github.com/brylie/LoomDSP/blob/9846c8e7789d14b0cd15f8f13b93df5134c10416/examples/patches/broken-pulse.ron) | Seven Euclidean hits over sixteen steps, Chance on the envelope trigger, and seeded random values assigned to filter Cutoff. | Let Transport run at 108 BPM. Try Hits, Rotation, and Chance 1. Transport Reset restarts both seeded random streams. |
| [Tidal Echo](https://github.com/brylie/LoomDSP/blob/9846c8e7789d14b0cd15f8f13b93df5134c10416/examples/patches/tidal-echo.ron) | Modulation onto a control, a feedback loop, and a sub-patch with three exposed parameters. | Let Transport run. Inside Tidal Echo, the band-pass signal feeds Mixer In 1, and Delay Line returns to In 2. Outside, turn Time and Feedback and pin the LFO's output badge to trace the sweep to Cutoff. |

The app reported zero dropouts during the patch runs. Each loopback recording contained a signal and no full-scale or non-finite samples. Peak levels for the three captures were about −22.3, −21.9, and −29.4 dBFS in the order above. These are levels from the actual loopback takes. The PR includes separate, short reference renders. The examples reopened in the app, and the reference tests matched at host buffer sizes of 1, 77, 256, and 1,000 frames.

## What helped while patching

- **Signal cues and quick probes.** Pitch and gate travelled as four-voice bundles in Glass Chords. An output tooltip showed the count, and Voice Merge made the fold into one signal explicit. Right-clicking an output let me add a meter in one step. This is familiar from [VCV Rack's polyphonic cables](https://vcvrack.com/manual/Polyphony).
- **Useful defaults with an explanation.** Broken Pulse ran from the bar without a clock cable. The Phase input's clock mark and tooltip explained which default it used. This is a useful counterpart to inspecting a normalled input in hardware or a pre-cord in [Bitwig Grid](https://www.bitwig.com/userguide/latest/special_connections/). The [normals explainer](../learn/normals.md) gives LoomDSP's rules.
- **Direct controls and a smaller playable surface.** Typing `1.6k` on the Cutoff knob committed 1,600 Hz and Undo restored 1,400 Hz. MIDI Learn mapped CC 74 and the mapping survived reopening. Grouping Tidal Echo preserved the outer LFO assignment and exposed Cutoff on the tile. With a note held in a separate Synth voice patch, its context menu swapped the filter from SVF to Ladder and the signal path stayed wired.
- **Feedback is shown where it happens.** Tidal Echo's delay starts at 278 ms, about an eighth note at 108 BPM. The compiler reports one automatic one-sample delay on the return from Delay Line to Mixer In 2. Run `loom check examples/patches/tidal-echo.ron` on the example checkout to see where. The published example calls the return control **Feedback** and caps it below unity.

## Where the session slowed down

| Finding | Repeatable check | Follow-up |
|---|---|---|
| First sound | Choose **New** and click **Hold note**. The button becomes **Release note** with only Transport and no audible route. Wiring the built-in Synth voice took MIDI Note, Synth voice, and Audio Out plus four cables. | [Starter and output-state evidence](https://github.com/brylie/LoomDSP/issues/505#issuecomment-7007081635) |
| Familiar search terms | Open a fresh module browser and type `VCO`, `VCA`, `VCF`, or `ADSR`. Each returned an empty list. Searching `saw`, `amplifier`, `svf`, and `envelope` found the tiles. | [Search evidence and suggested cases](https://github.com/brylie/LoomDSP/issues/594#issuecomment-7007082116) |
| Matrix target and depth | In Tidal Echo, pin LFO Out and select **Matrix** at the outer level. The line lands on the visible Cutoff knob, while the row shows **SVF #12: Cutoff** after reopening, with **+25%** depth. A 1,000 Hz cutoff with that bipolar assignment can span about 178 to 5,623 Hz on its full logarithmic range. | [#834, visible Matrix target and range](https://github.com/brylie/LoomDSP/issues/834) |
| Labels while following a route | With that line pinned at 100% in a 1,440 × 900 logical window, wheel down four steps over the patch centre. The selected route and control shape remained visible as port and knob labels turned into grey bars. | [Overview evidence at intermediate zoom](https://github.com/brylie/LoomDSP/issues/527#issuecomment-7007082623) |
| Inserting a module in a running path | Try putting a processor between Pulse Out and SVF In in Broken Pulse. A normal module add places a tile. Wiring into and out of it takes a separate cable edit on each side. The slot-swap workflow shows the value of keeping a signal path connected during a change. | [#835, inline cable insertion](https://github.com/brylie/LoomDSP/issues/835) |
| Teaching notes and visible names | Save an example as a new patch. The current format rewrites RON and drops source comments. In the session, saving a new patch through the control channel also gave it the chosen filename in the title while the stored patch name was still `Untitled`. The contributed files have explicit names and descriptions. | [Teaching notes and patch name evidence](https://github.com/brylie/LoomDSP/issues/512#issuecomment-7007084116) |

### Audio output handoff

After releasing a held note, the app switched from **BlackHole 2ch** (48,000 Hz, 512 frames) to **System default**, which chose **MacBook Pro Speakers** (44,100 Hz, 512 frames). The reply reported advancing callbacks, zero overruns, and one stream error. A later snapshot showed increasing callbacks and the same error count. The kept error list was empty. A fresh launch on the speakers reported zero errors.

To repeat, start with BlackHole as the output, take two control-channel snapshots, send `{"cmd":"audio","device":"default"}`, then take two more snapshots after the switch. Record the output name, rate, callbacks, overruns, and stream errors. This single observation describes the reported handoff. The loopback recordings cover steady playback on BlackHole. [Device-test follow-up](https://github.com/brylie/LoomDSP/issues/601#issuecomment-7007083138)

## Priorities suggested by the session

1. **Editable musical steps and pitch.** Euclidean Rhythm and Chance made a quick moving groove. A pitch and gate sequencer with a scale quantiser would make it easy to place specific notes and rests. [Integration patch suggestion for #307 and #308](https://github.com/brylie/LoomDSP/issues/307#issuecomment-7007083625)
2. **A sample that plays.** A Buffer and Buffer reader would let the same rhythmic workflow use recorded audio, percussion, or voice. Start with a short WAV and clocked playback, then extend it to recording and overdub. [#836, Buffer and Buffer reader](https://github.com/brylie/LoomDSP/issues/836) · [#311, Buffer writer](https://github.com/brylie/LoomDSP/issues/311)
3. **Useful finished sounds.** The looping Function generator, finished delay, and reverb are useful next additions. Tidal Echo can teach the primitives while those are developed. [Module catalogue](../product/module-catalogue.md)
4. **A way to use and share the music.** Playable starters, in-app WAV recording, and self-contained patches with short audio previews would help players get a result into another project and learn from each other. The existing [Examples list](https://github.com/brylie/LoomDSP/issues/511), [recording](https://github.com/brylie/LoomDSP/issues/503), and [community library](https://github.com/brylie/LoomDSP/issues/247) work cover those directions.

Existing issue milestones and estimates were kept. The three new issues are on the [LoomDSP project board](https://github.com/users/brylie/projects/7), with proposed work awaiting an agreed spec.

| New issue | Milestone | Size | Priority |
|---|---|---|---|
| [#834](https://github.com/brylie/LoomDSP/issues/834) | M2 | M | P1 |
| [#835](https://github.com/brylie/LoomDSP/issues/835) | M2 | M | P2 |
| [#836](https://github.com/brylie/LoomDSP/issues/836) | M2 | L | P1 |
