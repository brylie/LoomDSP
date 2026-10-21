# ADR-0024: Widgets follow the kind of value, and structured data has shared kinds

- **Status:** Proposed
- **Date:** 2026-10-21
- **Related requirements:** FR-PANEL-02, FR-PANEL-04, FR-LIB-12, FR-LIB-17, FR-LIB-18, FR-LIB-19, FR-MOD-04, NFR-UX-01, NFR-UX-02
- **Related decisions:** ADR-0001 (lock-free boundary), ADR-0013 (modules carry their own panel), ADR-0014 (modulation without cables), ADR-0017 (primitives, traits, and abstractions), ADR-0018 (clean room)
- **Specification:** [Panel widgets](../design/panel-widgets.md)

## Context

ADR-0013 gave every module a panel built from one shared widget set, and said new kinds are added to the set "when a real module needs one". Six widgets exist: knob, toggle, radio buttons, button, light, and meter.

The [module catalogue](../product/module-catalogue.md) plans about 170 module types. Many of them need things the six cannot show: a filter slope picked from a list, a curve to draw, a step pattern, a sample with loop points, one signal against another. If each new widget is designed when its first module arrives, each will be shaped by that module. The second module that needs a curve will find an "envelope editor" with envelope gestures, and either bend to it or ask for its own. Users would then learn a curve editor per module.

Curves, patterns, and samples are also not parameters. They are module data (FR-LIB-19, OQ-17), which does not exist yet. How a widget binds to them must be settled before either is built, or the first data editor will fix the format for all.

## Options considered

1. **Grow the set one module at a time.** Design each widget when its first module needs it. Least work up front, and nothing is built before it is needed. But each widget fits one module, and modules that need the same kind of interaction can end up with different widgets for it.
2. **A drawing widget that modules fill.** One general widget where a module draws lines, shapes, and text from data it publishes. Very flexible. But every module then designs its own gestures, keyboard use, modulation display, and level of detail, which is what ADR-0013 set out to avoid, and a screen reader learns nothing from lines.
3. **One widget family per kind of value, and shared data kinds.** Group widgets by the kind of value they show or edit: one number, one choice, two numbers, an action, module state, a signal, a shape, a pattern, a buffer. Each group has one set of gestures, one keyboard model, and one way to show modulation. Structured module data comes in a few shared kinds (curve, pattern, buffer), and each kind has exactly one editor widget.

## Decision (proposed)

Option 3. **A widget is chosen by the kind of value it edits, not by the module that uses it.**

- Widgets are grouped by kind of value. Widgets in a group share their gestures, keyboard use, and modulation display.
- Structured module data comes in shared **data kinds**. A module declares a data slot of a kind; it does not invent its own format. Each data kind has exactly one editor widget.
- A new widget or data kind is added only when a catalogue entry cannot be served by the existing ones, and it joins a group or starts a new one. It is planned in the panel widgets specification before it is built.

## Consequences

- **One way per interaction.** A user who has drawn one curve can draw every curve. A user who has opened one select list can open every one. Product principle 11 (learning) and NFR-UX-01 (keyboard) are met once per group, not once per module.
- **Data kinds shape module data.** [#50](https://github.com/brylie/LoomDSP/issues/50) (OQ-17) designs storage and transfer for three known kinds with fixed capacities, rather than an open format. Copying a curve from one module to another becomes possible, because both hold the same kind.
- **Some modules give up a special view.** A module that would look best with its own drawing gets the nearest shared widget instead, or waits until a second module needs the same view. The specification lists views left out on purpose, such as a piano roll.
- **More planning before code.** Each new widget needs its section of the panel widgets specification agreed first. This is the spec-first rule applied to widgets.
- **Shapes from parameters.** A module whose shape comes from its knobs, such as an ADSR, gets a read-only curve through a pure function in its descriptor, run on the UI side. This keeps module-specific drawing code out of the canvas (ADR-0013) and off the audio thread (ADR-0001).
- **Third-party modules** (FR-SDK) use the same set and the same data kinds. A module that needs something new proposes a widget for the set, as ADR-0013 already says.
