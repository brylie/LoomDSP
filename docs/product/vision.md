# Vision

## Summary

LoomDSP is an open-source modular audio environment written in Rust. It is a desktop application for musical experimentation and live performance. Users build instruments and effects by connecting small DSP modules on a visual canvas. They hide the wiring behind a custom front panel and play the result on stage with MIDI controllers.

LoomDSP is not a DAW. It is a grid for building **living musical systems**: patches that play, evolve, and arrange themselves through modulation, randomness, texture, sequencing, looping, and logic. Everything is a module, connected the same way: synths and effects, modulators, note effects, sequencers, loopers, and even arrangement (ADR-0011).

A flagship example: a patch that plays group A's loops round robin, each twice, then hands over to group B after two rounds, then returns to A. A clip launcher cannot express this; in LoomDSP it is ordinary patching with counters and selectors, and the same building blocks scale from loops to parts to whole songs ([use case UC8](personas-and-use-cases.md), [arrangement specification](../design/arrangement.md)).

People share their patches and modules, as with VCV Rack, so an ecosystem of creators grows around the core. Exporting patches as plugins (CLAP, VST3) or standalone apps may come later, but is deliberately out of scope for now.

## The problem

Sound designers who want deep modular control face a split today:

1. **Modular environments** (Bitwig Grid, Reaktor, Max/MSP, VCV Rack) give deep control. But most are closed source, tied to one host, or hard to turn into a clean, shareable instrument.
2. **Plugin frameworks** (JUCE, iPlug2, Faust, Rust plugin frameworks) produce distributable plugins. But they require writing code, and the design loop is slow compared to patching.
3. **Live performance** with modular software is fragile. Complex patches cause CPU spikes, and editing during a show risks dropouts.

## The vision

> Patch it, play it live, then share it. All from one open-source tool.

A musician should be able to start from an empty canvas, build a system of sequencers, loopers, and modulation, give it a tidy panel, map it to a MIDI controller, and play it on stage without worry. When it is ready, they share the patch, and others can open it, play it, and learn from it.

## Priorities

1. **A working instrument for live performance.** Patching, MIDI controllers, front panels, presets, and stable audio come first.
2. **Encapsulation and panels.** These make large patches manageable and playable.
3. **Living systems.** Loopers, groups and next actions, counters and logic, scenes, sequencers, and modulators: patches that play and arrange themselves.
4. **Sharing.** Self-contained patch files, a patch library, and a module ecosystem, following VCV Rack.

**Kept out on purpose, for now: plugin and standalone export.** Generative live performance and plugin export pull in different directions. Plugins bring host integration, automation, several formats, and per-platform builds and signing. Doing both at once risks doing neither well. We revisit export after living systems are proven (roadmap, "Later").

## Product principles

Each principle includes its reason, so we can judge trade-offs later.

1. **Audio never glitches because of the UI.** Editing a patch during a performance must be safe. Reason: LoomDSP is a live instrument. One dropout on stage destroys trust.
2. **Patch anything to anything.** Signals share one numeric format, one rate, and normalised ranges, in the spirit of Eurorack and Bitwig Grid. Types guide the user with colour but never block a connection. Reason: the creative value of modular comes from unexpected connections.
3. **Encapsulation is the main tool for managing complexity.** Any selection can collapse into a sub-patch with its own ports and panel. Reason: large patches become unreadable fast.
4. **The panel is a product, not a debug view.** Front panels should look finished enough to perform with and to ship. Modules, sub-patches, and patches all use the same panels, so a finished sub-patch works like a built-in module. Reason: a performer needs clear controls, and export is only useful if the result is pleasant to use.
5. **Rust end to end.** Engine, UI, file format, and export tooling are all Rust. Reason: one language and one type system across the whole stack. No bridge to JavaScript, no duplicated data model. See ADR-0005.
6. **Open standards, without being limited by them.** We use open formats (CLAP, VST3 now that its SDK is MIT, WAM2, WebGPU via wgpu, WASM) where they help. We do not shape the core architecture around the browser. Reason: open standards help the ecosystem, but the desktop live instrument is the main product.
7. **Patches are data, not code.** Loading a patch never runs arbitrary code. Reason: users will share patches. Safe sharing needs this guarantee.
8. **Everything is a module.** Modulators, note effects, sequencers, loopers, device-style processors, and arrangement logic are modules in the grid, not separate parts of the program. Reason: one way to connect things means anything can modulate, sequence, or process anything else, and every feature saves, undoes, and exports the same way. See ADR-0011.
9. **Polish worthy of the stage.** The patch canvas aims for the clarity and finish of Bitwig Studio's Grid: compact modules, clear cables, controls and live displays on the modules themselves (direct manipulation, ADR-0013). Reason: musicians judge an instrument by how it feels to play and to read at a glance. See ADR-0010.
10. **Permissive and AI-friendly.** Apache-2.0 code, only permissively licensed dependencies, and a preference for upstream communities that welcome AI-assisted contributions. Where no suitable crate exists, we build the primitive ourselves and publish it as a crate. See ADR-0006. Reason: LoomDSP is built with heavy AI assistance, and we want to give back to the projects we depend on.
11. **Every decision teaches.** LoomDSP is also a way to learn modular sound design, for musicians and for developers. When we settle how something works, we write down the theory behind it, not only the rule: what the signal is, why the obvious approach fails, and how Eurorack, VCV Rack, and Bitwig Grid handle it. These explainers live in [`docs/learn/`](../learn/README.md), linked from specifications and code. Reason: many people come to modular to learn, including the people who build LoomDSP. A rule without its theory teaches nothing, and the next person who does not know the "why" will undo it.

