# Module Help Pages Specification

Every built-in module has a Markdown help page, generated from its descriptor, so learners can read what a module does, what each port and parameter means, and how to use it.

- **Kind:** living specification.
- **Status:** Agreed and Implemented, P1. The command (`loom modules --markdown DIR` and `--check`), the folder `docs/modules/`, the marker line, and the stale-page test were agreed in review of [#385](https://github.com/brylie/LoomDSP/pull/385).
- **Decision behind it:** none of its own. Builds on [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md) (ports and parameters carry their own text) and [ADR-0018](../adr/0018-clean-room-module-development.md) (our own words).
- **Use case and requirements:** FR-LIB-11.
- **Issues:** [#43](https://github.com/brylie/LoomDSP/issues/43).
- **Code:** `crates/loom-cli/src/help_pages.rs`, the `--markdown` option of `loom modules` in `crates/loom-cli/src/main.rs`. Pages: [`docs/modules/`](../modules/README.md).

## Goal

> I added a Clock Divide and want to know what Offset does, and what the Reset input reads when nothing is plugged in.

The descriptor already holds this: each port and parameter has a label, a one-sentence description, a signal type, a default, and a range ([port and parameter text](port-and-parameter-text.md)). A help page shows it in one place, and leaves room for longer notes and an example patch.

## Behaviour

`loom modules --markdown DIR` writes one page per registered module into `DIR`, named after the module's ID (`loom.filter.svf.md`), and an index, `README.md`. The repository's pages live in `docs/modules/`. `mise run help-pages` regenerates them. With `--check`, the command writes nothing and exits with an error if any page is out of date, for scripts.

A page has two parts:

| Part | Holds | Who writes it |
|---|---|---|
| Generated, from the top to the marker line | Name, description, ID, version, category; inputs with signal type and what they read unconnected; outputs with signal type and polarity; parameters with range, unit, whole-number and logarithmic marks, choice names, and default; the panel's widgets, with each light's label and description. | The command. Never edit it by hand. |
| Notes, below the marker line | Longer explanation, an example patch, links to explainers in `docs/learn/`. | People. |

The marker line is:

```markdown
<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->
```

The index lists every module by category, with its name, ID, and description, and links to its page. It is all generated.

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

## Out of scope

- Showing help pages inside the app. The module browser shows the one-sentence description ([module browser](module-browser.md)).
- Text for display values is listed with each light, from its `DisplaySpec` ([port and parameter text](port-and-parameter-text.md#display-values)).
- Translation (NFR-UX-04).
- HTML output or a documentation website.

## Open points

None. In review of [#393](https://github.com/brylie/LoomDSP/issues/393), example patches were agreed to live in hand-written notes below the marker line as minimal inline RON snippets, keeping the generated part strictly descriptor-derived (Rule 1). Notes link to full patches in `examples/patches/` when available.
