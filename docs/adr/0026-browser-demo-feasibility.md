# ADR-0026: A browser demo waits for a spike; pre-rendered audio examples come first

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-25
- **Related requirements:** FR-APP-02, FR-APP-05, NFR-PORT-01, NFR-PORT-02, NFR-WEB-01 to 04
- **Specification:** none yet. A spike would produce one.

## Context

The website ([site/](../../site/README.md)) invites people to try LoomDSP, but today they must download a build or compile it. A demo in the browser would remove that step. Is one feasible now?

What we have:

- `loom-core`, `loom-modules`, `loom-patch`, and `loom-rt` build for `wasm32-unknown-unknown`. CI checks this (NFR-PORT-01).
- ADR-0003 describes a runtime in one AudioWorklet. It is deferred.

What we do not have:

- A web audio host. There is no AudioWorklet glue, no JavaScript bridge for the control queues of ADR-0001, and no loading of patches in the browser.
- A browser UI. The canvas is an Iced widget drawn with wgpu (ADR-0010). Whether it runs well on WebGPU in current browsers is the first go/no-go question in the [roadmap](../product/roadmap.md#web-target), and nobody has tested it.
- Web MIDI input.
- An owner. The roadmap asks for someone in the community to own the web target.

The roadmap defers the web target until after P3. It would also pull effort from the desktop app while the architecture is still moving.

## Options considered

1. **Build the web target now.** Gives a live demo. Costs a host, a UI port, and ongoing upkeep while the patch format and API still change. Conflicts with the roadmap.
2. **Do nothing.** Costs nothing. Visitors cannot hear LoomDSP without installing it.
3. **Pre-rendered audio examples.** Render example patches to audio with `loom render` and embed them on the website. Works today, needs no new code, and is reproducible because renders are deterministic. Visitors hear the sound but cannot change it.
4. **A small engine-only spike later.** A page that loads one fixed patch, runs the engine in an AudioWorklet, and exposes a few knobs as plain HTML controls. No canvas. It tests the audio half of the web target cheaply and answers the "is the WASM engine fast enough" question.

## Decision

Do not build a browser demo yet. Do option 3 now, as a small follow-up:

- Render a few short clips from `examples/patches/` with `loom render`. Use FLAC or Ogg Opus to keep them small.
- Store the clips and the exact command for each in `site/`, and add a play button to each example on the page.
- Re-render them when a patch or module changes the sound. A test or script should flag clips that fall out of date.

Plan option 4 as a spike after P3, or sooner if a community member offers to own it. Its exit question: does a fixed patch run glitch-free in an AudioWorklet on current Chrome, Firefox, and Safari? Only if the answer is yes do we look at the UI half (Iced on WebGPU).

## Consequences

- The website can play real LoomDSP audio at once, with no new engine work.
- Clips can go stale when modules change. We must regenerate them, and this adds a small task to module changes that alter sound.
- The portability rules (NFR-PORT-01, NFR-PORT-02) stay in force, so the spike remains cheap.
- No change to ADR-0003, which stays deferred. If the spike succeeds, a new ADR confirms or replaces it.
