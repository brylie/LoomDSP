# ADR-0030: Panels hide the controls a mode does not use

- **Status:** Accepted
- **Date:** 2026-11-01
- **Related requirements:** FR-PANEL-01, FR-PANEL-09, NFR-UX-01
- **Related decisions:** ADR-0013 (modules carry their own panel), ADR-0025 (one design language, and panels derived from the declaration). This ADR changes one point of ADR-0025, below.
- **Specification:** [Panels, Panel layout](../design/panels.md#panel-layout)

## Context

Some controls do nothing in some modes. In AD mode, an envelope's Sustain and Release are ignored. A free-running LFO ignores Cycles, and a synced one ignores Rate. Today these controls look as live as any other, so a performer can turn a knob and hear nothing.

ADR-0025 says a derived panel "is a pure function of the descriptor, computed once when the module type is registered", so the same module always looks the same.

## Options considered

1. **Dim in place.** Ignored controls stay where they are, drawn faded. The layout never moves, and the panel stays a function of the descriptor alone. The tile keeps controls that do nothing.
2. **Hide.** Ignored controls are not shown. The tile shows only what matters in the current mode, and can get shorter. The panel now depends on the values of its mode parameters, and parts of the tile move when the mode changes.
3. **Leave as is.** Simple, and misleading.

## Decision

Option 2, with limits that keep the tile steady.

- **A parameter can declare when it is shown,** as a condition on one choice parameter of the same module: `ParamOptions::shown_when(param, choices)`. Its widget is hidden while the condition is false. Only a choice parameter can hide controls, so the layout changes only when a mode is picked, never while a knob turns.
- **The tile keeps its width.** A hidden widget's row closes up, and the widgets left in the row are centred again.
- **The tile gets shorter only when whole rows empty.** Rows that only hidden widgets used are removed. Ports sit in the top rows of the tile's side columns, so they never move; only the tile's bottom edge does.
- **Nothing becomes unreachable.** The inspector still lists every parameter, and marks a hidden one "Not used in" the current choice. MIDI mappings and modulation assignments on a hidden parameter keep working. Tab skips hidden controls.
- **Change to ADR-0025.** A panel is a pure function of the descriptor **and the values of its mode parameters**. It is rebuilt when one of those values changes, which is a mode click, not a frame or a knob turn. Everything else in ADR-0025 stands.

## Consequences

- A performer sees only the controls that do something. Envelope in AD mode shows Attack, Decay, and Curve.
- A mode change moves the tile's bottom edge and re-centres a row. Ports, cables, and the tile's width stay put.
- The app rebuilds a module's scene when one of its mode parameters changes, so modules with modes cost one scene rebuild per mode change.
- Module authors mark which parameters a mode ignores. The registry refuses a condition that names a missing or non-choice parameter.
