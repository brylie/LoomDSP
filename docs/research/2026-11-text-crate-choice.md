# Choosing the Text Crate: `cosmic-text` or Parley

- **Kind:** Research snapshot (reading dependencies' source, release notes, and crates.io, and one probe). Non-normative.
- **Date:** 2026-11-11. `cosmic-text` 0.15.0 (in our lock, under Iced) and 0.19.0 (newest), `cryoglyph` 0.1.0, `glyphon` 0.9.0 to 0.12.0, `parley` 0.12.0 (newest) and 0.11.1, `fontique` 0.12.0, `harfrust` 0.12.0, `swash` 0.2.10, `accesskit` 0.25.2, `wgpu` 27.0.1, `winit` 0.30.13, `vello` 0.12.0. macOS 27.0.1 (26A434) on Apple silicon.
- **Related decisions:** [ADR-0041](../adr/0041-own-ui-layer.md) (our own UI layer, and its decision part 3: text comes from existing crates), ADR-0006 (permissive dependencies), ADR-0018 (clean room).
- **Related:** [UI layer spec](../design/ui-layer.md) section 8, rules 4 to 8; [plan](../dev/leaving-iced-plan.md) step 2.4; OQ-14 (Iced churn).
- **Code:** `examples/text-font-probe/`, a probe outside the workspace that uses no LoomDSP code.

It is a dated snapshot. Do not update it to follow later releases; write a new snapshot instead.

## Question

Spec section 8, rule 8 asks the maintainer to choose the text crate for LoomDSP's own UI layer, between `cosmic-text` and Parley, by six criteria in this order:

1. IME composition in a single-line field.
2. Reporting text runs, caret, and selection to AccessKit.
3. One font system shared by the canvas and the panels.
4. How its glyphs are drawn.
5. It picks the same system font as today.
6. Licences that pass `cargo deny`, and versions that match our `winit` and `wgpu`.

The choice is for the long run. So each criterion is answered twice where versions matter: **now**, while Iced 0.14 is in the app and fixes our `wgpu` at 27 and `cosmic-text` at 0.15, and **after phase 4**, when Iced is gone and both can move. The cost of the move from one to the other is a cost, not the deciding fact. "Iced uses `cosmic-text`" counts for nothing by itself.

## Summary

| Criterion | `cosmic-text` | Parley |
|---|---|---|
| 1. IME composition | Not built in: we hold the preedit text ourselves | Built in: `PlainEditor` composes, and gives the area for the candidate window |
| 2. AccessKit text | Not built in: we write it | Not built in since 0.12: we write it |
| 3. One font system | Yes, now: the canvas can use Iced's own font system | No, until the panels leave Iced |
| 4. Glyph drawing | Now: `cryoglyph`, already in our tree, maintained by Iced's authors (`iced-rs/cryoglyph`), so text drawing moves at Iced's pace until phase 4. After phase 4: `glyphon`, or our own atlas | Our own atlas, now and later. Vello needs `wgpu` 30 |
| 5. Same font as today | Yes on macOS: same face, same widths. Same code path elsewhere | No on macOS: Helvetica by default; with `system-ui` the same face, but widths differ by up to 4% |
| 6. Licences and versions | Passes. Adds no crate now | Passes. Adds 35 crates |

1. **No `glyphon` release builds with our `wgpu` 27.** The plan's step 2.4 named `glyphon` as "the usual choice". Its releases target `wgpu` 25, 28, 29, and 30, not 27.
2. **Parley 0.12 removed its AccessKit integration.** It moved into Parley's `editor` example "to be copied and adapted". Copying it would be porting framework code, which this project does not do. With either crate we write that part.
3. **Parley's IME support is real and built in.** `cosmic-text` has none.
4. **On macOS, `cosmic-text` matches today exactly, and Parley does not.** Widths decide layout, hit tests, and `locate` answers, so a mismatch fails the parity gate.
5. **Recommendation:** `cosmic-text`. It is cheaper now and no worse after phase 4. Its one real loss is criterion 1, and we pay for that with our own preedit handling, about 150 lines. See [the question](#the-question-for-brylie).

## 1. IME composition in a single-line field

**Parley.** `PlainEditor` in `parley` 0.12.0 holds a composition beside the text (`src/editing/editor.rs`):

- `set_compose(text, cursor)` (line 351) starts or updates the preedit text, which replaces the selection when composing starts. `cursor` places the caret and selection inside the preedit.
- `finish_compose()` (line 416) commits it, and `clear_compose()` (line 401) drops it.
- `ime_cursor_area()` (line 867) gives the rectangle for the candidate window: the preedit text's bounds if there is one, otherwise the selection on the focused line.
- Each edit updates the composition range (`update_compose_for_replaced_range`, from line 191).

This matches rule 4 point by point: the shell's preedit and commit events map to `set_compose` and `finish_compose`, and the caret area goes back to the shell.

**`cosmic-text`.** Neither 0.15.0 nor 0.19.0 has a composition: no "preedit" or "compose" anywhere in `src`. Its `Editor` works through an `Action` enum (`src/edit/mod.rs`, line 24) with no composition actions. We would hold the preedit text ourselves: insert it as a styled span, keep its range and its caret, remove or commit it on the shell's events, and give the caret's rectangle from `Buffer::layout_cursor` (0.15, line 491) or `Buffer::cursor_position` (0.19, line 1247). Estimate: about 150 lines, medium confidence. Iced's own text input has such handling; we would not read or copy it.

**Not checked here:** composition with a real Japanese or Chinese input method. That needs a person at the keyboard and is already a phase 3 acceptance criterion. Reading the APIs settles what each crate offers.

## 2. Text runs, caret, and selection to AccessKit

**What AccessKit 0.25.2 needs** (`src/lib.rs`): `Role::TextRun` nodes (line 65), each with `character_lengths` (line 2357), `character_positions` (line 2408), `character_widths`, and `word_starts` (line 2388), and a `TextSelection` of two `TextPosition`s (lines 771 and 784) on the field.

**Parley.** Up to 0.11.1 Parley had an `accesskit` feature, on `accesskit` ^0.24 (crates.io dependencies of 0.11.1), one version behind ours. The 0.12.0 release notes say, under Removed: "the `accesskit` feature, and the AccessKit integration it enabled, were removed", and "the integration now lives in the `editor` example, where it is easier to evolve and can be copied and adapted by consumers" (#716). Copying it is porting framework code, so we write our own. Parley gives what that needs: `Cluster` spans a full grapheme cluster from 0.12 (release notes, #715), with `text_range()` (`src/layout/cluster.rs`, line 159), `advance()` (line 188), `is_word_boundary()` (line 209), and `visual_offset()` (line 445). 0.12 also added `Cursor::upstream_cluster`, `Cursor::downstream_cluster`, and `PlainEditorDriver::set_selection` for this use (release notes, #716).

**`cosmic-text`.** No AccessKit code in 0.15 or 0.19. Each `LayoutGlyph` has its byte range in the text (`start` and `end`, `src/layout.rs` lines 16 and 18) and its place and width (`x`, `w`). Grapheme clusters and word starts would come from `unicode-segmentation`, already in our tree.

**Cost:** about the same for both, 200 to 300 lines of ours, medium confidence. Parley's clusters are already graphemes, which saves a little.

## 3. One font system for the canvas and the panels

**Now.** Iced 0.14 keeps one global `cosmic-text` 0.15 `FontSystem`, behind a public function (`iced_graphics` 0.14.0, `src/text.rs` line 116, `font_system()`). A canvas on `cosmic-text` 0.15 can use that same font system. Fonts are loaded once, and fallback resolves the same way for the canvas and the panels.

A canvas on Parley has its own `FontContext` from `fontique`, beside Iced's. Two databases of the system's fonts, loaded twice, until the panels leave Iced in phase 3. They resolve families by different rules (criterion 5), so the canvas and the panels could pick different faces for the same request.

**After phase 4.** Either crate gives one font system, since the panels then use ours too.

## 4. How glyphs are drawn

**Versions, from crates.io:** `glyphon` 0.9.0 needs `wgpu` ^25 and `cosmic-text` ^0.14; 0.10.0 (2025-12-18) needs `wgpu` ^28 and `cosmic-text` ^0.15; 0.11.0 needs `wgpu` ^29 and `cosmic-text` ^0.18; 0.12.0 (2026-07-09) needs `wgpu` ^30 and `cosmic-text` ^0.19. None builds with `wgpu` 27. `cryoglyph` 0.1.0, "a fork of glyphon for iced" (its `Cargo.toml`), needs `wgpu` 27 and `cosmic-text` 0.15. It is MIT OR Apache-2.0 OR Zlib and already in our tree under `iced_wgpu`. `vello` 0.12.0 (2026-11-11) needs `wgpu` ^30.

**`cosmic-text`, now:** `cryoglyph` 0.1 draws `cosmic-text` buffers into a `wgpu` 27 render pass, and adds nothing to our tree. It is Iced's crate, so it moves with Iced's releases. **After phase 4:** `glyphon`, at the release that matches our `wgpu` then, or our own atlas fed by `cosmic-text`'s `SwashCache`.

**Parley, now and after phase 4:** no atlas crate takes Parley's glyph runs. Our own atlas, rasterising with `swash` (in our tree) from the font data and variation coordinates Parley gives. Or Vello, once our `wgpu` reaches 30, with the compositing cost ADR-0041 names for Masonry.

**Our own atlas, costed in lines from what it must do.** `cryoglyph` 0.1.0 is 1,350 lines (`text_render.rs` 407, `text_atlas.rs` 344, `cache.rs` 250, `shader.wgsl` 128, the rest 221). It is a reference point only; we would not read it as a design.

| Part | What it does | Lines | Confidence |
|---|---|---|---|
| Rasterising | `cosmic-text`: `SwashCache` gives each glyph's image. Parley: a `swash` scaler from the font data and variation coordinates | 20 / 60 | High / medium |
| Packing | Place glyph images in the atlas with `etagere` (MIT OR Apache-2.0, in our tree) | 40 | High |
| Eviction | Drop glyphs no frame used, oldest first, when the atlas is full | 100 | Medium |
| Upload | Make the texture, grow it, write new glyphs with the queue | 120 | Medium |
| Colour glyphs | A second, RGBA atlas for emoji and colour fonts, beside the coverage atlas | 80 | Low |
| Subpixel positions | Cache a glyph per horizontal offset bin, such as quarters of a pixel | 30 | High |
| Shader and pipeline | Instanced quads that sample the atlas; the canvas already has an instanced pipeline to follow | 250 | Medium |
| **Total** | | **640 / 680** | |

That is code rule 1 says we should not write when a crate does it. With `cosmic-text` it is needed only if we drop `cryoglyph` before phase 4 or `glyphon` after. With Parley it is needed in every case short of Vello.

## 5. The same system font as today

**How today's font is found.** Spec rule 6 says the default font is "the system sans-serif, found by `cosmic-text`'s font system". The source says something more specific. `cosmic-text` 0.15.0 sets the sans-serif family to "Open Sans" (`src/font/system.rs`, line 159). Where that is not installed, shaping falls back through a fixed list per platform (`src/font/fallback/`): `.SF NS` first on macOS (`macos.rs`, line 32), Segoe UI on Windows (`windows.rs`), and Noto Sans, then DejaVu Sans, then FreeSans on Linux (`unix.rs`, line 32). So on a machine with Open Sans installed, today's app uses Open Sans. The spec's rule 6 is corrected in this change to say so.

**The probe, on macOS 27.0.1.** `examples/text-font-probe` measures six tile labels at 9, 11, and 13 px through five paths. "Today" is Iced 0.14's paragraph, called exactly as `loom-canvas` measures tile text (`Font::DEFAULT`, simple shaping, built without Iced's `fira-sans` feature, as the app is). The face is the PostScript name of the font each glyph came from. Widths are logical pixels.

| Path | Face | "Cutoff" 9 px | "Resonance" 11 px | "Attack 12 ms" 13 px |
|---|---|---|---|---|
| Today (Iced 0.14) | `.SFNS-Regular` | 23.871 | 51.439 | 70.383 |
| `cosmic-text` 0.15 | `.SFNS-Regular` | 23.871 | 51.439 | 70.383 |
| `cosmic-text` 0.19 | `.SFNS-Regular` | 23.871 | 51.439 | 70.383 |
| Parley, `sans-serif` | `Helvetica` | 23.854 | 55.650 | 75.137 |
| Parley, `system-ui` | `.SFNS-Regular` | 24.741 | 52.019 | 69.024 |

All 18 measurements agree to the third decimal for the three `cosmic-text` paths. The full output is in the probe's run; these are three of its rows.

**Parley's default is Helvetica on macOS.** `fontique` 0.12.0 maps the generic `sans-serif` to Helvetica (`src/backend/coretext.rs`, line 28), Arial on Windows (`dwrite.rs`, line 33), and fontconfig's `sans-serif` on Linux (`fontconfig.rs`, line 837). It maps `system-ui` to "System Font" and `.SF NS` on macOS (`coretext.rs`, line 32), Segoe UI on Windows (`dwrite.rs`, line 37), and fontconfig's `system-ui` on Linux (`fontconfig.rs`, line 841).

**With `system-ui`, Parley finds the same face but sets it differently:** from 2% narrower to 4% wider than today, varying by label and size. A likely cause, not proven: Parley passes the font size to its shaper (`parley_engine` 0.12.0, `src/shape/shaper.rs`, line 360), which lets `harfrust` 0.12 apply the size-dependent tracking in Apple's fonts (it handles the `trak` table, `src/hb/ot_shape.rs`). `cosmic-text`'s simple shaping does not. Turning off pixel rounding (`quantize: false`) changed nothing. Whatever the cause, these widths move layout, hit tests, and `locate` answers, so the `--screenshot-window` parity gate would fail.

**Windows and Linux, from source, not run:**

- **`cosmic-text`:** the same crate and the same fallback list as today, so the same face and widths, as long as the version stays at 0.15. On 0.19 the list is the same; 0.19 also changed basic shaping to "fallback to a default font" (CHANGELOG, #498), which the probe shows changes nothing on macOS.
- **Parley `system-ui`:** Segoe UI on Windows, the same family as today's fallback; widths not known. On Linux, fontconfig's `system-ui` alias, which may not be Noto Sans or DejaVu Sans; not known.

The probe prints the same lines on any platform. A run on Windows and Linux would settle both.

## 6. Licences and versions

- **Licences:** `cosmic-text` MIT OR Apache-2.0; `cryoglyph` and `glyphon` MIT OR Apache-2.0 OR Zlib; `parley` Apache-2.0 OR MIT.
- **`cargo deny check` against the root `deny.toml`:** passes for the probe (which holds `cosmic-text` 0.15 and 0.19, `parley` 0.12, and `iced_graphics` 0.14), and for a scratch manifest holding `parley` 0.12 alone: `advisories ok, bans ok, licenses ok, sources ok`.
- **`cosmic-text` now:** 0.15.0 and `cryoglyph` 0.1.0 are already in our lock, so it adds no crate. **After phase 4:** 0.19.0 with `glyphon` 0.12.0 needs `wgpu` 30.
- **Parley now:** 78 crates in its tree. 35 crate names are not in our lock today: `fontique`, `parlance`, `parley`, `parley_data`, `parley_emoji`, `parley_engine`, eleven `icu_*` crates, `objc2-core-text`, `yeslogic-fontconfig-sys`, and the `zerovec`, `yoke`, `zerofrom`, `tinystr`, `litemap`, `writeable`, and `displaydoc` support crates, among others. It brings `harfrust` 0.12 beside the 0.3.2 Iced uses, and `skrifa` 0.44, which we already have beside 0.37. It needs no `wgpu` or `winit`. **After phase 4:** the same.

## Costs of each choice

### What each choice builds

| Step | `cosmic-text` | Parley |
|---|---|---|
| 2.4b, canvas text | Draw tile text with `cryoglyph` 0.1 and Iced's font system: a `Painter` and a `TextMeasure` over them. No new crate | Our own atlas (about 680 lines), a second font system, and `system-ui` with a fix for the width differences, or a parity gate that accepts moved text |
| 3.3, text fields | Our preedit handling (about 150 lines), and our AccessKit text glue (200 to 300) | Our AccessKit text glue (200 to 300). Composition is built in |
| Phase 4, Iced removed | Move to the `glyphon` release that matches our `wgpu`, or keep `cryoglyph` while it matches; bump `cosmic-text` from 0.15 (0.19 changed `set_text`'s signature) | Nothing text-specific; possibly Vello |

### Churn (OQ-14)

Leaving Iced was partly about churn, so the text crate's own pace matters.

- **`cosmic-text`:** five minor releases in the last year, each breaking under 0.x: 0.15.0 (2025-10-30), 0.16.0 (2025-12-29), 0.17.0 (2026-01-29), 0.18.0 (2026-02-19), 0.19.0 (2026-04-22), plus four patch releases. Most are small: 0.17 removed `Attrs::matches`, and 0.19 made the `Buffer` setters lazy, which changed their signatures. No release in the last six and a half months.
- **Parley:** seven minor releases in twelve months: 0.6.0 (2025-10-06), 0.7.0 (2025-11-24), 0.8.0 (2026-03-27), 0.9.0 (2026-04-21), 0.10.0 (2026-06-01), 0.11.0 (2026-06-26), 0.12.0 (2026-10-09). The 0.12 release notes mark 19 entries "Breaking change". They include a new crate split (`parley_engine`), the AccessKit removal, `Cluster` becoming a grapheme cluster, and removed and renamed layout metrics.
- **`glyphon`:** one release per `wgpu` major: three in the last year (0.10.0 on 2025-12-18, 0.11.0 on 2026-04-13, 0.12.0 on 2026-07-09).

### The rest of a single-line field (section 8, rules 2 and 3)

Not a criterion of rule 8, but lines we owe. From each editor's API: `cosmic-text`'s `Edit` trait and `Action` and `Motion` enums (`src/edit/mod.rs`, `src/cursor.rs`; the same pieces in 0.15 and 0.19), and Parley's `PlainEditor` and its driver (`src/editing/editor.rs`, 0.12.0).

| Feature | `cosmic-text` | Parley |
|---|---|---|
| Caret and selection | `cursor`, `set_cursor`, `selection`, `set_selection` (lines 204 to 213); `Selection::Normal`, `Word`, and `Line` | A selection with `select_*` methods, `selection_geometry` (line 830), `cursor_geometry` (line 857) |
| Word movement | `Motion::PreviousWord`, `NextWord`, `LeftWord`, `RightWord` (`cursor.rs`, from line 119) | `move_word_left` and `move_word_right` (line 534), `select_word_*`, `delete_word` (line 259), `backdelete_word` |
| Home and End | `Motion::Home`, `SoftHome`, `End` (`cursor.rs`, lines 103 to 107) | `move_to_line_start` (line 463), `move_to_line_end`, `move_to_text_start`, `move_to_text_end` |
| Select all, double-click | No select-all action: set the selection from the buffer's start to its end, about 5 lines. `Action::DoubleClick` and `TripleClick` (line 47) | `select_all` (line 554), `select_word_at_point` (line 676) |
| Clipboard | `copy_selection` (line 307), `delete_selection`, `insert_string` (line 315). The clipboard itself is ours, through effects | `selected_text` (line 817), `insert_or_replace_selection` (line 166). The same |
| Undo inside the field | Change tracking: `start_change`, `finish_change`, and `apply_change` (lines 322 to 328), and a `Change` that reverses itself. The undo stack and its grouping are ours: about 60 lines | None found. We keep snapshots of text and selection before each edit, which is cheap for one line of text: about 80 lines |

Both give the movement and selection rule 2 asks for. Both leave the undo stack to us; `cosmic-text` gives the reversible changes that fill it. Neither gives the key bindings; mapping our key events to these calls is ours under either choice, and about the same size.

### Fit with what we already have

- **`TextMeasure` and `Painter` (plan step 1.3):** both fit. `measure(text, style, wrap_width)` becomes a buffer or a layout's width and height. `Painter::text` becomes a glyph run for the atlas. `Shaping::Simple` and `Shaping::Full` map to `cosmic-text`'s `Basic` and `Advanced`. Parley's style properties name no shaping mode (none found in `src/style/mod.rs`), so `Simple` would mean nothing there.
- **Level of detail (panels spec: text close up, bars further out):** not affected by either. It decides whether text is drawn at all.
- **Shaping cost with many tile labels:** `cosmic-text` keeps the shaping in each `Buffer` until its text or attributes change, and has an optional `shape-run-cache` feature that shares shaped runs between buffers (`src/shape_run_cache.rs`, in 0.15 and 0.19). A Parley `Layout` must be rebuilt for any change to its text or style (crate docs, `src/lib.rs`), with no shared cache. We would keep one layout per label, keyed by text, size, and weight, beside the canvas's frame cache. Either way, labels are shaped once and redrawn from the cache; with Parley the cache is ours.

### Against the plan's stop point

The plan stops the work if "IME or text editing needs more than a few hundred lines on top of `cosmic-text`" (Stop points). On the `cosmic-text` path we owe about 150 lines for the preedit and about 60 for the undo stack: about 210 lines that only `cosmic-text` needs. The AccessKit text glue, 200 to 300 lines, is owed under either choice, so it is not a cost of `cosmic-text`. All of it together is 410 to 510 lines, before step 3.3 maps keys to the editor. The `cosmic-text` part is within "a few hundred". The total is near the line's edge, and step 3.3 should count its lines against it as it goes. On the Parley path the same count is about 80 lines (undo), plus the same AccessKit glue.

## The question for Brylie

**Which text crate should LoomDSP's UI layer use: `cosmic-text` or Parley?**

- **`cosmic-text` (recommended).**
  - *Gives:* today's font and widths exactly, so the parity gates hold. One font system with Iced's panels now. Glyph drawing from a crate already in our tree (`cryoglyph`) now and `glyphon` later. No new crates now. A slower release pace.
  - *Costs:* our own IME preedit handling, about 150 lines, and our undo stack inside a field, about 60, with the changes `cosmic-text` tracks. Our AccessKit text glue, 200 to 300 lines, owed under either choice. A version bump with small API changes after phase 4. A dependency on `cryoglyph`, maintained by Iced's authors, until then. Together about 410 to 510 lines of ours, near the plan's stop point of "a few hundred lines on top of `cosmic-text`", of which about 210 are owed to `cosmic-text` alone.
- **Parley.**
  - *Gives:* IME composition and the candidate area built in. Grapheme clusters that fit AccessKit's character lists. A design that its authors build for editors.
  - *Costs:*
    - our own glyph atlas, about 680 lines, needed in every case short of Vello at `wgpu` 30;
    - a second font system until phase 3 ends;
    - 35 new crates;
    - a different font on macOS by default, and different widths even with `system-ui`, so the parity gates fail until that is understood;
    - our AccessKit text glue all the same, 200 to 300 lines, and our undo stack inside a field, about 80 lines;
    - a faster release pace with large breaking releases.

Rule 8 puts IME first. Parley wins that criterion outright, but `cosmic-text` meets it with about 150 lines of our own, and 3, 4, 5, and 6 then favour `cosmic-text`. That is the reasoning behind the recommendation. If you weigh built-in composition above the rest, Parley is the choice, and the font question in criterion 5 must be solved before step 2.4b can pass its parity gate.

## Unverified facts cleared, and still open

Cleared (the plan's † marks for these go in this change):

- `glyphon`: its versions, licence, and `wgpu` and `cosmic-text` requirements, from crates.io. None fits `wgpu` 27.
- Parley's AccessKit text properties: they existed up to 0.11.1 and were removed in 0.12.0.
- Parley's editor tracking IME composition: confirmed in 0.12.0's `PlainEditor`.

Still open:

- What VoiceOver, Narrator, and Orca read from text fields built either way. That is a phase 3 check.
- Whether Parley's width differences come from `trak` tracking. A test that shapes with and without the point size would settle it.
- The probe on Windows and Linux.
- Composition with a real input method, for either crate.
- The line estimates. They come from reading, not from building.

## Sources

- crates.io API: versions, dates, licences, and dependency requirements of `cosmic-text`, `glyphon`, `parley`, and `vello`, read 2026-11-11.
- `parley` 0.12.0 release notes: <https://github.com/linebender/parley/releases/tag/v0.12.0>.
- `cosmic-text` 0.19.0 `CHANGELOG.md`, entries 0.15.0 to 0.19.0.
- Source, at the versions above, from the Cargo registry: `cosmic-text` (`src/font/system.rs`, `src/font/fallback/`, `src/buffer.rs`, `src/layout.rs`, `src/edit/mod.rs`, `src/shape_run_cache.rs`); `cryoglyph` (`Cargo.toml`, line counts); `iced_graphics` 0.14.0 (`src/text.rs`); `fontique` 0.12.0 (`src/backend/`); `parley` 0.12.0 (`src/editing/editor.rs`, `src/layout/cluster.rs`, `src/lib.rs`); `parley_engine` 0.12.0 (`src/shape/shaper.rs`); `harfrust` 0.12.0 (`src/hb/ot_shape.rs`); `accesskit` 0.25.2 (`src/lib.rs`).
- `examples/text-font-probe`, run on macOS 27.0.1 with `cargo run --release`.
