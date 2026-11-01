# Visual System Specification

The shared sizes, button roles, and quiet chrome that every view of the desktop app uses, so the app looks like one instrument and the controls a performer plays stand out.

- **Kind:** living specification.
- **Status:** Agreed, P1 (2026-11-01).
- **Decision behind it:** [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md) (one design language; themes are global).
- **Use case and requirements:** FR-PANEL-06 (global themes), NFR-UX-01 (keyboard and focus), NFR-UX-03 (colour is never the only signal).
- **Background:** the [P1 UX audit](../research/2026-11-p1-ux-audit.md), findings 4, 11, 13, and 17.
- **Issues:** [#454](https://github.com/brylie/LoomDSP/issues/454). Category colours and the palette: [#459](https://github.com/brylie/LoomDSP/issues/459). Tabs: [#456](https://github.com/brylie/LoomDSP/issues/456).
- **Code:** `crates/loom-app/src/style.rs` (tokens and button roles), `crates/loom-canvas/src/controls.rs` (knob start cap), `stats_bar` in `crates/loom-app/src/main.rs`.

## Goal

Each view of the app picked its own text sizes and button styles. The app set eight text sizes inline in seven files, and nearly every button was the same bright orange, the colour the canvas keeps for values a user can change. Nothing stood out, and the chrome competed with the knobs. A performer should see the patch and its values first, and find the one emergency control, Panic, at a glance.

## Behaviour

### Type scale

The app's own widgets (toolbar, inspector, matrix, browser, help, dialogs, status bar) use five text sizes, in logical pixels:

| Token | Size | Used for |
|---|---|---|
| `SMALL` | 11 | Table cells, row details, captions in the inspector and matrix |
| `BODY` | 12 | Running text, hover text, the status bar |
| `CONTROL` | 13 | Buttons, fields, pick lists, toggles, checkboxes |
| `HEADING` | 15 | Section headings, the patch name |
| `TITLE` | 18 | Help and dialog titles |

The markers of toggles, checkboxes, and radio buttons take one size from the same module (`SWITCH`, 14), so no size is written inline. Help's Markdown notes use the same scale: body 12, first-level headings 18, second- and third-level 15, code 11.

The canvas keeps its own text sizes in world units (`TITLE_SIZE`, `LABEL_SIZE` in `loom-canvas`), because tile text grows and shrinks with zoom.

### Button roles

| Role | Look | Used for |
|---|---|---|
| Primary | Orange fill, dark text | The one main action of a view or dialog, such as Create. A mode that is on, such as MIDI learn while learning. At most one per view. |
| Secondary | Quiet: a lifted grey fill, light text | Every other button. The default. |
| Danger | Red fill, light text | Panic. |
| Plain | No fill, text only | Small inline actions such as the × that removes a row. |

A disabled button keeps its role's shape and is drawn faded, so it reads as unavailable, not as another role.

### Toolbar

The toolbar shows the patch name, then its buttons in groups, with wider space between groups than inside them:

| Group | Buttons |
|---|---|
| File | New, Open, Save, Save As |
| Add | Add module |
| Play | Hold note, Panic |
| Edit | Undo, Redo |
| Setup | MIDI learn, Preferences |

No toolbar button is primary unless a mode is on (MIDI learn while learning). Panic is danger.

### Knobs

A knob's value arc always shows a short orange start cap, even at its minimum. The cap is the first 4% of the arc's sweep. The white pointer dot still marks the exact value, so the knob's position never depends on colour (NFR-UX-03).

### Status bar

| Shown | When |
|---|---|
| Module and cable count, audio device and rate, load, dropouts, input alerts, MIDI, the last status message | Always |
| Measure and Move camera switches, the frame summary | Only with `--dev`, `--measure`, `--measure-idle`, or `--bench` |

MIDI shows a count, such as "MIDI: 2 inputs" or "MIDI: no inputs". Its hover text lists the input names. The status message takes the remaining width and is cut, not wrapped; its hover text shows all of it.

## Rules

1. **Sizes come only from the token module.** No `.size(N)` or `text_size(N)` literal appears in `crates/loom-app/src` outside `style.rs`; a test checks the sources. *Why:* five named sizes are easy to keep consistent, and a future theme (FR-PANEL-06) changes them in one place.
2. **One primary button per view, at most.** *Why:* orange is the colour of values a user can change on the canvas. When every button is orange, nothing stands out and the chrome competes with the knobs.
3. **Panic is danger red.** *Why:* it is the one emergency control. On a dark stage it must be found at a glance, without reading.
4. **Secondary is the default.** A new button is secondary unless this spec gives it another role. *Why:* a quiet default keeps the chrome calm as the app grows.
5. **A knob at its minimum still shows orange.** *Why:* a knob with an empty arc looked switched off, though it is live and playable.
6. **The status bar shows performer information by default.** Developer tools appear only when the app starts with a developer or measuring flag. *Why:* the bar sits under a performer's eyes, and the frame meter is for checking the app, not for playing.
7. **The status bar never pushes an item off the window.** Long lists become counts with the detail in hover text. *Why:* with two controllers plugged in, the list of MIDI inputs pushed the feedback-loop status off the right edge.

## Acceptance criteria

- [ ] No `.size(N)` or `text_size(N)` literal remains in `crates/loom-app/src` outside `style.rs` (test over the sources).
- [ ] Every app text size is one of the five tokens (follows from the test above).
- [ ] The toolbar has no primary button while MIDI learn is off, and Panic is danger red (headless window test on the button pixels, as for #460).
- [ ] Undo and Redo look different enabled and disabled (existing #460 test still passes).
- [ ] A knob at position 0 draws an orange arc piece at the start of its sweep, and a knob at 1 draws the full arc (test on the control geometry).
- [ ] Without `--dev` or a measuring flag, the status bar has no developer tools; with `--dev` it has them (headless `Ui` test).
- [ ] With four MIDI inputs, the status bar shows "MIDI: 4 inputs" and every item fits at 1280 px wide (headless `Ui` test).
- [ ] AGENTS.md links this spec from its architecture list.

## Out of scope

- Category colours and the shared palette: [#459](https://github.com/brylie/LoomDSP/issues/459).
- Tabs and the matrix layout: [#456](https://github.com/brylie/LoomDSP/issues/456).
- The inspector, help, and browser layouts: [#457](https://github.com/brylie/LoomDSP/issues/457), which uses these tokens.
- Light theme and user themes: FR-PANEL-06, P2.
- Canvas text sizes, which scale with zoom.

## Open points

None.
