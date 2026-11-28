# Module Help Pages Specification

Every built-in module has a Markdown help page, generated from its descriptor, so learners can read what a module does, what each port and parameter means, and how to use it.

- **Kind:** living specification.
- **Status:** Agreed and Implemented, M1. The command (`loom modules --markdown DIR` and `--check`), the folder `docs/modules/`, the marker line, and the stale-page test were agreed in review of [#385](https://github.com/brylie/LoomDSP/pull/385).
- **Decision behind it:** none of its own. Builds on [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md) (ports and parameters carry their own text) and [ADR-0018](../adr/0018-clean-room-module-development.md) (our own words).
- **Use case and requirements:** FR-LIB-11.
- **Issues:** [#43](https://github.com/brylie/LoomDSP/issues/43), [#388](https://github.com/brylie/LoomDSP/issues/388).
- **Code:** `crates/loom-cli/src/help_pages.rs`, the `--markdown` option of `loom modules` in `crates/loom-cli/src/main.rs`. Pages: [`docs/modules/`](../modules/README.md).

## Goal

> I added a Clock Divide and want to know what Offset does, and what the Reset input reads when nothing is plugged in.

The descriptor already holds this: each port and parameter has a label, a one-sentence description, a signal type, a default, and a range ([port and parameter text](port-and-parameter-text.md)). A help page shows it in one place, and leaves room for longer notes and an example patch.

## Behaviour

`loom modules --markdown DIR` writes one page per registered module into `DIR`, named after the module's ID (`loom.filter.svf.md`), and an index, `README.md`. The repository's pages live in `docs/modules/`. `mise run help-pages` regenerates them. With `--check`, the command writes nothing and exits with an error if any page is out of date, for scripts.

The same command writes one page per registered slot role from its role descriptor. It lists the required and optional ports and parameters, and links to the registered modules that fill the role. Each module page links to any roles its descriptor declares. The index lists the roles after the module families.

A page has two parts:

| Part | Holds | Who writes it |
|---|---|---|
| Generated, from the top to the marker line | Name, description, ID, version, family, category; inputs with signal type and what they read unconnected; outputs with signal type and polarity; parameters with range, unit, whole-number and logarithmic marks, choice names, and default; the panel's widgets, with each light's label and description. | The command. Never edit it by hand. |
| Notes, below the marker line | Longer explanation, an example patch, links to explainers in `docs/learn/`. | People. |

The marker line is:

```markdown
<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
```

The index lists every module by family, in the module browser's order ([ADR-0029](../adr/0029-module-families-and-a-shared-palette.md)), with its name, ID, and description, and links to its page. It is all generated.

## Rules

1. **The descriptor is the one source.** The generated part only shows descriptor fields. *Why:* the descriptor is what the engine and the app use, so a page made from it cannot disagree with the module.
2. **Regeneration keeps everything below the marker.** *Why:* notes and example patches take real thought to write, and a descriptor change must not erase them.
3. **A page without the marker is not touched.** Only a line that is exactly the marker counts; a page that quotes it inside other text has none. The command reports it and exits with an error. *Why:* the page may hold hand-written text that the command cannot tell apart from an old generated part.
4. **A page of a module that no longer exists is reported, not deleted.** The command exits with an error, so its notes can be moved before the page is removed by hand. *Why:* same as rule 3.
5. **A test fails while the committed pages are out of date** (`committed_pages_are_current` in `help_pages.rs`). Its message names each stale page and says how to regenerate. *Why:* CI then keeps the pages and the descriptors in step, without a person remembering.
6. **Line endings do not count.** A checkout with Windows line endings is not stale. *Why:* CI runs the tests on Windows too.
7. **Our own words only.** Pages show descriptor text, which already follows [ADR-0018](../adr/0018-clean-room-module-development.md). Hand-written notes follow it too: no text from other products' manuals.

## Acceptance criteria

- [x] `loom modules --markdown DIR` writes one page per module and an index. `loom modules --help` describes the option.
- [x] Each page lists ports with label, name, signal type, and unconnected default (outputs: polarity), and parameters with label, name, range, unit, and default.
- [x] Text below the marker survives regeneration, and a second run writes nothing (`regenerating_keeps_notes`).
- [x] A page without the marker and a page of an unknown module are kept and reported (`unmarked_and_unknown_pages_are_kept`).
- [x] A test fails, with the command to run, when a committed page is stale (`committed_pages_are_current`).
- [x] Every built-in module has hand-written notes with an example patch (FR-LIB-11).

## Help in the app

**Status:** Agreed and implemented.

Opening help releases keyboard focus from the search or Author field in either browser. Closing help restores that field's focus if it is still present.

The app shows a module's help in an overlay over a dimmed backdrop. F1, or Cmd+/ (Ctrl+/ off a Mac, with or without Shift; [UI layer](ui-layer.md), section 6, rule 13), opens it for the highlighted module in the browser, including while the search field has focus, or for the selected tile; with neither, F1 opens the **Keyboard shortcuts** page, which the inspector's button also opens. The inspector has a help action, and the tile menu has a help item. It opens on top of Your patches, the licence chooser, and Preferences, which wait underneath, and only a question is drawn over it. Escape, the **Close** button, or a click on the backdrop closes the overlay and nothing else; a click inside it does nothing else. The same help view is used for all entry points.

A slot tile's **Help** menu item opens the role's page in the same view. Its port hover text uses the role port's label and description. Links between modules and roles open the corresponding help in the app.

The overlay has a fixed title row with **Close**, and one scrolling body in reading order ([#457](https://github.com/brylie/LoomDSP/issues/457)): the family, category, and ID; the summary; the hand-written notes up to their example; **Parameters**, **Inputs**, and **Outputs**; then **Open example** and the example section. Each entry shows its label, its details in words with units ("20 to 20000 Hz, logarithmic. Default 1000 Hz.", "audio; reads 0 with no cable", "audio, bipolar"), and its description. Text wraps inside the overlay, and the scrollbar takes its own space. Sizes come from the [visual system](visual-system.md); the details use the same text as the generated pages (`ParamSpec::range_text`, `PortDefault::text`).

The view builds its description, port and parameter information from the module descriptor, as the generated page does. It shows the hand-written notes as Markdown, split around the reference sections as above. Notes and example patches are bundled at compile time, so help works without the repository or a network connection. It does not add a separate link to the GitHub page.

When a module has an example, **Open example** opens it as a new patch. It uses the usual unsaved-changes prompt before replacing the current patch. A module without an example has no Open example action. The view has a plain-data model exposed through `App::ui_snapshot()` and `--dump-ui`. Use `--select ID --show-help` with `--dump-ui` or `--screenshot-window` to inspect it without a window. Links to other module pages open their bundled help; links to other documents report that they are not bundled.

### Keys, links, and focus

**Status:** Agreed and built (plan 3.2, row 6, 2026-11-21): help is a panel of `loom-ui` (`help/panel.rs`).

Help is a modal dialog, by the WAI-ARIA dialog pattern ([APG Dialog (Modal)](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/)).

- **Focus on open** goes to **Close**, the first control, as the APG's dialog pattern advises; it is at the top of the box, so nothing scrolls away. Opened from the keyboard (F1, or Enter or Space on a button or a menu item), Close shows the focus ring, so a keyboard user sees where the focus went; opened by the pointer, it shows none, as a browser decides `:focus-visible` when a script moves the focus (`Effects::focus_as_input`; the navigator's ruling, 2026-11-21). Changed from this spec's first text, which put the focus on the dialog itself: a focus has to be on a stop, and with nothing focused the scroll keys reached nothing.
- **Tab** and Shift+Tab cycle the dialog's stops in reading order and stay inside it: Close, the links of the notes, **Open example**, the links of the example section, then Close again.
- **Escape** closes help, as **Close** and a press on the backdrop do. Focus returns to what had it before help opened, if it is still there; otherwise to the first stop of its panel, or to the canvas ([UI layer](ui-layer.md), section 6, rule 4).
- **Links** follow the WAI-ARIA link pattern ([APG Link](https://www.w3.org/WAI/ARIA/apg/patterns/link/)): each is a Tab stop, Enter follows it, and Space does not. A press follows it and takes no focus. A link is underlined in the link colour, so it is told apart by more than colour (WCAG 2.2, SC 1.4.1). Following a link shows the new page from its top, with focus on Close again, so Tab starts over, as on a new page in a browser.
- **Links to pages that are not bundled** (a `../learn/` explainer, a design document, an example patch file) show as plain text in the app: no underline, no node, no stop, since the app cannot follow them. The pages in `docs/modules/` keep them as links. *Why:* a link that only warns, in a status line the open dialog hides, is a dead end for a keyboard or screen reader user.
- **Scrolling** by keyboard: Up, Down, Page Up, Page Down, Home, and End scroll the body while the focus is on Close, on a link, or on Open example, as a browser scrolls a page from a focused link. From a link or Open example, the key goes to the body around it; from Close, above the body, the body scrolls from Close (`Element::scrolls_from`). Space on Close or Open example clicks it, as on any button; on a link it follows nothing (the APG link pattern) and scrolls the body a page, as a browser's Space scrolls a page from a focused link, and Shift+Space a page back. *Why:* without it, text after the last stop could not be reached by keyboard (WCAG 2.2, SC 2.1.1).
- **A press on the backdrop** closes help, as Escape and **Close** do; a press inside the box does nothing else. The backdrop is a panel of its own under the box's, so a press beside the box reaches it.
- **Code blocks wrap** inside the dialog, in the monospace face, rather than scroll sideways (WCAG 2.2, SC 1.4.10, Reflow).
- **On the web and in a plugin**, Tab stays inside the dialog as in the stand-alone app, and Escape is the way out (WCAG 2.2, SC 2.1.2), as for the question.

### Acceptance criteria

- [x] F1 opens help for the highlighted browser item, even with search focused, and for the selected tile. The inspector action and tile menu open the same view. Escape closes it.
- [x] Cmd+/ (Ctrl+/ off a Mac), with or without Shift, opens help as F1 does, from the canvas and from the browser's search, and by what it types on German and French layouts (`the_slash_shortcut_opens_help_as_f1_does`, `the_help_key_follows_the_layout`).
- [x] The view shows description, ports and parameters from the descriptor and renders bundled hand-written notes as Markdown.
- [x] Open example loads the bundled example after the usual unsaved-changes flow; it is absent when there is no example.
- [x] With nothing selected, the inspector shows two short lines and a **Keyboard shortcuts** button; it and F1 open the shortcuts page (test, [#457](https://github.com/brylie/LoomDSP/issues/457)).
- [x] Help details read as words with units, with no "unconnected:" and no stray spaces (test).
- [x] Help text stays inside the overlay; a click on the backdrop closes it and a click inside does not (test).
- [x] Tab cycles Close, the links, and Open example, and stays inside; focus starts on Close, with a ring only from the keyboard; Escape returns focus to what had it (test).
- [x] Enter follows a focused link and Space does not; a link to a page that is not bundled is plain text with no node (test).
- [x] The scroll keys move the body from Close and from a link, and a code block wraps (test).
- [ ] VoiceOver reads each link by its words, and moves to it by Tab and by its own cursor (manual: on the M2 QA issue, #643).
- [x] UI snapshots and `--dump-ui` report the open help view and its content.
- [x] Help and examples work without access to the repository or network.

## Out of scope
- Text for display values is listed with each light, from its `DisplaySpec` ([port and parameter text](port-and-parameter-text.md#display-values)).
- Translation (NFR-UX-04).
- HTML output or a documentation website.

## Open points

None. In review of [#393](https://github.com/brylie/LoomDSP/issues/393), example patches were agreed to live in hand-written notes below the marker line as minimal inline RON snippets, keeping the generated part strictly descriptor-derived (Rule 1). Notes link to full patches in `examples/patches/` when available.
