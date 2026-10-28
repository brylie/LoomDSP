# ADR-0016: Ports and parameters carry a label and a description

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-16
- **Related requirements:** FR-LIB-11, FR-SDK-01, NFR-UX-04
- **Related decisions:** ADR-0008 (patch format), ADR-0013 (modules carry their own panel)
- **Specification:** [Port and parameter text](../design/port-and-parameter-text.md)

## Context

A port or parameter had only a stable name, such as `n` or `offset`. The name is an ID: patch files store it, so it can never change (AGENTS.md hard rule 5). That makes it a poor thing to show a user. Bitwig Grid and VCV Rack show names such as "Division", and a tooltip that says what the control does.

Help pages (FR-LIB-11), hover text, and translation (NFR-UX-04) all need text for each port and parameter. Product principle 11, "Every decision teaches", needs it too: a user should be able to learn what a control does without leaving the app.

## Options considered

1. **A description only, and keep showing the name.** Smallest change. But the text a user reads most, the label under a knob, stays an ID that cannot be improved or translated.
2. **A description, and an optional label that falls back to the name.** No extra writing per module. But most modules would keep showing IDs, and a translator could not tell a missing label from a deliberate one.
3. **A required label and a required description.** Every module author writes both. A missing one is a compile error for built-in modules and a test failure for others.

## Decision

Option 3. Every `PortSpec` and `ParamSpec` has a required `label` (a short display name) and a required `description` (one sentence). Neither is an ID: patch files never store them, so they can be reworded at any time. The name stays the only stable ID.

## Consequences

- Module authors write two short texts per port and parameter. Checks catch a missing one: at compile time for built-in modules, in the `loom-test` harness for others, and as a note when a module is registered.
- The UI can show labels instead of names, and descriptions on hover, without another change to every module. Those UI changes are separate work.
- Translation can look text up by module ID plus port or parameter name, with the English text as the source.
- `PortSpec` and `ParamSpec` are written as struct literals with named fields, so a missing field is a compile error. Optional parameter settings move into `ParamOptions`, so adding one later breaks no module.
