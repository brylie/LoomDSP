# ADR-0025: One design language, and panels derived from the declaration

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-21
- **Related requirements:** FR-PANEL-01, FR-PANEL-03, FR-PANEL-06, FR-PANEL-07, FR-PANEL-09, FR-ENCAP-03, FR-SDK-01, NFR-UX-01, NFR-UX-02
- **Related decisions:** ADR-0013 (modules carry their own panel), ADR-0016 (ports and parameters carry their own text), ADR-0017 (primitives, traits, and abstractions), ADR-0024 (widgets follow the kind of value)
- **Specification:** [Panels, Default panels](../design/panels.md#default-panels), [Panels, Exposed controls](../design/panels.md#exposed-controls-in-a-sub-patch), [Panel widgets](../design/panel-widgets.md)

## Context

In many modular environments, each module maker draws their own panel. Every module looks different, and the same idea (a knob, a range switch, an envelope) works differently on each. Users must study each module before they can play it. Some of that is part of the fun, but on stage it costs time and confidence.

LoomDSP already took one stance on clutter: cables run behind the tiles, dimmed, and come forward only when the user points at them. The controls stay visible. This ADR takes the matching stance on the controls themselves.

ADR-0013 says panels are data, and that a module "picks from the widget set". ADR-0024 says which widgets the set holds. Three questions are still open:

- **How much can a panel's look vary?** FR-PANEL-06 asks for "widget styles", and FR-PANEL-07 for custom images "as backgrounds or widget skins". Both would let each module or patch look and feel different again.
- **Must every module author lay out a panel by hand?** Today each built-in module lists its widgets and their grid positions. Most of that list follows from the parameters: P1 panels are "a knob for every parameter", with a few left in the inspector.
- **What does a sub-patch's surface look like before anyone designs it,** and how does a control on the surface relate to the control inside?

## Options considered

1. **Free drawing.** Each module, or each panel, draws itself, for example from an SVG layout with custom images. Total freedom of look. But every panel becomes its own design system. Gestures, keyboard use, modulation display, and screen reader text must be built again each time, and they will differ. ADR-0013 already turned down module drawing code for the same reasons.
2. **Shared widgets, every panel written by hand.** What P1 does. The look is consistent, but every module author must place every widget, and a sub-patch shows nothing until someone designs it. Authors who skip the work get empty tiles.
3. **Shared widgets, a panel derived from the declaration, and overrides when wanted.** A module declares its parameters, display values, and data slots. The panel follows from them by fixed rules: a continuous parameter becomes a knob, a two-state one a toggle, a short named list radio buttons, and so on. An author can give a parameter a hint (use a fader, keep it in the inspector) or write the whole panel. Every path ends in the same widgets.
4. **Derived panels only.** Simple and fully consistent. But a front panel built for performance needs a layout chosen by a person, not by rules.

## Decision

Option 3. **Every panel is built only from the shared widget set, and by default it is derived from what the module declares.**

- **One design language.** A widget looks and works the same on every module tile, sub-patch tile, and front panel. Panels choose widgets, positions, sizes, and text. They do not choose how a widget is drawn or how it responds. There are no per-module or per-patch skins, images, or widget styles, and no background images or logos on any panel, front panels included. Themes are global: they change colours, fonts, and contrast for the whole app, and every widget follows.
- **Derived by default.** A module that declares no panel gets one derived from its declaration, by rules in the specification. The derivation is a pure function of the descriptor, so the same module always gets the same panel.
- **Overrides in two steps.** A hint on one parameter changes its widget or keeps it off the panel ("inspector only"), and the rest is still derived. A hand-written panel replaces the derived one. Both are checked by the registry in the same way. A module's hand-written panel must still show every parameter that is not marked "inspector only", so leaving a parameter off a tile is always written down, never an accident.
- **Sub-patch surfaces are mapped widgets.** Any widget inside a sub-patch can be exposed on its surface: a control, a light, a meter, a scope, or a curve editor. The surface shows the same kind of widget, bound to the same inner parameter, display value, output, or data slot. A sub-patch with no designed panel gets one derived from what it exposes, by the same rules as a module.
- **A new look is a new widget.** When no widget fits, a widget is proposed for the shared set (ADR-0024). It is not drawn by one module.

## Consequences

- **Learn once, use everywhere.** A user who has learned the knob, the select list, or the curve editor on one module knows it on every module, inside and outside every sub-patch, and on every front panel. Entering a sub-patch never means learning a new design system. This is product principle 12.
- **Module authors write less.** A new module declares its parameters and gets a working, consistent, keyboard-reachable panel. Third-party modules (FR-SDK-01) get the same panel quality as built-in ones with no layout work.
- **Accessibility is built once.** Keyboard use, focus, screen reader text, and colour-independent states are properties of the widgets, so a derived panel has them all (NFR-UX-01, NFR-UX-02).
- **Contrast is checked once, per theme.** Text and widgets always sit on the theme's own background, never on an image, so a theme that meets WCAG 2.1 contrast (4.5:1 for text, 3:1 for widget parts) meets it on every panel. *Why:* a background image under text makes contrast depend on each picture, and nobody checks each picture. Stage light makes poor contrast worse.
- **Requirements change.** FR-PANEL-06 loses "widget styles". FR-PANEL-07 keeps vector rendering on high-DPI screens and loses custom images, as backgrounds and as widget skins. These requirement changes are made when this ADR is accepted, as for FR-PANEL-02 in ADR-0024.
- **`ParamSpec` grows.** Derivation needs choice names ([#160](https://github.com/brylie/LoomDSP/issues/160)) and a widget hint on the parameter. The hint is not a layout: it names a kind of widget from the parameter's group, or "inspector only".
- **Some looks are not possible.** A module cannot have a vintage faceplate, a photo-real knob, or its own gestures. That is the price of consistency, and it is chosen on purpose. A front panel stays clean and minimal: users who want it to look like their own get layout, labels, and the global theme.
- **The derived layout must be good.** If derived panels look poor, authors will write every panel by hand and the benefit is lost. The specification checks the rules against every built-in module.
