# Our Own UI Stack Instead of Iced: Trade-offs

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-06. Repository facts were checked at commit `61009d4`. Facts about other projects were checked through web search summaries on this date, not against their source.
- **Related decisions:** [ADR-0009](../adr/0009-ui-stack-iced.md) (Iced for the UI), [ADR-0010](../adr/0010-own-patch-canvas.md) (our own patch canvas), [ADR-0028](../adr/0028-app-accessibility-waits-for-iced.md) (accessibility waits for Iced)
- **Related questions:** [OQ-13](../product/open-questions.md#oq-13-embedding-iced-in-plugin-windows) (embedding in plugin windows), [OQ-14](../product/open-questions.md#oq-14-iced-version-churn) (Iced version churn)

This snapshot asks what LoomDSP would gain and lose if it stopped using Iced, or any ready-made Rust UI framework, and drew its whole UI itself. It records evidence and a suggested position. It decides nothing. A change of UI stack needs a new ADR that supersedes ADR-0009.

It is a dated snapshot. Do not update it to follow later releases; write a new snapshot instead.

## How to use this document

- Entries marked † come from web search summaries or the author's general knowledge. They were not checked against the project's source or release notes on the snapshot date. Check them before relying on them.
- No other project's source code was read (ADR-0018).

## Where we are today

LoomDSP already owns a large part of its UI.

| Layer | Who provides it today |
|---|---|
| Window, event loop, high-DPI scaling | Iced, through `winit` 0.30 |
| GPU device and surface | Iced, through `wgpu` 27 (`iced_wgpu`) |
| Text shaping and layout | Iced, through `cosmic-text` 0.15 and `cryoglyph` |
| Widget layout | Iced |
| Standard widgets | Iced: `pick_list`, `scrollable`, `tooltip`, `text_editor`, `markdown`, `container`, `mouse_area`, `pin`, `rule` |
| Theming | Iced, with our tokens in `crates/loom-app/src/style.rs` |
| Patch canvas: tiles, ports, cables, panels, hit-testing, pan and zoom | Ours: `crates/loom-canvas`, one wgpu primitive (`canvas.wgsl`), about 6,400 lines |
| What the user sees, as plain data | Ours: view models such as `inspector.rs` and `App::ui_snapshot()` (ADR-0021) |
| Headless tests, screenshots, control channel | Ours, built on `iced_runtime` and `iced_wgpu` (`ui_test.rs`, `screenshot.rs`, `control/run.rs`) |

The app source in `crates/loom-app/src` and `crates/loom-canvas/src` is about 70,000 lines. Only a small set of Iced widget types appears in it. So leaving Iced does not mean replacing a large widget library. It means taking over the foundations under those few widgets: input, focus, text editing, layout, overlays, and the runtime that the test tools drive.

ADR-0009 listed a custom wgpu or Vello renderer as an option and noted the cost: "We build text input, focus, and accessibility ourselves." ADR-0010 rejected a canvas on raw wgpu outside Iced (its option 4) for the same reason. This snapshot asks the same question for the whole app.

## What we would gain

1. **Accessibility on our own schedule.** ADR-0028 defers the app's accessibility tree until Iced supports AccessKit. Screen reader users are blocked on work outside our control (NFR-UX-06). With our own stack we could connect `accesskit_winit` directly, and build the tree from the plain-data view models that ADR-0028 already asks every view to carry.
2. **Plugin window embedding (OQ-13).** A plugin draws inside a window that the DAW owns, and the host drives the event loop. Iced expects to create its own window through `winit`. The P0 spike never tried embedding. A stack built for this from the start avoids a retrofit.
3. **No pre-1.0 churn (OQ-14).** Each Iced release may break our widgets, `ui_test.rs`, the screenshot path, and the control channel. With our own stack, breaking changes happen only when we choose them.
4. **One rendering model.** Panels, the inspector, and the canvas could all draw with the canvas's instanced shapes. ADR-0010 already names "one visual language" across the canvas, front panels, and plugin GUIs as a goal.
5. **Full control of redraw and idle cost.** NFR-PERF-04 asks that an unchanged patch does not redraw. Without a framework, nothing redraws unless our code asks for it. Today we measure Iced's behaviour with `--measure-idle`.

## What we would lose or have to build

1. **Text editing.** Shaping and drawing can stay on `cosmic-text`. What we would build is everything on top: cursor movement, selection, clipboard, undo inside a field, multi-line editing (we use `text_editor`), and input method editors (IME) for languages typed through composition. This is the largest hidden cost and the easiest to underestimate.
2. **Focus, overlays, and layering.** Dropdowns, tooltips, and the panels in `overlays::Overlays` need z-order, clipping, dismissal on an outside click, and a keyboard focus order (NFR-UX-01). Each piece is small. Together they are a long tail of edge cases that users notice.
3. **Layout.** We would need a layout engine for the inspector, browser, and toolbar. `taffy` † is a permissive flexbox and grid engine that would cover this, but the glue code is ours.
4. **Test and QA tooling.** The headless `Ui` harness sends real events to `App::view`. `--dump-ui`, `--screenshot-window`, and the control channel run on `iced_runtime`. All of them would be rewritten and trusted again before the new UI could be.
5. **Accessibility as our sole responsibility.** We would gain the schedule but own every bug. ADR-0028 warns that a tree which disagrees with the screen is worse for a screen reader user than none.
6. **Web target.** ADR-0009 counts Iced's WebGPU support as a reason to choose it. `winit` and `wgpu` also run in browsers, so the target stays possible, but the glue is ours.
7. **Focus of a small team.** LoomDSP is in early alpha with a small team. Time spent on a toolkit is time not spent on modules, the patch format, or live performance. New contributors would also learn a toolkit with no documentation outside this repository.

## How Iced says it is run

The Iced book has a [philosophy chapter](https://book.iced.rs/philosophy.html), read on 2026-11-06. In summary, it says:

- Iced is its author's personal project, not a community effort, a business, or a brand.
- The author builds and reviews every feature, and does not hand features to other contributors.
- New contributors get no special welcome, and adoption is not a goal.
- Breaking changes come whenever the design improves. A stable API is not a goal.

What this means for LoomDSP:

1. **Churn is policy, not a phase (OQ-14).** We should not expect Iced to settle at 1.0 into a stable API. Each upgrade may break our widgets, the `Ui` harness, the screenshot path, and the control channel.
2. **We cannot plan on contributing what we need.** ADR-0028 waits for Iced to add AccessKit. Under this policy, that comes only if and when the author builds it. Offering a pull request is unlikely to speed it up.
3. **Documentation will stay thin.** Promotion and onboarding are not goals, so our own docs must explain the parts of Iced we depend on.
4. **The policy is stated openly.** That is useful. It lets us judge the risk now rather than discover it later.

None of this is a criticism of the project. It is a fact about the dependency that we should plan around, the same as its licence or its platform support.

## What exists around Iced (†)

Checked through web search on 2026-11-06. None of this was verified against source.

- **Upstream Iced** showed no announcement of merged AccessKit support. An old upstream issue discusses using `accesskit_winit` with Iced. Sources: [iced issue 552 (mirror)](https://git.tdem.in/iced-rs/iced/issues/552), [AccessKit](https://github.com/AccessKit/accesskit).
- **Forks and downstream toolkits** are adding AccessKit to Iced themselves. `toddy-iced` is a vendored fork whose documentation claims an AccessKit tree for its built-in widgets. The COSMIC desktop's `libcosmic`, built on Iced, has said it is integrating AccessKit. Sources: [toddy-iced 0.6.2](https://docs.rs/crate/toddy-iced/0.6.2), [libcosmic comment](https://lemmy.linuxuserspace.show/comment/775084).
- **AccessKit's** own description says its platform adapters do not yet cover every kind of element or every property in its schema. Source: [AccessKit](https://github.com/AccessKit/accesskit).
- **Vizia,** a Rust UI framework made with audio plugins in mind (MIT), offers a `baseview` window backend for hosting inside a DAW window, and uses AccessKit. Whether AccessKit works under the `baseview` backend was not confirmed. ADR-0009 noted it has no dedicated node editor, which matters less now that the canvas is ours. Sources: [vizia on docs.rs](https://docs.rs/vizia/), [vizia on lib.rs](https://lib.rs/crates/vizia).

The forks show that AccessKit on Iced is feasible. They do not change the cost ADR-0028 named for keeping a fork ourselves.

## The realistic shape of "our own"

Dropping a framework does not mean writing everything from nothing. An own stack would most likely be `winit` + `wgpu` + `cosmic-text` + `taffy` + `accesskit`, all permissive (ADR-0006). That is close to what Iced already assembles, without its widget library and its Elm-style runtime. So the real question is narrower: **should we own the widget, focus, and event layer?** The renderer question was mostly settled by ADR-0010.

## Suggested position

Keep Iced. Continue the ADR-0010 pattern: take ownership piece by piece where Iced is clearly the limit, as with the canvas, and keep what the user sees in plain-data view models so the toolkit stays replaceable.

Revisit when either trigger fires:

1. **The plugin embedding spike (OQ-13) fails,** or needs a fork of Iced to pass. Include Vizia and a thin own stack in that spike.
2. **Accessibility becomes a scheduled goal while upstream Iced still has no AccessKit support.** ADR-0028 already asks for a check at each Iced upgrade.

If a trigger fires, the comparison to make is three-way: an own stack on the crates above, Vizia, and an Iced fork. Measure each against the same gates: NFR-PERF-04, embedding in a host window, and one screen-reader pass over the inspector.
