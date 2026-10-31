# Frame Meter Specification

The measure line of `--bench`, `--measure`, and `--measure-idle` says why each frame was drawn and how much CPU the app used, so a developer can find what keeps the app busy in one run.

- **Kind:** living specification. Developer tooling.
- **Status:** Agreed ([#252](https://github.com/brylie/LoomDSP/issues/252), 2026-10-28). Implemented.
- **Decision behind it:** none. It extends the frame-rate report described in [P1 canvas benchmarks](../benchmarks/p1-canvas.md).
- **Use case and requirements:** NFR-PERF-04 (no redraw when nothing changes), the Redraw rules of the [panels spec](panels.md).
- **Background:** none needed.
- **Prior art:** none needed.
- **Issues:** [#252](https://github.com/brylie/LoomDSP/issues/252). Related: [#196](https://github.com/brylie/LoomDSP/issues/196) (the `LiveFrame` cause found by hand), [#90](https://github.com/brylie/LoomDSP/issues/90) (stats bar updates).
- **Code:** the report: `crates/loom-app/src/bench.rs`. The frame counters: `loom_canvas::stats`.

## Goal

"The idle patch draws 72 frames a second. Why?" In #195 the answer took several runs, cutting the patch apart until the frames stopped. The cause was the `LiveFrame` subscription. One line that counts frames by cause would have said so at once.

## Behaviour

Each measure line, and the final line, gains two fields after the existing ones:

```text
fps=72.0 avg_ms=13.89 worst_ms=16.70 frames=720 ... causes=tick:20,live:700,meters:0,control:0,input:0,redraw:0,forced:0,other:0 cpu=4.1
```

### `causes`

The kinds of message that asked for frames, with how many frames each asked for, as totals since the start, like `frames`.

| Kind | What asked for the frame |
|---|---|
| `tick` | The stats bar timer (`Message::Tick`) |
| `live` | The live value frames for modulated knobs (`Message::LiveFrame`) |
| `meters` | Telemetry: meters and scopes (`Message::Meters`) |
| `control` | The control channel (`Message::ControlFrame`, and commands) |
| `input` | A window event from the user: pointer, keys, resize, focus |
| `redraw` | The canvas asked for a redraw itself, with no message (hover text, an animation) |
| `forced` | `--bench` or `--measure`, which redraw every frame |
| `other` | Any other message |

### `cpu`

The CPU time the whole app process used over the line's interval, user plus system, as a percent of one core, with one decimal: `100 * (Δuser + Δsystem) / Δwall`, where the three differences are taken over the line's interval. It is the same number Activity Monitor or `top` shows, so it can go over 100 on a machine with several cores. On the per-second lines the interval is the last second. On the final line it runs from the first line to the end, like `fps`.

## Rules

1. **A frame counts once for each kind that asked for it.** Every message handled since the last drawn frame adds its kind to the next frame's set. When the frame is drawn, each kind in the set counts 1. So the counts can add up to more than `frames`. *Why:* a frame asked for by both a tick and a live value is caused by both. Counting it only under the first one would hide the second, and the second may be the one the developer is looking for.
2. **Fixed order, every kind listed.** The kinds are always printed in the order of the table, with 0 for those that asked for nothing. *Why:* scripts and agents can read the line with a fixed pattern, and a 0 is information too (`live:0` says the live path is quiet).
3. **Measuring adds no frames.** The counts and the CPU time are read on the report's own thread, as the frame counters are today. Counting a message is an increment in the app's update, and asks for nothing. An idle patch may draw startup frames, then settles at `fps=0.0`.
4. **The whole process.** `cpu` covers every thread: UI, audio, telemetry, and MIDI. It comes from `getrusage(RUSAGE_SELF)` on macOS and Linux, and from `GetProcessTimes` on Windows. *Why:* idle cost is what the computer pays, whatever thread pays it, and this way the check needs no `ps` or Activity Monitor running beside it.

## Acceptance criteria

- [x] The measure line and the final line show `causes` and `cpu` in the format above. The `bench.rs` line test covers both fields.
- [x] A frame asked for by two kinds counts once under each (unit test).
- [x] An idle patch with no assignments and `--no-audio --measure-idle` has no recurring frame cause after startup. Checked in a three-second run: the second report showed `fps=0.0` and the cumulative frame count stayed at 3.
- [x] A patch with an enabled assignment and `--fake-audio` shows `live` frames. Checked with `examples/patches/modulation.ron --fake-audio --measure-idle`: the final line counted 165 live frames.
- [x] `--measure-idle` on an idle patch, with the stats bar timer off ([#90](https://github.com/brylie/LoomDSP/issues/90)), prints `fps=0.0` after startup.
- [x] AGENTS.md and [P1 canvas benchmarks](../benchmarks/p1-canvas.md) explain the new fields.

## Out of scope

- Showing causes or CPU in the stats bar. The stats bar is for players; this is for developers.
- CPU per thread. The audio thread's own load is already in the stats bar (FR-PERF-04, [panels spec](panels.md#audio-load-colour)).
- GPU time.

## Open points

- None.
