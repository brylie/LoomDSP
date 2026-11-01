# Port and Parameter Text Specification

Every port and parameter carries a short label and a one-sentence description, so users can learn what a control does where they use it.

- **Kind:** living specification.
- **Status:** Implemented, for ports, parameters, and display values ([#118](https://github.com/brylie/LoomDSP/issues/118)). [Module names and summaries](#module-names-and-summaries): Agreed and implemented, M1 ([#458](https://github.com/brylie/LoomDSP/issues/458)).
- **Decision behind it:** [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md)
- **Use case and requirements:** FR-LIB-11, FR-SDK-01, NFR-UX-04
- **Issues:** #115 (fields and checks), #43 (help pages), [#118](https://github.com/brylie/LoomDSP/issues/118) (display values)
- **Code:** `PortSpec`, `ParamSpec`, `ParamOptions`, `DisplaySpec`, `assert_documented`, and `ModuleDescriptor::undocumented` in `loom-core/src/descriptor.rs`; `Registry::notes` in `loom-core/src/registry.rs`; `Harness::allow_undocumented` in `loom-test`.

## Goal

A user hovers over the `n` knob on Clock Divide and wants to know what it does. The knob should say "Division", and the tooltip "Fires on every Nth trigger."

## Behaviour

A port or parameter has three texts:

| Field | Example | Stored in patches | Can change |
|---|---|---|---|
| `name` | `n` | Yes | Never, once released |
| `label` | Division | No | Any time |
| `description` | Fires on every Nth trigger. | No | Any time |

Write each spec as a struct literal:

```rust
params: &[
    ParamSpec {
        name: "n",
        label: "Division",
        description: "Fires on every Nth trigger.",
        min: 1.0,
        max: 64.0,
        default: 2.0,
        unit: "",
        options: ParamOptions::DEFAULT,
    }
    .integer(),
],
```

## Rules

1. **Every field is named and required.** There is no `new()`. *Why:* a missing field is then a compile error, and rust-analyzer shows it while typing. With five positional numbers, it was hard to tell the minimum from the default.
2. **Optional parameter settings live in `ParamOptions`.** Start from `ParamOptions::DEFAULT` and change them with builder methods such as `.integer()` and `.logarithmic()`. *Why:* a new option later then breaks no module. `ParamSpec` as a whole has no default, because the label and description must always be written. It is a constant, not `Default::default()`, because descriptors are `static`.
3. **Labels and descriptions are not stable IDs.** Reword them whenever it helps. *Why:* patch files store only names, so a new text never breaks a patch. Translation can look text up by module ID plus name.
4. **A label is a short display name in Title Case**, usually one or two words. *Why:* it sits under a knob on a tile.
5. **A description is one short sentence that fits a tooltip.** Follow the writing style in AGENTS.md: plain words, no em dashes. Longer theory goes in an explainer in `docs/learn/` and in the help page. *Why:* many readers use English as a second language, and a tooltip has little room.
6. **Missing text is checked in three places.** Text that is empty or only spaces counts as missing.
   - Built-in modules: `const _: () = loom_core::assert_documented(ALL);` in `loom-modules`, so a missing text is a compile error that names the module and port.
   - Other modules: the `loom-test` harness fails every render of a module with missing text. `.allow_undocumented()` turns this off for a quick experiment.
   - Loading: `Registry::register` still loads the module, and adds a note to `Registry::notes`. `loom check` prints the notes. *Why:* missing help text is worth fixing, but not worth refusing a patch.

## Acceptance criteria

- [x] `PortSpec` and `ParamSpec` have required `label` and `description` fields and no `new()`.
- [x] Every built-in port and parameter has a label and a description, and the crate does not compile if one is empty.
- [x] The harness fails on missing text unless `.allow_undocumented()` is called.
- [x] The registry loads a module with missing text and keeps a note; `loom check` prints the notes.

## Display values

**Status:** Implemented ([#118](https://github.com/brylie/LoomDSP/issues/118)). Code: `DisplaySpec` in `loom-core/src/descriptor.rs`, `panel_view` and `display_text` in `loom-app/src/scene.rs`, `hover_content` in `loom-canvas/src/widget/hover_text.rs`.

A module's display values (the lights and step positions of [panels](panels.md#display-values-69)) get the same treatment.

- A `ModuleDescriptor` lists them as `displays: &[DisplaySpec]`. `DisplaySpec { name, label, description, kind }` is a struct literal with every field required, like `PortSpec`. `name` is the stable ID (used by panels and `Module::displays`), `label` is the caption, `description` is one sentence, and `kind` is `Level` or `Trigger` ([panels](panels.md#lights-for-one-sample-triggers)). At most 4 per module, as now.
- **The same three checks** as ports and parameters: `assert_documented`, the `loom-test` harness, and `Registry::notes`. The same label and description rules (rules 4 and 5) apply. The label of a light is its caption, so "Level" for ADSR's `level` becomes "Envelope Level" if that reads better.
- **Light captions show the label,** not the name.
- **Lights show hover text:** the label in bold, then the description, then the current value as text: the label and the value, such as "Gate: 1.00". A `Level` value shows as a number with two decimals. A `Trigger` value shows "lit" while its light is lit, else "dark". *Why:* a light with no explanation is a mystery on a front panel. *Why a number, not words such as "high":* a display value has no unit yet, so the UI cannot tell a gate from a level. Words can come with display value units (see the open question in [#118](https://github.com/brylie/LoomDSP/issues/118)).
- **Meters get hover text from the output they show.** A meter is bound to an output port, which already has a label and a description, so it shows those, plus the RMS level in dB ("Out: -12.0 dB", or "below -60 dB"). It gets no `DisplaySpec` of its own. *Why:* two texts for one signal could disagree.
- This changes the descriptor API and every module with display values (ADSR, MIDI Note, Memory, and the test descriptors). Early alpha allows it: update them all in the same pull request.

### Acceptance criteria: display values

- [x] `DisplaySpec` has required `label` and `description` fields, and the crate does not compile if one is empty.
- [x] The harness and the registry check them as for ports and parameters.
- [x] A light's caption is its label, and hovering it shows the description and the value. A test beside `tiles_show_labels_not_names` in `crates/loom-app/src/scene.rs` checks the labels.
- [x] A meter's hover text is its output's label, description, and level.

## Module names and summaries

**Status:** Agreed and implemented, M1 ([#458](https://github.com/brylie/LoomDSP/issues/458)). Code: `module_text_follows_the_rules` in `crates/loom-modules/src/lib.rs`.

A module's name and one-line summary are the first text a user meets: in the browser, hover text, the inspector, and help. They speak to a musician.

1. **Names are Title Case.** Every major word starts with a capital, including the part after a hyphen: All-Pass, One-Pole, 1-Sample Delay, Phase Wrap and Fold, Sample and Hold. Short joining words (and, or, of, the, a, to, in) stay lower case unless they come first. Acronyms keep their capitals (SVF, LFO, MIDI). *Why:* one rule, so names read as names and sort predictably.
2. **The kind of value comes first in a name.** Constant, Level Constant, Pitch Constant. *Why:* parentheses read as an afterthought, and "Level Constant" says what it makes.
3. **A summary is at most two sentences, and the first says what the module does for the user.** "A filter that keeps the low, band, high, or notch part of a sound", not "State variable filter (trapezoidal integration)". *Why:* the summary shows in a tooltip and the browser's description line.
4. **No internal method names, no requirement IDs, no claims of sound quality** in a summary or any descriptor text. How it is built (polyBLEP, trapezoidal integration, cubic interpolation) goes in the hand-written notes, with an explainer link where a term needs one. Words like warm, rich, classic, pristine, or ideal are left out: the user can hear the sound. *Why:* the plain style in AGENTS.md, and readers with English as a second language.
5. **A choice parameter's range is its choices,** such as "ADSR or AD", in help and the generated pages. A parameter with several control ranges gives its full range, then the control ranges as their own sentence.
6. **Hand-written notes follow the same style:** short sentences, plain words, facts that match the code, and module names as they are now.

### Acceptance criteria: module names and summaries

- [x] Every built-in module name is Title Case with the hyphen rule above (test over the registry).
- [x] No descriptor text contains `FR-`, `NFR-`, or a word from the deny list (polyBLEP, polyBLAMP, trapezoidal, Nyquist, warm, rich, classic, pristine, ideal) (test over the registry).
- [x] Every module summary is at most two sentences (test).
- [x] Choice parameters show their choices, not "0 to 1, whole numbers", in help and the generated pages (test on the range text).
- [x] The hand-written notes that used sales words, unexplained jargon, or old names are rewritten, and the help pages are regenerated.

## Out of scope

- Showing labels on tiles and in the inspector, and descriptions on hover: [Labels and hover text](panels.md#labels-and-hover-text) in panels.md (Implemented).
- Generating help pages: [module help pages](help-pages.md) (#43).
- Text for panel widgets. (Display values are above.)
