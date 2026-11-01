# UX Review: Using LoomDSP Without a Mouse, or With Low Vision

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-01, on `main` at e59132e (after #515).
- **Follows:** the persona reviews of the same day, from [P1 UX audit, round 2](2026-11-p1-ux-audit-2.md) to the [hybrid modular review](2026-11-hybrid-modular-ux-review.md).
- **Used by:** issues #524 and #525, and comments on #494 and #330. Decisions go in specs, ADRs, or the product docs.

This review reads the desktop app as people who cannot rely on a mouse or on small text: someone who works only with the keyboard (because of a motor disability, repetitive strain, or a switch device that sends keys), and someone with low vision who needs larger text and clear contrast. Screen reader support is decided already: [ADR-0028](../adr/0028-app-accessibility-waits-for-iced.md) defers it until Iced supports AccessKit, and keeps the names and values a screen reader would need in the plain-data view models. This review does not argue that again. It asks what these users can do now, and what could be done before AccessKit arrives. Do not update this snapshot later; write a new one after the fixes.

## Method

- Tried to build a patch with only keys, step by step: add a module, connect it, set a value, add modulation, add a probe, reset a control, save. For each step, found the key path in the code or spec, or its absence.
- Read the keyboard handling in `crates/loom-canvas/src/widget/input.rs` and `panel_input.rs`, the app's shortcuts (`main.rs`, `panic.rs`, `subscriptions.rs`), the matrix's focus (`matrix.rs`), the shortcut list users see (`HELP` in `main.rs`), and the view code for the toolbar and right column (`view.rs`).
- Read ADR-0028, NFR-UX-01 to 06, the keyboard sections of `panels.md` and `modulation.md`, and the earlier reviews' accessibility findings.
- Searched the open `accessibility` issues so as not to repeat them.

**Already filed, not repeated here:** contrast of every colour pair (#463), colour-only cues and pointer target sizes (#495), status severity and a lasting learn-mode indicator (#493), the key conflicts between specs and Space reaching Run (#494), readable pitch values (#513), and the keyboard model for widgets with several parts (#289).

**Limits.** No assistive technology or switch device was used, and no disabled user was watched. The keyboard walk was done in the code and specs, not at the window.

Priority means: **High** blocks the task; **Medium** slows the user or looks broken; **Low** is polish.

## Summary

Once a patch exists, a keyboard user can do a lot. Tab selects modules in reading order; Enter goes into a module's panel; the arrow keys change values in fine and coarse steps, move tiles, and resize them; a focused control shows its hover text and a visible focus ring; the module browser is fully keyboard driven; and the modulation matrix was built with a keyboard path on purpose. That is more than most modular tools offer.

But a keyboard user cannot build a patch. There is no way to make a cable without dragging. A, the key that adds a module, works only while the mouse pointer happens to be over the canvas. The toolbar, the tabs, and the inspector cannot take keyboard focus, so the matrix's keyboard path can only be opened with a click. A control can be reset only by a timed double-click. And a low-vision user cannot make the app's own text any larger: the canvas zooms, but the toolbar, inspector, help, and browser stay at 11 to 13 pixels.

| # | Area | Finding | Priority |
|---|---|---|---|
| 1 | Keyboard | A patch cannot be built by keyboard: no cable without a drag, and A needs the pointer over the canvas | High |
| 2 | Vision | The app's text cannot be made larger; only the canvas zooms | High |
| 3 | Keyboard | The toolbar, tabs, and inspector take no keyboard focus; the matrix opens only with a click | Medium |
| 4 | Motor | A control resets only by a double-click within 400 ms and 4 px | Medium |
| 5 | Motor | Probes and the tile menu open only by right-click | Low |
| 6 | Vision | Scopes and meters always move; no reduced-motion option | Low |
| 7 | Screen readers | Deferred (ADR-0028); the view models already carry most of what a tree needs | Low |

## Findings

### 1. A patch cannot be built by keyboard (High)

The steps of a first patch, with only keys:

| Step | Key path today |
|---|---|
| Add a module | A opens the browser at the pointer, but only when the pointer is over the canvas: the key handler takes the pointer's position, and with the pointer elsewhere it does nothing and swallows the key (`input.rs`). The toolbar's Add module button takes no focus (finding 3). |
| Connect two modules | None. Cables are made by dragging from a port; no spec gives a key path, and the inspector has no connect control. |
| Select a module | Tab and Shift-Tab, in reading order. |
| Change a value | Enter, Tab to the control, arrow keys (Shift for fine steps). |
| Add modulation | The matrix's Add form, which is keyboard driven, but opening the Matrix tab needs a click (finding 3). |
| Add a scope or meter | None. Right-click only (finding 5). |
| Undo, save, panic, help | Cmd/Ctrl+Z, Cmd/Ctrl+S, Cmd/Ctrl+., F1. |

So a keyboard user can play and tune a patch someone else built, but cannot make one. WCAG 2.5.7 (dragging movements) asks for a way to do every drag without dragging; here the drag that matters most, the cable, has none.

Direction (spec first):
- **A key path for cables.** For example: with a module selected, a key opens a small list of its outputs; picking one opens a list of every input in the patch (searchable, as the browser is), and picking one connects them. The same list form removes or moves a cable on a focused input. This also helps mouse users in a large patch.
- **A opens the browser without the pointer.** When the pointer is not over the canvas, open it at the selected module (placing the new one beside it), or at the centre of the view.
- Record both in the keymap issue's single table (#494).

### 2. The app's text cannot be made larger (High)

The canvas zooms from 0.1 to 4 times, so tiles can be made large. Everything else uses fixed sizes from `style.rs`: 11 pixels for small text, 12 for body text, 13 for controls, up to 18 for titles. There is no setting for a larger interface, and the app does not follow an operating system's text size preference (only its display scale, which enlarges everything at once and is often already in use). Port labels on the canvas are 9 units at 100% zoom.

Low-vision users usually need text at 150 to 200%. Today they can zoom the canvas, but the inspector, help, module browser, matrix, toolbar, and status bar stay small. WCAG 1.4.4 (resize text) asks for 200% without loss of content.

Direction: an interface scale preference, stored per machine (preferences.md), applied to every token in `style.rs` and to the window's layout, with the default from the operating system where it reports one. The type scale is already in one module, which makes this much easier than in most apps. Check that the right column and toolbar still fit at 200% (they are fixed widths today).

### 3. Focus cannot reach the toolbar, tabs, or inspector (Medium)

Iced 0.14 buttons cannot take keyboard focus, so Tab moves only among modules and their controls. Shortcuts reach some toolbar actions (New, Open, Save, Save As, Undo, Redo, Panic, Preferences, Help), but not Add module (finding 1), Hold note, MIDI learn, or the Inspector and Matrix tabs. The matrix gains focus only when its tab is clicked (`matrix.rs`, `view.rs`), so its keyboard path, built for people who do not use a mouse ([modulation.md](../design/modulation.md)), cannot be reached without one. The inspector's sliders, range picker, and modulation rows are mouse only too.

Direction: a shortcut for every toolbar action and for each tab, listed in the keymap (#494), and a key that moves focus between regions (canvas, right column, toolbar), as many desktop apps do with F6. Inside the right column, give the inspector the same focus model the canvas has. This is also part of what an accessibility tree will need later (ADR-0028), so it is not wasted work.

### 4. Reset needs a timed double-click (Medium)

A control returns to its default on a double-click within 400 ms and 4 pixels (`DOUBLE_CLICK`, `DOUBLE_CLICK_SLOP`, `crates/loom-canvas/src/widget.rs`). There is no key for it on a focused control. For someone with a tremor, or using a switch or a head pointer, a timed double-click in a 4-pixel area is hard or impossible, and a keyboard user has no way at all. Typed values (#504) would let them type the default, but only if they know it.

Direction: a key that resets the focused control (Backspace or Delete is the usual choice: a focused control takes keys before the canvas, so it would not delete the module). Settle it in the keymap (#494). Consider following the operating system's double-click time instead of a fixed 400 ms.

### 5. Right-click only menus (Low)

Add scope and Add meter (the probe menu) and the tile menu with Help open only on a right-click. Help also opens with F1, which covers the tile menu's one item. The probe menu has no key path. Panels.md's planned canvas context menu (#330, M2) names Shift+F10 and the Menu key; that would answer this.

Direction: no new issue. Make sure #330 includes the probe items and the keyboard opening it already plans.

### 6. Motion (Low)

Scopes redraw at the frame rate and meters move with the signal. That motion is content, not decoration, and it is the point of a scope. But a user who is distracted or made unwell by motion has no way to calm it: there is no reduced-motion option, and the app does not read the operating system's setting. Scope hold (#513) will give a way to freeze one; a global option would pause or slow every live display.

Direction: note it in the visual system spec; no issue now.

### 7. Screen readers (Low here, because decided)

ADR-0028 defers the accessibility tree until Iced supports AccessKit, for good reasons: a fork is too costly, and a hand-kept bridge can say things that are not true. Its consequence, that each new view model holds what a screen reader would read, is visible in the code: the inspector rows and the UI snapshot carry labels, values with units, and states. The control channel already proves those names are complete enough to drive the app.

Direction: none new. When this review's keyboard work is done (findings 1, 3, 4), the canvas and side column will already have the focus order and names the tree needs. The revisit trigger in ADR-0028 (each Iced upgrade) stands.

## What works well

- **Keyboard on the canvas.** Tab and Shift-Tab select modules in reading order, Enter enters a panel, and arrow keys change values, move tiles, and resize them, each with a fine or coarse step (NFR-UX-01, #101).
- **A focus ring only for keyboard focus,** around the control and not its caption, so the value stays readable.
- **Hover text for keyboard focus,** after the same delay as for the pointer (panels.md).
- **The module browser** is fully keyboard driven: type to search, arrows to choose, Enter to add, Escape to close.
- **The modulation matrix** was designed as the keyboard path for modulation, with row and cell navigation.
- **Global shortcuts** for the file, undo, panic, preferences, and help actions, with Panic working even in a text field.
- **One type scale in one module** (`style.rs`), enforced by a test, which makes an interface scale setting cheap.
- **Shape as well as colour** for ports, family headers, and the clip light (NFR-UX-03), with more planned in #495.
- **A clear decision on screen readers** (ADR-0028) instead of a silent gap, with the groundwork kept in the view models.

## Issues

Grouped to keep the backlog small. Priority and size are on the LoomDSP project board.

| Issue | Findings | Milestone | Priority | Size |
|---|---|---|---|---|
| [#524](https://github.com/brylie/LoomDSP/issues/524) Build a patch by keyboard: cables by key, A without the pointer, focus for the toolbar, tabs, and inspector | 1, 3 | M2 | P1 | L |
| [#525](https://github.com/brylie/LoomDSP/issues/525) Interface scale: make all app text larger, up to 200% | 2 | M2 | P1 | M |
| [#494](https://github.com/brylie/LoomDSP/issues/494) (comment): a reset key for the focused control, and shortcuts for every toolbar action and tab | 3, 4 | | | |
| [#330](https://github.com/brylie/LoomDSP/issues/330) (comment): include the probe items and the keyboard opening | 5 | | | |
| Not filed here: reduced motion (visual system note), screen readers (ADR-0028) | 6, 7 | | | |

## Suggested order

1. Interface scale (2, #525). Mostly mechanical: the type scale is already one module.
2. A without the pointer (1, #524), and a reset key (4, #494). Small, and they unblock the two most common steps.
3. Cables by key, and focus for the toolbar, tabs, and inspector (1, 3, #524), with the keymap (#494).
4. The canvas context menu with a keyboard opening (5, #330).
