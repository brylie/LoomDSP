# Module Browser Specification

A searchable list of module types that adds the chosen module to the canvas, where the user is looking. It is for everyone who builds a patch, and fastest for users who know what they want.

- **Kind:** living specification.
- **Status:** Agreed
- **Decision behind it:** [ADR-0010](../adr/0010-own-patch-canvas.md) (fast insertion on our own canvas).
- **Use case and requirements:** FR-CANVAS-03, FR-CANVAS-06, NFR-UX-01, NFR-UX-05.
- **Prior art:** many node editors open a search box at the pointer when you press a key on the canvas, and add the chosen node there. We use the idea, not any product's layout or wording.
- **Issues:** [#11](https://github.com/brylie/LoomDSP/issues/11)
- **Code:** `crates/loom-app` (not built yet).

<!-- Spec first: agree on this before building. Keep it true as the code is
     built. See README.md in this folder. -->

## Goal

"I know I want a filter. Let me type `filt`, press Enter, and have it appear where my pointer is, so I can cable it up straight away."

Today the toolbar has a drop-down of module IDs and an Add button. It lists every ID in one long list, shows nothing about what a module does, and puts the new module wherever auto-layout finds space, often off screen. A user has to know the ID, find it, and then find the new tile.

## Behaviour

### Opening the browser

| Gesture | Where the browser opens | Where the module goes |
|---|---|---|
| `A` with the pointer over the canvas | At the pointer | At the pointer, snapped to the grid |
| Double-click on empty canvas | At the pointer | At the click, snapped to the grid |
| "Add module" button in the toolbar | Under the button | Centre of the visible canvas, snapped to the grid |

The browser is a small floating panel: a search field on top, a list of matches below, and the highlighted module's description at the bottom. The search field has keyboard focus as soon as the browser opens, so the user can type at once.

### Searching

- Each module shows its **name**, its **category**, and its **ID** in a quieter style.
- An empty search lists every module, grouped by category, categories in alphabetical order, modules by name within each.
- Typing filters the list. A module matches when every word of the query is found in its name, ID, or category, ignoring case. Example: `osc saw` matches `loom.osc.saw`; `filter` matches every module in the Filter category.
- Results are ranked: a name that starts with the query first, then a word in the name that starts with it, then any other match. Ties keep the empty-search order.
- The first result is highlighted after each change to the query.
- Below the list, the highlighted module's one-sentence `description` is shown, so the user can tell what it does before adding it.

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

### Adding

- The new module's top-left corner goes at the chosen point, snapped to the grid. If that spot overlaps another tile, it still goes there; the user moves it.
- The new module becomes the only selection.
- Adding is one undo step ([undo.md](undo.md)). Undo removes the module.
- Opening, searching, and closing the browser are not edits and add nothing to the undo history.
- If adding fails, the browser closes and the status bar says what happened and what to do (NFR-UX-05).

## Rules

1. **`A` opens the browser, with no modifier.** Ctrl+A or Cmd+A still selects all. Tab keeps moving focus between controls. *Why:* one key that always does the same thing is easier to learn than a key whose meaning depends on the selection, and `A` was free.
1. **The search field takes keys while the browser is open.** Canvas shortcuts (Delete, Space for Run, F, Tab focus) do not fire while typing. *Why:* typing `f` in a query must not frame the patch, and Backspace must not delete the selected module.
2. **The module goes where the user opened the browser,** not where auto-layout finds room. *Why:* the user opened it there because that is where they want to patch. A module that appears off screen breaks the flow.
3. **The pointer position is taken when the browser opens,** not when Enter is pressed. *Why:* the pointer may drift onto the list while the user picks a row.
4. **Search reads only the registry's descriptors** (`name`, `id`, `category`, `description`). *Why:* one source of text (ADR-0016). A new module appears in the browser with no extra work.
5. **The search code is a plain function with no UI types,** for example `search(registry, query) -> Vec<&ModuleDescriptor>`. *Why:* matching and ranking can then be tested without a window.
6. **The toolbar drop-down goes away.** *Why:* two ways to add a module that behave differently is confusing; the browser covers both.

## Acceptance criteria

- [ ] Given an empty query, the list shows every registered module, grouped by category in alphabetical order. (test)
- [ ] Given the query `saw`, the first result is the saw oscillator. (test)
- [ ] Given the query `osc saw`, only modules whose name, ID, or category contain both words are listed. (test)
- [ ] Given the query `FILTER`, every Filter module is listed (case does not matter). (test)
- [ ] Given a query with no matches, the list is empty and Enter adds nothing. (test)
- [ ] With the pointer over the canvas, pressing `A` opens the browser at the pointer with the search field focused. (manual)
- [ ] Typing a query and pressing Enter adds the highlighted module with its top-left at the snapped pointer position, selects it, and closes the browser. (test for placement; manual for the gesture)
- [ ] Double-click on empty canvas opens the browser there. Double-click on a control still resets the control. (manual)
- [ ] The toolbar button opens the browser and adds at the centre of the visible canvas. (manual)
- [ ] Escape and a click outside close the browser and change nothing. (manual)
- [ ] While the browser is open, typing `f`, Space, Backspace, or Delete edits the query and does not act on the canvas. (manual)
- [ ] The highlighted module's description is shown, and changes as the highlight moves. (manual)
- [ ] One add is one undo step, and opening or closing the browser adds none. (test)

## Out of scope

- Dropping a module onto a cable to insert it (FR-CANVAS-08, P2).
- Opening the browser from a port drag and auto-connecting the new module (see Open points).
- Favourites, recently used modules, and user presets.
- Searching parameter or port names.
- Help pages ([#43](https://github.com/brylie/LoomDSP/issues/43)). The browser shows only the one-sentence description.
- Fuzzy matching that allows typos.

## Open points

None. Settled when the spec was agreed:

- **The open key is `A`.** Tab was suggested in the issue, but it already moves focus between a selected module's controls ([panels.md](panels.md)).
- **Double-click on empty canvas opens the browser too.** It needs no keyboard. A double-click on a control still resets it.
- **Auto-connect from a port** is left for a later issue.
- **Every registered module is listed.** Hiding modules waits until one needs it.
