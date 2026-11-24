# Visual System Specification

The shared sizes, button roles, and quiet chrome that every view of the desktop app uses, so the app looks like one instrument and the controls a performer plays stand out.

- **Kind:** living specification.
- **Status:** The M1 visual system is implemented (2026-11-01). [Module shapes and playing state](#module-shapes-and-playing-state) records the agreed design direction (2026-11-19).
- **Decision behind it:** [ADR-0025](../adr/0025-one-design-language-and-derived-panels.md) (one design language; themes are global), [ADR-0029](../adr/0029-module-families-and-a-shared-palette.md) (module families and a shared palette).
- **Use case and requirements:** FR-PANEL-06 (global themes), NFR-UX-01 (keyboard and focus), NFR-UX-03 (colour is never the only signal).
- **Background:** the [M1 UX audit](../research/2026-11-p1-ux-audit.md), findings 4, 11, 13, and 17.
- **Issues:** [#454](https://github.com/brylie/LoomDSP/issues/454). Category colours and the palette: [#459](https://github.com/brylie/LoomDSP/issues/459). Tabs: [#456](https://github.com/brylie/LoomDSP/issues/456).
- **Code:** `crates/loom-app/src/style.rs` (tokens and button roles), `Family` in `crates/loom-core/src/descriptor.rs`, the family colours in `crates/loom-canvas/src/theme.rs`, `crates/loom-canvas/src/controls.rs` (knob start cap), `crates/loom-app/src/stats_bar.rs` (the stats bar).

## Goal

Each view of the app picked its own text sizes and button styles. The app set eight text sizes inline in seven files, and nearly every button was the same bright orange, the colour the canvas keeps for values a user can change. Nothing stood out, and the chrome competed with the knobs. A performer should see the patch and its values first, and find the one emergency control, Panic, at a glance.

## Behaviour

### Module shapes and playing state

**Agreed design direction, 2026-11-19.** Use the same visual vocabulary on module tiles, exposed sub-patch controls, and front panels. Choose a picture when it helps answer a musical question. A pulse has a high and a low part. Showing the share of the cycle spent high explains Width in a way that the number 0.25 alone cannot. A simple switch can already explain a two-state choice. [Panel layout](panels.md#panel-layout)

| What the player wants to know | What the panel communicates | Example |
|---|---|---|
| What have I set? | A contour or pattern shows the relationship between settings. It updates when the relevant setting changes and remains useful while the patch is silent. | Increasing Attack lengthens the rising segment. Rotating a rhythm shifts its hits and preserves their spacing. |
| What can I change here? | A control or handle identifies an editable part. Preserve its name, unit, keyboard action and exact value wherever it is exposed. | A Width handle on a pulse edge would mean the player can move that edge. A read-only preview uses the existing tile gesture. |
| What is happening now? | A live mark shows a reading from the running module. Give it a shape distinct from an edit handle and preserve the settings when the player follows it. | On an envelope, horizontal position can show stage and progress, while height shows the voice's actual level. On a rhythm, the current step can land on a rest. |

**Polyphony matters when the question is about voices.** One level can occur during both Attack and Release. An activity view intended to explain a chord should let a player distinguish overlapping voices and follow release tails. If a gate falls during Attack, the release begins at the level that voice reached. This is a useful review case because a marker following the ideal contour would give the wrong answer. Offer the same information on demand in words, such as “three active voices, one attacking, one sustaining, one releasing”. Colour, continuous motion and frame-by-frame speech should each be optional for understanding the state.

**Review a panel while it is silent and while it plays.** Ask a player to predict the effect of an edit, identify the part they can operate, and describe the result during playback. For an envelope, try two overlapping notes and release one early. Repeat using keyboard navigation and the accessible description. [Illustrated examples and interactive envelope study](../research/2026-11-19-release-polish.md#what-is-the-patch-doing-now)

**Implementation scope.** Envelope, LFO and Ladder already have calculated, read-only previews. Direct parameter handles remain an open widget design. The current polyphony rules give displays voice 1. A future per-voice activity view needs an agreed change to those rules, including how it reports stage and level, handles coincident voices, and reads bounded telemetry without waking an idle or hidden panel. The studies illustrate those choices for review. [Shape previews](panel-widgets.md#read-only-shape-preview) · [Polyphony, rule 9](polyphony.md) · [Telemetry](telemetry.md)

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
| Secondary | Quiet: a lifted grey fill, dark text | Every other button. The default. |
| Danger | Red fill, dark text | Panic. |
| Plain | No fill, text only | Small inline actions such as the × that removes a row, and the breadcrumb's entries. |

A disabled button keeps its role's shape and is drawn dimmed, so it reads as unavailable, not as another role.

#### Role tokens

**Status:** Implemented, M2 (Brylie's decisions of 2026-11-18, [plan for leaving Iced](../dev/leaving-iced-plan.md), step 3.2, row 2). The tokens are `style::PRIMARY`, `SECONDARY`, `DANGER`, and `PLAIN`, read through `style::Role`.

Each role has a look for each state a button can be in: normal, hovered (the pointer is over it), pressed (a press on it is held, by the pointer or by Space), and off (turned off). The tokens are ours, in `style.rs`, as `loom-ui-core` colours, and they are **the one source for both toolkits until Iced goes** (phase 4): `loom-ui`'s buttons read them, and so do the role style functions that Iced's buttons are drawn with (`style::primary`, `secondary`, `danger`, `plain`), which map each of Iced's button states to the token for that state. Iced keeps its own state handling for its buttons; only the colours come from here. That mapping is a bridge, deleted in phase 4 with Iced: the states are named for what our buttons need, not for Iced's. The values start from the dark theme's palette, whose panel background is `#22242A`.

| Role | State | Fill | Text | Text on fill | Fill on background | Fill against normal |
|---|---|---|---|---|---|---|
| Primary | normal | `#F5A623` | `#2A1A00` | 8.31 | 7.65 | |
| Primary | hovered | `#F9C876` | `#2A1A00` | 10.87 | 10.01 | 1.31 |
| Primary | pressed | `#DC8D0A` | `#2A1A00` | 6.31 | 5.80 | 1.32 |
| Primary | off | `#5C4A2E` | `#CDBD9E` | 4.60 | 1.83 † | 4.19 |
| Secondary | normal | `#7F828D` | `#0E0E10` | 5.03 | 4.05 | |
| Secondary | hovered | `#9497A0` | `#0E0E10` | 6.61 | 5.31 | 1.31 |
| Secondary | pressed | `#6C6F7A` | `#F2F3F6` | 4.51 ‖ | 3.10 | 1.31 |
| Secondary | off | `#3E414B` | `#AEB1BB` | 4.75 | 1.52 † | 2.66 |
| Danger | normal | `#E04F4F` | `#0E0E10` | 4.96 | 3.99 | |
| Danger | hovered | `#E77474` | `#0E0E10` | 6.56 | 5.28 | 1.32 |
| Danger | pressed | `#D42525` | `#F2F3F6` | 4.63 | 3.02 ‖ | 1.32 |
| Danger | off | `#5A3434` | `#C9ABAB` | 5.01 | 1.46 † | 2.73 |
| Plain | normal | none | `#E6E8EE` | 12.66 on the background | ‡ | |
| Plain | hovered | `#40444F` | `#E6E8EE` | 7.94 | 1.59 ‡ | 1.59 |
| Plain | pressed | `#50556A` | `#E6E8EE` | 6.02 | 2.10 ‡ | 2.10 |
| Plain | off | none | `#8E929C` | 4.98 on the background | ‡ | |

Ratios are by the WCAG 2.2 formula (`loom_canvas::contrast_ratio`), against the panel background. The filled roles have a 2 px radius and no border; plain has neither.

- **One table, for the one theme.** The app has one theme, a dark one (`boot.rs`, `Theme::custom`, panel background `#22242A`), so the tokens are set for that background and every ratio is measured on it. A light theme (FR-PANEL-06) needs a table of its own, checked the same way.
- **Text is at least 4.5:1 in every state** (WCAG 2.2, SC 1.4.3). **A turned-on fill of primary, secondary, or danger is at least 3:1 against the background** (SC 1.4.11), so the button's edge shows. *Why:* the toolbar is read at a glance on a dark stage.
- **Each state stands apart from normal: our own rule.** For primary, secondary, and danger, the hovered and the pressed fills are each at least 1.3:1 against the normal fill, and plain's hovered fill at least 1.5:1 against the background it sits on, with pressed a further step. WCAG asks nothing of this. *Why:* a step most people cannot see is a lost state: a press would look like nothing happened.
- **Pressed may change the text colour.** A filled role's pressed fill is dark enough that its text turns light (secondary and danger), so the text keeps 4.5:1.
- † **Off.** An inactive component is exempt from SC 1.4.3 and 1.4.11. Its text still meets 4.5:1, so a player can read what the button is before its hover text says why it is off, and its fill is dimmer than every turned-on state, so it reads as off without colour alone.
- ‡ **Plain** is identified by its text, which meets 4.5:1; its hovered and pressed fills are feedback, not what identifies it, so SC 1.4.11 asks nothing of them. Plain buttons sit on the stats bar and the breadcrumb, which paint no background of their own, so the panel background is what they sit on. Keyboard focus is shown by the focus ring, at 3:1 ([UI layer spec](ui-layer.md), section 6).
- ‖ **Closest to a line:** secondary's pressed text, at 4.51:1, and danger's pressed fill, at 3.02:1. The token test pins every value in this table, so a change to any of them shows.
- **Hovered is lighter and pressed darker** than normal for every filled role. *Why:* before, hovered primary and danger were darker and pressed was the normal look, so the two could not be told apart.
- **The look a button shows:** off when it is turned off, whatever else holds; otherwise pressed while a press on it is held and the pointer is over it, or while Space is held on it; otherwise hovered while the pointer is over it; otherwise normal. Keyboard focus adds the focus ring to whichever look shows.
- **What else uses the role functions.** The menus' highlighted item (`style::outlined`) builds on a role, so it takes these colours and keeps its outline. The right panel's tabs (`style::tab`) and the module browser's rows have styles of their own and keep their look. No text field, pick list, slider, checkbox, or radio uses a button style.
- Before: the role functions passed Iced's own role styles through, and an off button was its normal look at half opacity, about 1.5:1.

Tile and probe menus share a solid panel and the same row style. Menu text, including a disabled row, has at least 4.5:1 contrast with its fill. The edge of a row and the outline on hover or keyboard focus have at least 3:1 contrast. The panel edge also stands out against a tile and the canvas.

Active checkboxes use the control edge for the unchecked outline and the
canvas value colour with a dark check mark for the checked state. The browser
uses family tints inside its rows, where both the plain and selected fills are
dark. Tile edges, inactive knob tracks, and unlit light markers keep at least
3:1 against their adjacent canvas or control fills.

### Toolbar

The toolbar shows the patch name, then its buttons in groups, with wider space between groups than inside them:

| Group | Buttons |
|---|---|
| File | New, Open, Save, Save As, Save to library (inside a sub-patch), Browse, Recent |
| Add | Add module |
| Play | Hold note, Panic |
| Edit | Undo, Redo |
| Setup | MIDI learn, Preferences |

No toolbar button is primary unless a mode is on (MIDI learn while learning). Panic is danger.

Inside a sub-patch, the breadcrumb sits under the toolbar: "Patch", then one entry per level, with "›" between them, each a plain button that opens its level ([sub-patches](sub-patches.md)). From row 2 of the plan for leaving Iced, the toolbar and the breadcrumb are `loom-ui`'s, with heights from `style::TOOLBAR_HEIGHT` and `style::BREADCRUMB_HEIGHT`.

### Knobs

A knob's value arc always shows a short orange start cap, even at its minimum. The cap is the first 4% of the arc's sweep. The white pointer dot still marks the exact value, so the knob's position never depends on colour (NFR-UX-03).

### Cable widths

A cable's stroke uses its compiled output width (FR-CANVAS-12). At 100%
zoom it is 2.4 units for a mono voice, 3.6 for a stereo voice, 4.8 for a
mono bundle, and 6 for a stereo bundle. At distant zoom each has a minimum
stroke in the same proportion. The inspector names the exact voice count
and stereo sides. The dashed stroke continues to mark an inserted
one-sample delay, and the signal colours continue to show the port types.

### Module families

Every module belongs to one of eight families ([ADR-0029](../adr/0029-module-families-and-a-shared-palette.md)). Its tile header takes the family's colour, and the module browser lists modules by family.

| Family | Colour | Echoes | Built-in modules |
|---|---|---|---|
| Sources | `#62222C` red | audio | Sine, Saw, Pulse, Triangle, Noise |
| Filters and effects | `#324553` slate | | SVF, Ladder, One-Pole, Delay Line, All-Pass |
| Modulation | `#38589F` blue | modulation | Envelope, LFO, Sample and Hold |
| Time and rhythm | `#463172` violet | phase | Transport, Clock, Clock Divide, Phase, Phase Scale, Phase Trigger, Phase Wrap and Fold |
| Notes and pitch | `#624222` brown | pitch | Pitch Constant |
| Logic and math | `#716A28` olive | gate | Add, Subtract, Multiply, Divide, Constant, Convert, Compare, Logic, Memory |
| Level and routing | `#2A7947` green | | Amplifier, Clamp, Level Constant, Scale, Slew, Mixer, Crossfade, Join, Split, 1-Sample Delay |
| Interface | `#606676` grey | | Audio In, Audio Out, MIDI Note, MIDI Controller, Level Meter, Scope, Button |

- **The palette** holds these 8 colours and a lighter tint of each, 16 swatches in all. A tint is its colour mixed halfway to white and takes dark text. FR-CANVAS-07 will offer the same 16 when users colour modules and groups.
- **Reserved colours** are not in the palette: orange for editable values, pink for modulation, light green for lights, the focus blue, and the warning red.
- **The family name** shows in the header's hover text, in each module browser row beside a swatch of its colour, and in help. Headers carry no other mark.
- **The category** stays as a finer label. Search matches it, and help shows it.

### Status bar

| Shown | When |
|---|---|
| Module and cable count, audio device and rate, load, dropouts, input alerts, output warnings, MIDI, the last status message | Always |
| `Learning` | While MIDI learn mode is on ([MIDI learn](midi-learn.md)) |
| The kept error or warning | While there is one, and it is not the last status message |
| Measure and Move camera switches, the frame summary | Only with `--dev`, `--measure`, `--measure-idle`, or `--bench` |

MIDI shows a count, such as "MIDI: 2 inputs" or "MIDI: no inputs". Its hover text lists the input names. The fixed items come first. The kept problem and the status message share the remaining width. Each is cut, not wrapped, and its hover text shows all of it.

### Status messages

**Status:** Agreed, M2 ([#493](https://github.com/brylie/LoomDSP/issues/493), 2026-11-03). Severity follows NFR-UX-03: a word as well as a colour.

- **Three severities.** Info has no label. A warning shows `Warning:` and an error shows `Error:`, each in its own colour. The snapshot reports them as `info`, `warning`, and `error`.
- **What each one means.** A kept error is a problem that stays true after the moment passes and that the player must act on, or that lost their work. A refused gesture or a problem that fixed itself is a warning. Info and most warnings show as the latest message only. *Why:* if every failed gesture stayed until dismissed, a performer would be clicking × on stage.
- **Kept warnings.** A staged graph commit keeps a waiting message until the engine takes the edit. A lost autosave also stays on screen. It stays true and leaves the player's work at risk, so it stays on screen. But nothing is wrong until a crash, so it reads as a warning, never as a red error, and nothing interrupts playing. *Why:* autosave is a safety net, and on stage other things can matter more than a full disk.
- **An error is also the latest message.** So `status` always holds what happened last, and a failed control command replies with its own text. The kept error field appears only when the newest kept error is not the latest message, so the same text never shows twice.
- **One kept problem per cause.** The bar shows the most severe, then the newest. Its hover text lists the others. *Why:* with one slot, a failed save could hide a lost device, and a later warning must not hide an error.

| Cause | Cleared by |
|---|---|
| Audio output | Sound is playing again, or the player chooses a device. No × button ([device changes](device-changes.md)). |
| Audio input | The stream opens, the player chooses an input, or an input is no longer wanted. × allowed. |
| File | A later successful save, open, or new patch. × allowed. |
| Autosave (a warning) | Autosave works again. × allowed, and it does not come back until autosave recovers and fails again. Autosave that is off for the run has only the ×. |
| Graph commit | A commit succeeds or another patch replaces this one. × allowed. A failed commit also reports separately when its undo history was lost. |
| Other | × or a newer error of this kind. |

Kept problems survive opening another patch, except as the table says.

- **Disabled toolbar buttons say why.** Hold note and Recent have hover text with the reason while disabled, as Undo and Redo do.

### Overlays

**Status:** Agreed (Brylie, 2026-11-20), for plan 3.2, row 3. Each look is a token in `style.rs`, so `loom-ui`'s overlays and anything Iced still draws read the same values.

| Token | What it is | Value |
|---|---|---|
| `SCRIM` | The full-window wash behind a dialog (the question, the licence chooser) | Black at 40% opacity, as today |
| `OVERLAY_PANEL` | A dialog's box: fill, border, and radius | Matched to the box Iced draws today, measured when the token is built, so the gate shows no change |
| `MENU_PANEL` | A menu's box over the canvas: the base fill, a 1 px quiet border, radius 6 | As today's probe menu |
| `MENU_SHADOW` | The shadow under a menu | Black at 35% opacity, offset 4 px down, blur 12 px, as today |
| `MENU_ITEM` | A menu item's looks: a 1 px quiet border and radius 3 always; a weak fill while hovered, pressed, or highlighted, however the highlight got there; the focus ring on top when the keys moved it | As today's menu items, without Iced's 2 px primary border on the highlight (Brylie's decision, 2026-11-20): the focus ring marks keyboard focus instead |

*Why:* the dialogs and menus were drawn with Iced's own box and its palette, which are not ours. Tokens keep the look the same while it moves, and give later menus and popovers one source.

## Rules

1. **Sizes and colours come only from the token module.** No `.size(N)`, `text_size(N)`, `.radius(N)`, or `.border(N, ...)` literal appears in `crates/loom-app/src`, in any folder, outside `style.rs` and test files, and no `loom-ui` element's `.background(...)` or `.colour(...)` builds a colour inline; a test checks the sources. *Why:* five named sizes are easy to keep consistent, and a future theme (FR-PANEL-06) changes them in one place.
2. **One primary button per view, at most.** *Why:* orange is the colour of values a user can change on the canvas. When every button is orange, nothing stands out and the chrome competes with the knobs.
3. **Panic is danger red.** *Why:* it is the one emergency control. On a dark stage it must be found at a glance, without reading.
4. **Secondary is the default.** A new button is secondary unless this spec gives it another role. *Why:* a quiet default keeps the chrome calm as the app grows.
5. **A knob at its minimum still shows orange.** *Why:* a knob with an empty arc looked switched off, though it is live and playable.
6. **The status bar shows performer information by default.** Developer tools appear only when the app starts with a developer or measuring flag. *Why:* the bar sits under a performer's eyes, and the frame meter is for checking the app, not for playing.
7. **The status bar never pushes an item off the window.** Long lists become counts with the detail in hover text. *Why:* with two controllers plugged in, the list of MIDI inputs pushed the feedback-loop status off the right edge.

## Acceptance criteria

Module families:

- [x] Every module declares a family in its descriptor; one without a family does not compile.
- [x] Every family colour has at least 4.5:1 contrast with the header text, and every tint at least 4.5:1 with the dark text (test in `theme.rs`).
- [x] Every pair of family colours is at least ΔE 14 (CIE76) apart in normal vision and in protanopia, deuteranopia, and tritanopia simulation (Machado 2009, severity 1), and every swatch is at least ΔE 15 from every reserved colour (test in `theme.rs`).
- [x] A tile's header uses its family's colour (test).
- [x] With an empty search, the browser lists modules by family in the order above, then by name; each row shows its family; searching a family or category name finds its modules (headless test).
- [x] Hovering a tile's header shows the module's family and description (test), and help shows the family.

The rest:

- [x] No `.size(N)` or `text_size(N)` literal remains in `crates/loom-app/src` outside `style.rs` (test over the sources).
- [x] Every app text size is one of the five tokens (follows from the test above).
- [x] The toolbar has no primary button while MIDI learn is off, and Panic is danger red (headless window test on the button pixels, as for #460).
- [x] Undo and Redo look different enabled and disabled (existing #460 test still passes).
- [x] A knob at position 0 draws an orange arc piece at the start of its sweep, and a knob at 1 draws the full arc (test on the control geometry).
- [x] Without `--dev` or a measuring flag, the status bar has no developer tools; with `--dev` it has them (headless `Ui` test).
- [x] With four MIDI inputs, the status bar shows "MIDI: 4 inputs" and every item fits at 1280 px wide (headless `Ui` test).
- [x] AGENTS.md links this spec from its architecture list.
- [x] An error stays visible after a later info message, and the snapshot reports the status severity (`App::ui_snapshot()` tests).
- [x] Each row of the kept error table has a test. The same text never shows twice.
- [x] With four MIDI inputs, input drift, `Learning`, a long kept error, a long status message, and both output warnings, every item fits at 1280 px wide (headless `Ui` test).
- [x] Disabled Hold note and Recent have hover text that says why (test on what the view passes to its tooltips, because the `Ui` harness does not reach them).

Role tokens (plan for leaving Iced, row 2):

- [x] Every ratio in the role token table, the state-to-state ratios included, is checked from the tokens in `style.rs` (`the_role_tokens_meet_their_table`).
- [x] `loom-ui`'s buttons and Iced's role style functions read the same tokens, for every state (`both_toolkits_read_the_same_role_tokens`).
- [x] A hovered, a pressed, and an off button each paint their token's fill and text (`loom-ui`: `an_element_paints_the_look_of_its_state`, `off_wins_over_hover_and_press`; a capture: `a_capture_draws_enabled_buttons_as_enabled`).
- [x] The toolbar never has two primary buttons (`the_toolbar_has_one_primary_at_most`).
- [ ] Brylie has seen `--screenshot-window` of the toolbar at the top level and inside a sub-patch, of Preferences, and of one overlay with buttons, before row 2 merges.

## Out of scope

- Tabs and the matrix layout: [#456](https://github.com/brylie/LoomDSP/issues/456).
- The inspector, help, and browser layouts: [#457](https://github.com/brylie/LoomDSP/issues/457), which uses these tokens.
- Light theme and user themes: FR-PANEL-06, M2.
- Canvas text sizes, which scale with zoom.

## Open points

None.
