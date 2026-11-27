# Module Browser Specification

A searchable list of module types that adds the chosen module to the canvas, where the user is looking. It is for everyone who builds a patch, and fastest for users who know what they want.

- **Kind:** living specification.
- **Status:** Implemented, with the built-in abstractions and slot rows of [#700](https://github.com/brylie/LoomDSP/issues/700); see [Groups](#groups).
- **Decision behind it:** [ADR-0010](../adr/0010-own-patch-canvas.md) (fast insertion on our own canvas).
- **Use case and requirements:** FR-CANVAS-03, FR-CANVAS-06, NFR-UX-01, NFR-UX-05.
- **Prior art:** many node editors open a search box at the pointer when you press a key on the canvas, and add the chosen node there. We use the idea, not any product's layout or wording.
- **Issues:** [#11](https://github.com/brylie/LoomDSP/issues/11)
- **Code:** `crates/loom-app/src/browser.rs` (search, model, and panel), `crates/loom-app/src/browser/access.rs` (what a screen reader finds), `crates/loom-canvas/src/widget/event.rs` (`CanvasEvent::OpenBrowser`).

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

"I know I want a filter. Let me type `filt`, press Enter, and have it appear where my pointer is, so I can cable it up straight away."

Before the browser, the toolbar had a drop-down of module IDs and an Add button. It listed every ID in one long list, showed nothing about what a module does, and put the new module wherever auto-layout found space, often off screen. A user had to know the ID, find it, and then find the new tile.

## Behaviour

### Opening the browser

| Gesture | Where the browser opens | Where the module goes |
|---|---|---|
| `A` with the pointer over the canvas | At the pointer | At the pointer, snapped to the grid |
| `A` while the canvas has the keys and the pointer is elsewhere | At the centre of the canvas | Nearest free grid position to the centre of the visible canvas |
| Double-click on empty canvas | At the pointer | At the click, snapped to the grid |
| Right-click on empty canvas, with no move between press and release | At the pointer | At the click, snapped to the grid |
| "Add module" button in the toolbar | At the centre of the canvas | Nearest free grid position to the centre of the visible canvas |

The panel's corner goes at that point, moved in as needed so the whole panel stays inside the canvas. *Why:* the toolbar button opens it at the canvas centre, not under the button, because only the canvas knows its camera, and so where the centre is in the patch.

The browser is a small floating panel: a search field on top, a list of matches below, and the highlighted module's description and ID at the bottom. The search field has keyboard focus as soon as the browser opens, so the user can type at once.

### Searching

- Each module shows its **name** and a small swatch of its **family** colour with the family name ([ADR-0029](../adr/0029-module-families-and-a-shared-palette.md)). The highlighted module's **ID** shows under its description at the bottom, in a quieter style, so no row is cut and the scrollbar covers no text ([#457](https://github.com/brylie/LoomDSP/issues/457)).
- The highlighted row is a lifted grey with an orange outline, so its quieter text stays readable (at least 4.5:1, tested).
- An empty search lists every module by family, in the family order of the [visual system](visual-system.md#module-families), then by name.
- Typing filters the list. A module matches when every word of the query is found in its name, ID, category, family, or description, ignoring case. Example: `osc saw` matches `loom.osc.saw`; `filter` matches every module in the Filter category.
- Results are ranked: a name that starts with the query first, then a word in the name that starts with it, then other matches in the name, ID, category or family. Matches that need the description come last. Ties keep the empty-search order.
- With no matches the list says "No modules match. Clear the search to see every module."
- The first result is highlighted after each change to the query.
- Below the list, the highlighted module's one-sentence `description` is shown, so the user can tell what it does before adding it.

### Groups

Agreed on the plan for [#700](https://github.com/brylie/LoomDSP/issues/700), 2026-11-14, and Implemented.

The list holds four kinds of row, in this order in an empty search:

| Group | Row text | Mark | What the area under the list shows | Menu |
|---|---|---|---|---|
| **Built-in abstractions** | The abstraction's name, such as `Synth voice` | `Built in` | Its one-sentence description; detail `Built in, inserted as a copy` | None |
| **Modules** | The module's name | Its family | Its description; detail its ID | None |
| **Slots** | `Filter slot`: one per role that a registered module or a library item fills | `Slot` | The role's description and `Add an empty slot, then swap a part in from its menu or the inspector.`, and for a Filter or Effect role `While empty, it passes its input through.`; detail the role's kind | None |
| **Library items** | The patch's name | `Sub-patch` | `A sub-patch from your library, inserted as a copy. Right-click it to insert it linked.`; detail its path | Insert linked |

- *Why built-ins first:* a new player should find the Synth voice without scrolling past every module.
- A query keeps the group order and ranks by name within each group. A built-in matches its name, `sub-patch`, and `built in`; a slot row matches its role's name, `slot`, and the role's kind.
- Slot rows are worked out again when the library folder has been read, so a Voice row appears once a library item fills Voice. A library item's role kinds are resolved through the registry, and an unknown or newer one adds no row. The highlight stays on the same row (by its UI ID) when the rows change.
- Row IDs come from what a row shows, inside the list and a group of its kind: `browser/list/builtin/<file stem>`, `browser/list/module/<type id>`, `browser/list/role/<role id>`, `browser/list/library/<path>`. The list is a scroll container of the runtime's, which keeps its offset by its UI ID, so the rows sit under it (changed in plan 3.2, row 5, 2026-11-24, from `browser/builtin/...`). A screen reader reads a row's name and mark, and leaves the mark out when the name already ends with it (`Filter slot`, not `Filter slot, Slot`).
- Picking a built-in inserts it as a copy ([sub-patches](sub-patches.md#built-in-abstractions-agreed-2026-11-14)); picking a slot row adds an empty slot ([sub-patches](sub-patches.md#empty-slots-agreed-2026-11-14)). Both become the only selection, as a module does.

### Keyboard and pointer

| Input | Effect |
|---|---|
| Typing | Edits the query. |
| Down, Up | Moves the highlight. It stops at the ends, it does not wrap. |
| Page Down, Page Up | Moves the highlight by a page. |
| Enter | Adds the highlighted module and closes the browser. Does nothing when there are no results. |
| Escape | Closes the browser and adds nothing. |
| Pointer over a row | Highlights it. |
| Click on a row | Adds that module and closes the browser. |
| Click outside the browser | Closes the browser and adds nothing. |
| Right-click on a library item | Opens its menu, with **Insert linked**. |
| The menu key, Shift+F10, or Alt+Enter, on a highlighted library item | Opens its menu on its first item, so Enter picks **Insert linked** at once. On any other row it does nothing. *Why:* before, only the pointer and a screen reader could open the menu, so a keyboard user could not insert a linked sub-patch (WCAG 2.2, SC 2.1.1); these are the keys that open the canvas's menus from the keyboard ([UI layer](ui-layer.md), section 6, rule 8). |

While a library item's menu is open, the menu has the keys, as every menu does ([UI layer](ui-layer.md), section 6, rule 8): Down and Up move to its item, Enter picks the item the keys are on (and does nothing before a key has moved there), and Escape closes the menu, and the browser only on a second press. Any other key closes the menu and goes on as above.

**Focus and Tab** (plan 3.2, row 5, agreed 2026-11-21, built 2026-11-24). The browser is a modal dialog holding a combobox and its list box, by the WAI-ARIA patterns ([APG Dialog (Modal)](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/), [APG Combobox](https://www.w3.org/WAI/ARIA/apg/patterns/combobox/)). Focus stays in the search field, which is the dialog's only Tab stop, so Tab and Shift+Tab stay on it; the list box is never a stop of its own, since its keys come through the field. Up, Down, Page Up, Page Down, Enter, Escape, the menu key, and Shift+F10 go to the browser before the field (`Element::takes_first`; [UI layer](ui-layer.md), section 6, rule 6, step 2b), so Up and Down never move the caret, as the combobox pattern has it; the field keeps every other key. The field's edits reach the browser as field events ([UI layer](ui-layer.md), section 8); Enter is the browser's, taken before the field, so the field never commits, and while an input method composes, Enter confirms the composition and picks nothing. A press on a row, or anywhere in the list, leaves the focus in the field ([UI layer](ui-layer.md), section 6, rule 3). Departures from the APG, each kept from this spec: the highlight stops at the ends rather than wrapping (the APG allows either); Escape closes the browser, since the list is always shown, where the APG would close a popup or clear the field. The field keeps its role, a text input with the highlighted row as its active descendant, as it was listened to on macOS; the APG's combobox role is a later change with its own VoiceOver check. When the browser closes, focus returns to what had it.

**A library item's menu** hangs under its row, from the row's left edge, and stays inside the window. It follows its row as the list scrolls, and closes with no choice once the row scrolls out of view ([UI layer](ui-layer.md), section 5, rule 13). A press on the browser closes only the menu; a press outside both closes both. The focus stays in the search field while the menu is open: its highlighted item is the field's active descendant, and the pointer over the item moves no focus ([UI layer](ui-layer.md), section 6, rule 8).

A screen reader finds the browser as a dialog named "Add module": the search field, which keeps the focus, and the list as a list box whose selected option is the highlighted row. A library item offers its menu to a screen reader (ShowContextMenu), opened on its first item, which becomes the field's active descendant ([UI layer](ui-layer.md), section 7, rule 27).

### Adding

- When A, a double-click, or a right-click gives a pointer position, the tile’s top-left corner goes there, snapped to the grid, even when it overlaps another tile.
- **Toolbar placement (Agreed).** The toolbar supplies no pointer position. Put the tile’s top-left corner on the nearest free grid point to the visible canvas centre where the whole tile fits. Leave one grid step between tiles so the port jacks stay clear. Measure straight-line distance, breaking ties by top then left. If the view is full or too small, use the nearest free grid point even if the tile extends outside the view. This applies to every kind of browser row.
- The new module becomes the only selection.
- Adding is one undo step ([undo.md](undo.md)). Undo removes the module.
- Opening, searching, and closing the browser are not edits and add nothing to the undo history.
- If adding fails, the browser closes and the status bar says what happened and what to do (NFR-UX-05).

## Rules

1. **`A` opens the browser, with no modifier.** Ctrl+A or Cmd+A still selects all. Tab keeps its [canvas focus behaviour](panels.md#keyboard-without-a-mouse). *Why:* one key that always does the same thing is easier to learn than a key whose meaning depends on the selection, and `A` was free.
1. **The search field takes keys while the browser is open.** Canvas shortcuts (Delete, Space for Run, F, Tab focus) do not fire while typing. *Why:* typing `f` in a query must not frame the patch, and Backspace must not delete the selected module.
2. **A pointer-placed module goes where the user opened the browser,** not where auto-layout finds room. *Why:* the user opened it there because that is where they want to patch. A module that appears off screen breaks the flow.
3. **The pointer position is taken when the browser opens,** not when Enter is pressed. *Why:* the pointer may drift onto the list while the user picks a row.
4. **Search reads only the registry's descriptors** (`name`, `id`, `category`, `family`, `description`) for modules, and the role descriptors, the built-ins' names, and the library items' names for the other groups. *Why:* one source of text (ADR-0016). A new module or role appears in the browser with no extra work.
5. **The search code is a plain function with no UI types,** for example `search(registry, query) -> Vec<&ModuleDescriptor>`. *Why:* matching and ranking can then be tested without a window.
6. **The toolbar drop-down goes away.** *Why:* two ways to add a module that behave differently is confusing; the browser covers both.

## Acceptance criteria

- [ ] Given an empty query, the list shows every registered module by family in the family order, then by name. (test)
- [ ] Given the query `saw`, the first result is the saw oscillator. (test)
- [ ] Given the query `osc saw`, only modules whose searchable fields contain both words are listed. (test)
- [ ] Given the query `FILTER`, every Filter module is listed (case does not matter). (test)
- [ ] Given a query with no matches, the list is empty and Enter adds nothing. (test)
- [ ] With the pointer over the canvas, pressing `A` opens the browser at the pointer with the search field focused. (manual)
- [x] With the pointer outside the canvas, `A` opens the browser and Enter adds the chosen module (`keys_add_connect_edit_and_save_with_the_pointer_outside`).
- [ ] Typing a query and pressing Enter adds the highlighted module with its top-left at the snapped pointer position, selects it, and closes the browser. (test for placement; manual for the gesture)
- [ ] Double-click on empty canvas opens the browser there. Double-click on a control still resets the control. (manual)
- [x] A right-click on empty canvas opens the browser there, and the module added goes at the click, snapped to the grid; a right-drag pans and opens nothing (`right_click_on_empty_canvas_opens_the_browser_there`, `a_right_click_on_empty_canvas_opens_the_browser_and_adds_there`).
- [ ] The toolbar button opens the browser and adds at the nearest free position to the centre of the visible canvas. (test)
- [x] Escape and a click outside close the browser and change nothing (`browser::panel::tests::the_menu_key_opens_the_menu_and_escape_closes_it_first`, `a_press_outside_the_menu_closes_it_and_outside_both_closes_both`).
- [ ] While the browser is open, typing `f`, Space, Backspace, or Delete edits the query and does not act on the canvas. (manual)
- [ ] The highlighted module's description is shown, and changes as the highlight moves. (manual)
- [ ] One add is one undo step, and opening or closing the browser adds none. (test)

## Out of scope

- Dropping a module onto a cable to insert it (FR-CANVAS-08, Later).
- Opening the browser from a port drag and auto-connecting the new module (see Open points).
- Favourites, recently used modules, and user presets.
- Searching parameter or port names.
- Help pages ([#43](https://github.com/brylie/LoomDSP/issues/43)). The browser shows only the one-sentence description.
- Fuzzy matching that allows typos.

## Open points

None. Settled when the spec was agreed:

- **The open key is `A`.** Tab was suggested in the issue, but it already moves focus between a selected module's controls ([panels.md](panels.md)).
- **Double-click on empty canvas opens the browser too.** It needs no keyboard. A double-click on a control still resets it.
- **So does a right-click on empty canvas** (Brylie's decision, 2026-11-28: "Build it now"). One click, as a right-click opens the probe menu on an output; a right-drag still pans, by the same 4-pixel allowance ([panels](panels.md), Add scope and Add meter). It replaces the empty-canvas menu the canvas context menu planned (Add module here, Paste here, Select all): Paste and Select all keep their keys, Cmd+V at the pointer and Cmd+A.
- **Auto-connect from a port** is left for a later issue.
- **Every registered module is listed.** Hiding modules waits until one needs it.
- **A library item's menu follows the menu keys** (settled 2026-11-12). Before, any key closed it, and Enter inserted a copy while the menu showed Insert linked, which a keyboard or screen reader user could not get past.
