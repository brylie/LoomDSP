# Module Help Pages Specification

Every built-in module has a Markdown help page, generated from its descriptor, so learners can read what a module does, what each port and parameter means, and how to use it.

- **Kind:** living specification.
- **Status:** Agreed and Implemented, P1, except [Help in the app](#help-in-the-app), which is Draft. The command (`loom modules --markdown DIR` and `--check`), the folder `docs/modules/`, the marker line, and the stale-page test were agreed in review of [#385](https://github.com/brylie/LoomDSP/pull/385).
- **Decision behind it:** none of its own. Builds on [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md) (ports and parameters carry their own text) and [ADR-0018](../adr/0018-clean-room-module-development.md) (our own words).
- **Use case and requirements:** FR-LIB-11.
- **Issues:** [#43](https://github.com/brylie/LoomDSP/issues/43), [#388](https://github.com/brylie/LoomDSP/issues/388) (help in the app).
- **Code:** `crates/loom-cli/src/help_pages.rs`, the `--markdown` option of `loom modules` in `crates/loom-cli/src/main.rs`. Pages: [`docs/modules/`](../modules/README.md).

## Goal

> I added a Clock Divide and want to know what Offset does, and what the Reset input reads when nothing is plugged in.

The descriptor already holds this: each port and parameter has a label, a one-sentence description, a signal type, a default, and a range ([port and parameter text](port-and-parameter-text.md)). A help page shows it in one place, and leaves room for longer notes and an example patch.

## Behaviour

`loom modules --markdown DIR` writes one page per registered module into `DIR`, named after the module's ID (`loom.filter.svf.md`), and an index, `README.md`. The repository's pages live in `docs/modules/`. `mise run help-pages` regenerates them. With `--check`, the command writes nothing and exits with an error if any page is out of date, for scripts.

A page has two parts:

| Part | Holds | Who writes it |
|---|---|---|
| Generated, from the top to the marker line | Name, description, ID, version, category; inputs with signal type and what they read unconnected; outputs with signal type and polarity; parameters with range, unit, whole-number and logarithmic marks, and default; the panel's widgets. | The command. Never edit it by hand. |
| Notes, below the marker line | Longer explanation, an example patch, links to explainers in `docs/learn/`. | People. |

The marker line is:

```markdown
<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
```

The index lists every module by category, with its name, ID, and description, and links to its page. It is all generated.

## Rules

1. **The descriptor is the one source.** The generated part only shows descriptor fields. *Why:* the descriptor is what the engine and the app use, so a page made from it cannot disagree with the module.
2. **Regeneration keeps everything below the marker.** *Why:* notes and example patches take real thought to write, and a descriptor change must not erase them.
3. **A page without the marker is not touched.** The command reports it and exits with an error. *Why:* the page may hold hand-written text that the command cannot tell apart from an old generated part.
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
- [ ] Every built-in module has hand-written notes with an example patch (FR-LIB-11). Follow-up work, one module at a time.

## Help in the app

**Status:** Draft ([#388](https://github.com/brylie/LoomDSP/issues/388)). Do not build from it until the open questions below are answered and this section is marked Agreed.

### Goal

> I am patching and I see a Clock Divide tile. I want to read what it does and hear an example, without leaving the app.

The module browser shows only the one-sentence description ([module browser](module-browser.md)), and hover text explains one port or control at a time ([panels](panels.md#labels-and-hover-text)). A learner needs the whole page where they patch (FR-LIB-11, product principle 11).

### Proposal

- **The app draws help from the descriptor.** It shows the same sections as the generated part of a page: description, inputs with what they read when unconnected, outputs, and parameters with ranges and defaults. It builds them from the `ModuleDescriptor` it already holds, not from the Markdown files. *Why:* rule 1 holds in the app too, and help then works offline, with no files to find at run time, and always matches the running build.
- **Only the hand-written notes and the example patch are built into the app**, at compile time (for example with `include_str!`). They are the part of a page that the descriptor does not hold. *Why:* the app must not depend on where it is installed or on the repository being present.
- **One help view, three ways in:**
  1. The module browser: a key shows the highlighted module's help before it is added.
  2. The inspector: a help action for the selected module.
  3. A tile: a menu item, and a key on the selected tile.
  *Why:* the user meets a module in all three places. One view keeps the text and its layout in one place.
- **Keyboard first.** Every way in has a key, and the view closes with Escape, like the module browser. *Why:* NFR-UX-01, and keyboard work in [#101](https://github.com/brylie/LoomDSP/issues/101) and [#190](https://github.com/brylie/LoomDSP/issues/190).
- **"Open example"** loads the module's example patch, so the learner can hear the module at once. A module without an example shows no such action.
- **Plain data first.** The view is built from a plain-data model, like the inspector rows (ADR-0021), so app tests and `--dump-ui` can read what it shows.

### Open questions

1. Where does the help view live: a panel beside the inspector, an overlay like the module browser, or its own window?
2. Which key opens it (F1, `?`, or both)? In the module browser the search field has focus, so `?` would be typed into it.
3. What does "Open example" do with the current patch: replace it, with the usual unsaved-changes prompt ([files](files.md)), or add the example next to it?
4. Should the view also link to the full page on GitHub, or stay fully offline?
5. How are the notes drawn: as Markdown with Iced's `markdown` widget, or as plain text? Markdown keeps links and lists; plain text keeps the view simple.

### Acceptance criteria (draft)

- [ ] From the module browser, the inspector, and a selected tile, one key opens the help view for that module, and Escape closes it.
- [ ] The view shows the same description, ports, and parameters as the module's generated page, built from its descriptor.
- [ ] A module with hand-written notes shows them below the generated part.
- [ ] "Open example" loads the module's example patch, as question 3 decides.
- [ ] `App::ui_snapshot()` and `--dump-ui` include the help view's model while it is open.

## Out of scope

- Showing the Markdown files themselves inside the app. The app draws help from descriptors ([Help in the app](#help-in-the-app)).
- Text for display values: comes with `DisplaySpec` ([port and parameter text](port-and-parameter-text.md#display-values)). Until then, lights are listed by name.
- Translation (NFR-UX-04).
- HTML output or a documentation website.

## Open points

- Should the generated part link example patches in `examples/patches/` that use the module? It would need the command to know where the examples are.
