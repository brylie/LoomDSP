# UX Review: Learning DSP and Sharing Patches

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01, on `main` at fd6f938 (after #485).
- **Follows:** [the generative review](2026-11-generative-ux-review.md), [the producer review](2026-11-producer-ux-review.md), and [P1 UX audit, round 2](2026-11-p1-ux-audit-2.md).
- **Used by:** issues #511 to #514, and a comment on #240. Decisions go in specs, ADRs, or the product docs.

This review reads the desktop app as someone learning digital signal processing by patching: they build a filter from parts to see why it rings, they find out what a one-sample delay does in a feedback loop, and then they want to share what they made, with the reasons, so others can learn from it too. They are part student (Sanna's learners), part Mira, and the reason for product principle 11, "every decision teaches". Do not update this snapshot later; write a new one after the fixes.

## Method

- Followed the learner's path: find an example, read the help for a module, follow a link to the theory, probe a signal, read a value, change the patch, write down why, and share it.
- Read the help overlay (`crates/loom-app/src/help.rs`), how it handles links (`Message::HelpLink` in `main.rs`), the generated help pages (`docs/modules/`), the explainers (`docs/learn/`), how a patch is saved (`crates/loom-patch/src/lib.rs`), and the files, patch metadata, and panels specs.
- Counted the help pages that link to explainers, and checked where Open starts and where the examples live.
- Searched the open issues for the learning library, the tutorial, metadata, sub-patches, the spectrum view, and the Expression module.

**Limits.** No learner was watched. Findings come from the code, the docs, and screenshots.

Priority means: **High** blocks a task or misleads the user; **Medium** slows the user or looks broken; **Low** is polish.

## Summary

LoomDSP already teaches more than most tools. Its oscillators are built from an explicit phase, so the learner sees what an oscillator is. Feedback loops are drawn with a dashed one-sample delay and named in the status bar. Every port and control explains itself on hover, with its range. And 14 explainers in `docs/learn/` cover the theory behind the design, from phase to block-size invariance.

The weak point is the bridge between them. The app cannot show the explainers: every link to one in help says "Linked document is not bundled". A learner cannot write down why a patch works, because Save drops the comments in a patch file and the app has no place for notes. Pitch reads as numbers like 0.075, not notes. The scope has no scale. The examples, the best teaching material, sit in a folder the Open dialog does not start in. And sharing what you made means sending a bare `.ron` file: no picture, no recording, no credit or licence yet.

| # | Area | Finding | Priority |
|---|---|---|---|
| 1 | Theory | Help links to explainers do not open: "Linked document is not bundled" | High |
| 2 | Notes | Save drops every comment; the app has no place to write why | High |
| 3 | Examples | The examples are hard to find: Open starts in Documents | Medium |
| 4 | Values | Pitch shows as numbers (0.075), not notes or Hz | Medium |
| 5 | Seeing | The scope has no grid or scale, and cannot be paused | Medium |
| 6 | Sharing | No picture of a patch, no recording, and no credit or licence yet | Medium |
| 7 | Depth | Writing your own DSP needs Rust; the Expression module is M4 | Low |
| 8 | Depth | No way to look inside a larger block until sub-patches (M2) | Low |

## Findings

### 1. The explainers cannot be opened from the app (High)

39 of the 47 module help pages link to an explainer, most often [triggers and gates](../learn/triggers-and-gates.md), [phase and cycles](../learn/phase-and-cycles.md), and [pitch](../learn/pitch.md). The help overlay renders these links, and a click sends `Message::HelpLink`. The handler opens the link only if it names another module's page; for anything else it sets the status to "Linked document is not bundled: ../learn/pitch.md" (`crates/loom-app/src/main.rs`). So the app shows the learner, inside its own help, that the theory exists and that they cannot reach it.

The explainers are the heart of principle 11, and they are plain Markdown, the same as the help pages that already ship.

Direction: bundle `docs/learn/` with the help pages, and open an explainer in the same overlay, with a Back step. Keep relative links working inside explainers. A link to the web (a paper, a standard) should open the system browser, with the address shown first.

### 2. Save drops every comment, and there is no place for notes (High)

The example patches teach through comments: "Filter envelope, scaled to +3 octaves (pitch units: 0.1 per octave)." Save writes the patch again from its data with RON's pretty printer (`Patch::save`, `crates/loom-patch/src/lib.rs`), so every comment is lost. A learner who opens an example, changes a knob, and saves it under their own name has thrown away its explanations. A learner who writes their own comments in a text editor loses them the next time the app saves.

Inside the app there is no place to write anything: no description field yet (patch metadata, M2, #244), and no notes on the canvas. For someone sharing a patch to teach, the reasons are the content.

Direction (a spec decision): keep notes as data in the patch, not as comments, so they survive a save. Two kinds cover most needs: the patch description (already in the metadata spec) and short notes placed on the canvas beside the modules they explain. Notes should go with copy and paste, and show in the browser and help when the patch is an example. Until then, say in files.md that Save does not keep comments.

### 3. The examples are hard to find (Medium)

The six example patches are the best teaching material in the app. Open starts in the current file's folder or in Documents ([files.md](../design/files.md)); the examples live in `examples/patches` beside the app, inside the app bundle on macOS. A module's help has an Open example button, but there is no list of all examples. The learning library (#240) has no milestone.

Direction: an Examples list in the window (a menu or a panel), with each example's name and one-line description, opening as an untitled copy as today. Give #240 a milestone.

### 4. Pitch reads as numbers (Medium)

Pitch is 0.1 per octave, with 0.0 as middle C (ADR-0007). That is a good design for patching, and [pitch](../learn/pitch.md) explains it. But a learner who hovers a pitch cable or watches a live value sees "0.075" or "-0.5 to 0.5", and must work out that 0.075 is the A above middle C (9 semitones up, 9/12 of 0.1). Nothing shows a note name or a frequency beside it.

Direction: wherever a pitch value is shown (live values, the inspector, hover text, probe readouts), add the nearest note and its frequency, for example "0.075 (A4, 440 Hz)". Pitch Constant already speaks in semitones; this applies the same idea everywhere else.

### 5. The scope cannot be read (Medium)

The Scope draws a trace and a zero line. It has no grid, no value at the top and bottom, and no time per division, so a learner cannot read an amplitude, a period, or a DC offset from it. There is no way to freeze it to study one moment. A spectrum view (#109, M2) will help with filters; it is not here yet.

Direction: a light grid with labelled divisions (the window time and the value range), and a hold toggle that freezes the trace. Keep the drawing cheap: a grid is a few rectangles, drawn once per tile.

### 6. Sharing gives a bare file (Medium)

To share a patch today, a learner sends the `.ron` file. The app gives them nothing else to share:

- **No picture.** `--screenshot-window` renders the window to PNG, but it is a developer flag. There is no Export image in the window.
- **No sound.** In-app recording is #503.
- **No credit or licence yet.** Authors, tags, a description, and a Creative Commons licence chooser are agreed for M2 (#244, #268), not built.
- **No single file with assets** (FR-SHARE-01, M4) and no link that opens in a browser (the web target is Later).

The patch format is readable text, the placeholder for unknown modules keeps data from newer versions, and examples open as copies: those are good foundations for sharing.

Direction: an Export image command that renders the canvas (or the selection) to PNG with the code that already exists, and later a short "share" bundle: the patch, an image, and a recording.

### 7. From patching to writing DSP (Low)

A learner who understands a filter by patching it next wants to write one. Today that means a Rust module, built from source (the SDK and third-party modules are M4). The Expression module (MT-MATH-04, a typed formula on its inputs, checked at load and never run as code) is M4. For a DSP learner it is the step between the two, and it fits the clean-room and safety rules.

Direction: consider moving the Expression module to M3. No separate issue; this is a catalogue decision.

### 8. Looking inside (Low)

UC7 (learn from a shared patch) depends on opening a sub-patch to see how it works. Sub-patches are #51 (M2). Until then, a patch can only be read flat. No action beyond the existing issue.

## What works well

- **Explicit phase.** Oscillators are shaped from a phase, so the learner sees what an oscillator is made of.
- **Feedback made visible.** A loop gets a dashed one-sample delay, and the status bar says "1 feedback loop, 1 automatic 1-sample delay (dashed)".
- **Everything explains itself.** Every port and control has a label, a one-sentence description, its range, and its stable name on hover.
- **Help pages** list every port and parameter with units and ranges, with hand-written notes and an example.
- **Probes.** Right-click an output or a cable to add a scope or a meter, already connected.
- **The explainers** in `docs/learn/` are clear, short, and honest about why a choice was made.
- **Determinism** means a patch someone shares sounds the same on every machine, so a lesson works for everyone.

## Issues

Grouped to keep the backlog small. Priority and size are on the LoomDSP project board.

| Issue | Findings | Milestone | Priority | Size |
|---|---|---|---|---|
| [#511](https://github.com/brylie/LoomDSP/issues/511) Learn from inside the app: open explainers from help, and an Examples list | 1, 3 | M2 | P1 | M |
| [#512](https://github.com/brylie/LoomDSP/issues/512) Notes that survive a save: patch description and notes on the canvas | 2 | M2 | P1 | M |
| [#513](https://github.com/brylie/LoomDSP/issues/513) Readable values: note names and Hz for pitch, and a scope grid and hold | 4, 5 | M2 | P2 | M |
| [#514](https://github.com/brylie/LoomDSP/issues/514) Export image of the canvas or selection | 6 | M3 | P2 | S |
| [#240](https://github.com/brylie/LoomDSP/issues/240) (comment): the learning library needs a milestone, and examples lose their comments on save | 2, 3 | | | |
| Not filed here: credit and licence (#244, #268), recording (#503), sub-patches (#51), spectrum (#109), the Expression module (catalogue) | 6, 7, 8 | | | |

## Suggested order

1. Open explainers from help (1, #511). The content exists and the overlay already renders Markdown; this is bundling and one link handler.
2. Notes that survive a save (2, #512), with the patch description from the metadata work (#244).
3. The Examples list (3, #511), with the learning library (#240).
4. Note names for pitch and the scope grid (4, 5, #513).
5. Export image (6, #514), when the metadata and recording work make sharing a whole package.
