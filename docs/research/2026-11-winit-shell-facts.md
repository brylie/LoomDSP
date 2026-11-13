# The winit Shell: Facts Checked at Our Versions, and the Linux Screen Reader

- **Kind:** Research snapshot (reading dependencies' source and docs, and a dependency check). Non-normative.
- **Date:** 2026-11-13. `iced` 0.14.0, `iced_runtime` 0.14.0, `iced_wgpu` 0.14.0, `iced_winit` 0.14.1, `iced_widget` 0.14.2, `wgpu` 27.0.1, `winit` 0.30.13, `raw-window-handle` 0.6.2, `accesskit` 0.25.2, `accesskit_macos` 0.27.2, `accesskit_winit` 0.34.2, `accesskit_unix` 0.24.1, `rfd` 0.17.2. `cargo-deny` 0.20.2, with the RustSec database at `af7b3ec3` (2026-11-12). AccessKit's `main` at `dea43841`.
- **Related decisions:** [ADR-0041](../adr/0041-own-ui-layer.md) (our own UI layer), ADR-0006 (permissive dependencies), ADR-0018 (clean room: these are dependencies, so their source may be read).
- **Related:** [plan](../dev/leaving-iced-plan.md) step 2.2, and its list of facts marked †; [the AccessKit note](2026-11-accesskit-macos-gaps.md#what-accesskit_winit-brings), whose "not checked" list this answers.

Here "the shell" means our window code. Which crate holds which part (the `winit` shell in `loom-ui`, the Iced half in `loom-app`'s bridge) is the plan's decision, in step 2.2.

It is a dated snapshot. Do not update it to follow later releases; write a new snapshot instead.

## Questions

1. Plan step 2.2 replaces `iced::application` with our own `winit` shell. Do the facts it rests on hold at the versions we pin?
2. `accesskit_winit` with its default features fails `cargo deny check` through the Linux adapter. Which advisories, is there a way round them, and what are the options?

## Summary

1. **Iced's integration example exists at tag 0.14.0 and uses our versions' API** (the plan's † fact). One part of the plan's wording is wrong: Iced does not draw into our render pass. `iced_wgpu::Renderer::present` makes its own encoder and passes and submits them itself. A frame is two submissions on one queue: ours (clear, canvas), then Iced's, which loads what we drew.
2. **For the same pixels, the shell must choose the surface as Iced does today:** a format that is not sRGB, because the workspace turns on Iced's `web-colors`, and a clear colour in raw components. The example chooses sRGB, which would change every pixel.
3. **Some of Iced's per-frame work lives in a private module of `iced_winit`, so our code must do it:** the redraw event and the updates that follow it, a redraw request for a later time, the cursor icon, and the input method. Iced's inline preedit text has no public API.
4. **A `UserInterface` has no public hit test.** Iced decides which layer gets the pointer inside its tree. Keeping overlays above the canvas needs something in the canvas's place in that tree.
5. **winit 0.30 has `resumed`, not `can_create_surfaces`** (that is 0.31). `accesskit_winit` 0.34.2 must be created before the window is shown, and it panics if the window is visible.
6. **The Linux screen reader:** three "unmaintained" advisories, RUSTSEC-2026-0370, -0371, and -0372, on `zvariant`, `zbus_names`, and `zvariant_derive`. Their authors merged them into `zbus` 6 and flagged the old names; none is a vulnerability. `atspi`, and so `accesskit_unix`, is still on `zbus` 5, and no feature avoids them. AccessKit ignores the same three IDs in its own `deny.toml`.

## 1. Drawing an Iced `UserInterface` ourselves

### The integration example

The example is at `examples/integration` in Iced's repository, tag 0.14.0 (not published to crates.io). It builds a winit 0.30 `ApplicationHandler`, its own `wgpu` 27 device and surface, draws a scene, then draws Iced on top. It uses `iced_wgpu::{Engine, Renderer}`, `iced_runtime::user_interface::{UserInterface, Cache}`, and `iced_winit::{Clipboard, conversion}`. We read it to learn the API; the shell's design comes from our own runtime (ADR-0041, spec section 2).

### Who owns what in a frame

`iced_wgpu::Renderer::present(clear, format, view, viewport)` (`iced_wgpu` `lib.rs` 165 to 180) creates a command encoder (140), opens render passes of its own (425, 513, 605), and submits (175). With `clear` set to `None`, its first pass loads what is in the view (435 to 447).

So the shell owns the surface, the surface texture and its view, an encoder and pass for the clear and the canvas, and `frame.present()`. Iced owns the rest. A frame is:

1. our encoder: clear, then the canvas; submit;
2. `present(None, ..)` on the same view, which submits Iced's work after ours;
3. `window.pre_present_notify()`, then `frame.present()`.

### The canvas's viewport and scissor

Today iced_wgpu sets the viewport to the canvas's bounds times the scale, and the scissor to that rectangle cut to the window and snapped, before `CanvasPrimitive::draw` (`lib.rs` 544 to 565), then resets both (577 to 591). The shell's pass must set the same before `CanvasRenderer::draw`. The canvas's bounds come from the `places` walk, as `screenshot.rs` already finds them.

The window-wide drag line is still an Iced `shader` widget. Shader primitives keep working through our own `UserInterface` draw, since `present` runs them.

### The device and the surface

What `iced::application` chooses today (`iced_wgpu` `window/compositor.rs`):

- adapter: high performance unless `WGPU_POWER_PREF` says otherwise (85 to 89);
- format: the first one that is not sRGB (98 to 139). The workspace turns on `web-colors`, which sets `GAMMA_CORRECTION` to false (`iced_graphics` `color.rs` 30 to 41);
- alpha mode: PostMultiplied, else PreMultiplied, else Auto (127 to 137);
- limits: the defaults, then the downlevel ones, each with `max_bind_groups: 2` and `max_non_sampler_bindings: 2048`; no features (150 to 175);
- present mode AutoVsync, and `desired_maximum_frame_latency: 1` (336). The example uses 2;
- the clear colour is the theme's background packed as raw components, again because of `web-colors`.

The canvas renderer must be built for the same format.

### The renderer type

`iced::Renderer` in our build is the fallback enum of `iced_wgpu` and `iced_tiny_skia`, because the workspace turns on `tiny-skia` (`iced_renderer` `lib.rs` 25 to 32). Whatever builds the `UserInterface` builds `iced::Renderer::Primary(iced_wgpu::Renderer::new(..))` and matches `Primary` to present. Where that code lives is the plan's decision (step 2.2). When step 2.3 drops `tiny-skia`, the match goes. `Engine::new` takes the antialiasing setting; today's app turns it off, so `None`. `iced` does not re-export `iced_winit`, so `iced_winit` and `winit` become direct dependencies of whichever crate uses them. Both are in the lock already, one `winit`, with no fork.

### Building, updating, and the cache

`iced_runtime` `user_interface.rs`: `build` (93), `update` with a batch of events, giving one status per event (184), `draw` (482), `operate` (539), `relayout` (582), and `into_cache` (588). The view borrows the app, so a `UserInterface` cannot live across an `update`: build, use, keep the cache, rebuild after messages, as `ui_test.rs` does today. The cache keeps the widget tree, which owns the text paragraphs, so taking the cache before `present` is safe.

### Iced's per-frame work that our code must do

`iced_winit`'s event loop (`lib.rs`) does this, partly in a private module (`mod window`, `window.rs` 199 to 275):

1. It collects events until `AboutToWait`, then runs one `update` with all of them (1135 to 1165).
2. It hands each event and its status to the subscriptions (1188 to 1199). Our input listeners are plain functions of an event, a status, and a window, so our code calls them with the statuses `update` gave. `listen_with` leaves out `RedrawRequested` (`iced_futures` `event.rs` 35 to 40); keep that.
3. On a redraw: it relays out if the size changed, then runs `update` with `RedrawRequested`, up to three times while that gives messages or moves the layout, running the app's update between (815 to 860). The canvas reads that event, and buttons choose their look during it (#460). The example throws these messages away; the shell must not.
4. After the draw, it applies the redraw request (`NextFrame` now, `At(t)` later, as a text field's caret uses), the input method, and the cursor icon (962 to 966). Public pieces: `conversion::mouse_interaction`, `conversion::ime_purpose`, and winit's `set_ime_allowed`, `set_ime_cursor_area`, `set_cursor`. The inline preedit overlay has no public API.

Events convert with `conversion::window_event(event, scale, modifiers)` (`conversion.rs` 169). The shell keeps the modifiers from `ModifiersChanged`. The cursor is `Cursor::Available` after a move and `Unavailable` before the first move and after the pointer leaves.

### Widget operations, tasks, and subscriptions

- Focus, scroll into view, and the `places` walk run on our own `UserInterface` through `operate`, with `black_box` for an operation with an answer, and a loop while `finish` returns `Outcome::Chain`, as `ui_test.rs` does today. No `Task` is needed.
- A `Task` can still be run without the application (`iced_runtime` `task.rs` 489), but window actions (screenshot, exit) have no runner outside `iced_winit`. The bridge's remaining uses all have direct replacements.
- Subscriptions need `iced_futures::Runtime`, and window frames, close requests, and `listen_with` are fed by `iced_winit`'s own broadcasts, which we would have to make. Plan step 2.1b part (d) replaces them with event sources that wake the loop.

### Hit testing

`Stack::update` gives each layer the cursor, top first. Once an upper layer reports a mouse interaction, the layers below see the cursor as "levitating" (`iced_widget` `stack.rs` 246 to 274). A `UserInterface` has no call to ask whether a point is over an overlay. So to send canvas input to `CanvasInput` directly and keep overlays above it, something must stand in the canvas's place in the tree and see what reaches it, or the order must come from our own layout data.

## 2. winit 0.30.13 and `accesskit_winit` 0.34.2

### `ApplicationHandler`

- `resumed` is required, and is where the window and surface are made. It can come twice. `can_create_surfaces` is winit 0.31.
- `EventLoop::with_user_event().build()`, `run_app`, `create_proxy`. `EventLoopProxy` is `Clone`, and on macOS `Send` and `Sync` for a `Send` event, so the runtime's waker can wrap it; each wake-up arrives in `user_event`.
- `ControlFlow::WaitUntil(t)` for deadlines; a deadline arrives as `new_events(StartCause::ResumeTimeReached)`.
- `exit()` is on `ActiveEventLoop`. The runtime's `Driver::exit` has no event loop in hand, so it sets a flag that the handler acts on after the turn.
- `request_redraw()` gives `RedrawRequested`.
- `WindowAttributes::with_visible(false)`, `with_inner_size`, `with_min_inner_size`, `with_title`; later `set_visible(true)`.
- `iced_winit` already makes each window hidden and shows it after the first build (`lib.rs` 326, 698). Hidden, then shown, changes nothing the user sees.

### `accesskit_winit`

- 0.34.2 is the release for `accesskit` 0.25, `accesskit_macos` 0.27, and `winit` 0.30 (it needs 0.30.5 or later). It is also the latest release.
- **It must be created before the window is shown.** All three constructors say so (`lib.rs` 149 to 250), and `with_direct_handlers`, which the other two call, panics if the window reports it is visible (197). winit's docs say `is_visible` is not implemented on X11, but its code is: `platform_impl/linux/x11/window.rs` 1158 to 1160 returns the real visibility. Only Wayland returns `None`. So the check fires on macOS, Windows, and X11, and never on Wayland.
- Built with the event loop proxy, it sends `accesskit_winit::Event` values to `user_event`: the initial tree asked for, an action asked for, or the screen reader left. So the shell's user event is one enum, with a `From` for AccessKit's event. `with_mixed_handlers` can answer the first request at once with the last tree, and keeps the proxy for actions.
- Every window event goes to `adapter.process_event` first.
- On macOS it uses `accesskit_macos::SubclassingAdapter`, the adapter `a11y/macos.rs` uses today, and ignores the deactivation handler.

## 3. macOS details the shell meets

- **Menu bar.** The app has no menu bar of its own; `menus.rs` holds the in-window menus (ADR-0021). The user sees winit's default app menu, on by default, which `iced_winit` keeps. The shell keeps it too.
- **Cmd+Q quits with no close request,** today and with the shell: winit closes the windows on `applicationWillTerminate`, which sends no `windowShouldClose`. `docs/design/files.md` already lists it as a check by hand. Not a regression; the shell is a place to fix it later.
- **File drop:** one `DroppedFile` per file.
- **Close:** today `exit_on_close_request(false)` and a close-request subscription. In the shell, `CloseRequested` goes to the runtime, and the app quits through `Driver::exit`.
- **Window:** 1440 by 900 logical, no minimum size, placed by the system, resizable.
- **High DPI:** `ScaleFactorChanged`, then `Resized`. The shell reconfigures the surface, rebuilds the viewport, lays the interface out again, and tells the runtime the scale changed.
- **Dialogs:** `rfd` 0.17.2's `set_parent` shows a dialog as a sheet on macOS, with a completion handler, so it need not block. A winit window gives the handles it needs.

## 4. The Linux screen reader and `cargo deny`

### The chain

`accesskit_winit` 0.34.2 → `accesskit_unix` 0.24.1 → `atspi` 0.29.0 and `accesskit_atspi_common` 0.21.1 → `zbus` 5.19.0, with `zvariant` 5.15.1, `zvariant_derive` 5.15.1, and `zbus_names` 4.3.5. AccessKit's `main` has not changed this since the release.

### The advisories

| Advisory | Crate | What happened |
|---|---|---|
| [RUSTSEC-2026-0370](https://rustsec.org/advisories/RUSTSEC-2026-0370) | `zvariant` 5.15.1 | Merged into `zbus` 6.0 as `zbus::wire` |
| [RUSTSEC-2026-0371](https://rustsec.org/advisories/RUSTSEC-2026-0371) | `zbus_names` 4.3.5 | Merged into `zbus` 6.0 as `zbus::names` |
| [RUSTSEC-2026-0372](https://rustsec.org/advisories/RUSTSEC-2026-0372) | `zvariant_derive` 5.15.1 | Its derives now come from `zbus` 6.0 |

All three were published 2026-10-16 as "unmaintained", with no patched version, by the zbus authors themselves, after a last release of each crate marked as merged. None is a vulnerability. `zbus` 5 itself has no advisory.

**Who has moved:** `zbus` 6 (latest 6.2.1, minimum Rust 1.89) is the replacement. `atspi` has not moved: its latest release, 0.30.0, and its `main` are on `zbus` 5.5, with no change open, and [odilia-app/atspi#382](https://github.com/odilia-app/atspi/issues/382) shows its maintainers weighing their minimum Rust version. AccessKit has not moved either: [AccessKit/accesskit#834](https://github.com/AccessKit/accesskit/pull/834) ignores the same three IDs in its own `deny.toml`, because its minimum Rust version, 1.87, is below what `zbus` 6 needs. A fix needs `atspi` to release on `zbus` 6 first, then AccessKit. Neither has a date. Our minimum Rust version, 1.99, is not what blocks it.

### Features

`accesskit_unix` always depends on `zbus` 5. Its `async-io` or `tokio` feature only chooses where its own tasks run (it runs its own executor on a thread of its own, so the app needs no async runtime). Dry runs against our `deny.toml`, for a Linux target:

| `accesskit_winit` features | `cargo deny` |
|---|---|
| default | advisories fail on the three IDs; licences pass |
| `tokio` instead of `async-io` | the same failure, and it adds `tokio` while keeping `async-io` |
| `default-features = false`, `rwh_06` | passes; on Linux the adapter builds and does nothing |

### Licences

About 25 crates are new on Linux, each MIT, Apache-2.0, or both, and `cargo deny check licenses` passed in every run. The `async-io` stack and `quick-xml` are in our lock already, through Iced's `smol` feature. `accesskit_winit` itself is Apache-2.0.

### Other paths, none better

- Using `accesskit_unix` directly brings the same crates.
- Moving `atspi` and AccessKit onto `zbus` 6 ourselves would mean forks from git sources, which `deny.toml` denies, and a port across a major version we would maintain.
- A Cargo feature off by default, or leaving Linux out of `[graph] targets`, would hide the advisories from `cargo deny` without deciding them.

## Options for the Linux screen reader

- **A. Ignore the three IDs in `deny.toml`, each with its reason.** Linux gets a screen reader (Orca, over AT-SPI) with the shell, as macOS and Windows do. This is what AccessKit does, and how `deny.toml` already treats three other unmaintained crates in the UI stack. Ignoring by ID still reports any new advisory on these crates. Costs: about 25 crates and a D-Bus client on Linux, one more thread there, three lines to remove later, and the comment in the root `Cargo.toml` that keeps `zbus` 5 out (for Iced's theme detection) must say why a screen reader is worth it when theme detection was not.
- **B. Leave the Linux adapter out at first** (`default-features = false`, `rwh_06`). `cargo deny` passes as it is. Orca sees nothing on Linux until upstream moves, with no date. It needs a tracking issue and a line in the spec, so the gap is decided. Either way round is a one-line change.
- **C. Help `atspi` move to `zbus` 6 upstream.** It fixes the cause for everyone, but the timing is not ours. It can go with A or B, not instead of them.

Decided 2026-11-13: option A, recorded in the [UI layer spec](../design/ui-layer.md)'s decisions and in the [plan](../dev/leaving-iced-plan.md), step 2.2.

## Not checked

- Orca on a Linux machine, and Narrator on Windows. No machine was at hand.
- How the shell's frame performs against `iced::application`'s. That is a gate of step 2.2 itself.