## Goals (version 1.0)

- G1. Build instruments and effects from a core library of primitive modules, with sample-accurate feedback.
- G2. Play patches live with MIDI controllers, presets, and a locked performance mode, with no dropouts.
- G3. Collapse any part of a patch into a reusable sub-patch with its own front panel.
- G4. Share living systems: self-contained patch files that others can open, play, and learn from, and a growing library of community patches and modules.
- G5. Provide a documented module SDK so others can add DSP modules in Rust.
- G6. Publish reusable primitives (for example real-time helpers and the canvas) as standalone crates.
- G7. Grow the module library toward the breadth of Bitwig Studio's Grid modules, modulators, note effects, and devices, all as modules in one grid.

Later goals, not scheduled: export patches as CLAP and VST3 plugins and standalone apps (ADR-0004), and run in the browser with WAM2 export. The strongest case for the browser is education: a teacher shares a link, and students start patching with no install.

## Non-goals (version 1.0)

- **A DAW.** No arranger timeline, mixer, or track list. Arrangement happens inside the grid: loopers with groups and next actions, counters and logic, scenes, and modulation curves (ADR-0011).
- **Plugin and standalone export.** Deferred until living systems are proven (see "Later goals").
- **The browser as the primary interface.** Browser support is optional and deferred.
- **A JavaScript or webview UI.** See ADR-0005.
- **A general text-based DSP language.** We may import Faust later.
- **Running third-party plugins inside a patch.** This adds a large surface for crashes and licensing issues.
- **Cloud accounts, a hosted marketplace, or collaboration servers.** Sharing works through files first.
- **Mobile apps.**
- **Video or visual synthesis.**

## Success measures

These are first proposals. We should revisit them after Phase 1.

| Measure | Target for 1.0 |
|---|---|
| Time for a new user to build a playable subtractive synth from primitives, following the tutorial | Under 20 minutes |
| Reference patch (about 100 modules, 8 voices) at 48 kHz, 128-sample buffer | Under 25% of one core on the reference machine |
| Live soak test: 1 hour of playing, preset changes, and patch edits | Zero dropouts, no memory growth |
| MIDI controller latency (input event to audible change) | One audio buffer plus driver latency, no extra |
| Self-arranging set | A 30-minute set that arranges itself (loopers, groups, counters, scenes) runs live with zero dropouts |
| Sharing | A patch saved on one machine, with its loops and samples, opens and plays the same on another machine and platform |
| Community | At least 3 modules contributed by non-core contributors. At least one published crate used by another project |

## How we build it

- **AI-assisted development.** Planning and implementation lean heavily on AI tools. Docs, ADRs, and clear requirement IDs help both people and AI agents work from the same context.
- **Open backlog.** A public GitHub project with milestones per roadmap phase, and labels such as `good first issue`, `help wanted`, and `hacktoberfest` to welcome new contributors.
- **Small, usable phases.** Each phase ends with something people can try. This keeps momentum and reduces burnout risk.

## Prior art and inspiration

| Project | What we learn from it |
|---|---|
| Bitwig Grid | Polymorphic, colour-coded signals that all interoperate; phase-driven design; modulation built into every parameter; the reference for canvas polish and feel |
| Bitwig Studio modulators and devices | All in scope, but as modules in one grid |
| Bitwig Studio clip launcher | Loopers with groups and next actions. In LoomDSP these are modules, so logic can drive them: for example, a counter that starts another group after N round-robin loops |
| Eurorack | Patch anything to anything; voltage conventions such as 1V per octave |
| NI Reaktor | Deep encapsulation, polished front panels |
| Max/MSP and RNBO | Patch-then-export workflow |
| Plogue Bidule | Plain, reliable graph patching for performance |
| VCV Rack and Cardinal | Open module ecosystem, and the sharing model we follow: people swap patches and modules |
| Faust | Whole-graph compilation for sample-accurate feedback |
| HexoDSP, FunDSP | Rust DSP building blocks and graph design |
| Elementary Audio | Graph diffing and reconciliation on the control thread |
