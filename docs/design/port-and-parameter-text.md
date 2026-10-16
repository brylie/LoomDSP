# Port and Parameter Text Specification

Every port and parameter carries a short label and a one-sentence description, so users can learn what a control does where they use it.

- **Kind:** living specification.
- **Status:** Implemented
- **Decision behind it:** [ADR-0016](../adr/0016-ports-and-parameters-carry-their-own-text.md)
- **Use case and requirements:** FR-LIB-11, FR-SDK-01, NFR-UX-04
- **Issues:** #115 (fields and checks), #43 (help pages)
- **Code:** `PortSpec`, `ParamSpec`, `ParamOptions`, `assert_documented`, and `ModuleDescriptor::undocumented` in `loom-core/src/descriptor.rs`; `Registry::notes` in `loom-core/src/registry.rs`; `Harness::allow_undocumented` in `loom-test`.

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

## Out of scope

- Showing labels on tiles and in the inspector, and descriptions on hover: [Labels and hover text](panels.md#labels-and-hover-text) in panels.md (Implemented).
- Generating help pages: #43.
- Text for display values and panel widgets.
