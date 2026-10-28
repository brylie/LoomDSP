# ADR-0015: One transport per patch, read by modules without cables

- **Status:** Accepted (2026-10-28)
- **Date:** 2026-10-15
- **Related requirements:** FR-LIB-07, FR-LIB-16, FR-LIB-20, FR-LIB-22, FR-LIB-25 to FR-LIB-27, FR-MIDI-07, FR-NATIVE-*
- **Related decisions:** ADR-0002 (graph execution), ADR-0007 (signal model), ADR-0011 (everything is a module), ADR-0012 (arrangement from phase-driven primitives)
- **Specification:** [Clock and transport](../design/clock.md)

## Context

Tempo-synced LFOs, sequencers, cycles, and the arrangement modules (ADR-0012) all need musical time: tempo, bar and beat position, and whether the music is playing. They must stay in step with each other, through tempo changes and patch edits.

Time can come from three places: the app's own tempo, a plugin host's transport, or external MIDI clock (FR-MIDI-07). Whatever the source, there must be exactly one, so every module agrees on where the bar starts.

If every synced module needs a cable from a clock module, a large patch gains dozens of clock cables that all say the same thing. Bitwig's Grid avoids this: a module's phase input follows the global transport until you patch something else into it.

## Options considered

1. **Clock as an ordinary module only.** Every synced module gets a cable from it. Explicit, but cluttered, and two clock modules could disagree.
2. **A global transport only, read inside modules.** No cables, but users cannot bend time: no swing, no polyrhythm by patching a different phase.
3. **A clock module as the source, as in VCV Rack.** A clock module on the canvas has run and reset on its panel and as inputs. Very direct, and it fits direct manipulation (ADR-0013). But on its own it still needs a cable to every synced module, and it has no rule for following a plugin host.
4. **One transport per patch, owned by the engine, with defaults and a Transport module.** The engine keeps the one transport. Modules read it through default inputs when nothing is patched. The transport's controls live on one Transport module, as in option 3, and Transport and Clock modules expose it as cables for anyone who wants to patch time.

## Decision

Option 4: the engine's single transport, controlled from a module as in VCV Rack.

- **The engine owns one transport per patch:** tempo, time signature, play state, and position in beats. Its source is the app, the plugin host, or MIDI clock.
- **Unconnected time inputs follow the transport.** A module declares a default for each phase or clock input, such as "bar phase" or "beat phase". When no cable is connected, the input reads that value from the transport.
- **Patching overrides the default.** A cable into the same input replaces the transport for that module. This is how users bend time.
- **Controls live on the Transport module.** Every patch has exactly one. Its panel has Run, Reset, and tempo, and its run and reset inputs let anything in the patch start and stop time. There are no transport controls in the toolbar.
- **Transport and Clock modules** put the transport on cables: bar and beat phase, play gate, triggers, and phases at musical rates. Clock Divide and related modules work on those signals.

## Consequences

- Synced modules work with no clock cables at all, and stay in step by default.
- The `ProcessContext` gains the transport state for each block. Default inputs are filled by the engine, so a module reads the same slice whether a cable is connected or not.
- Position is kept in double precision, so loops stay in step through hour-long sets.
- Sub-patches see the same transport. A sub-patch can still be fed a different phase through an input.
- Plugin export has a clear rule: the host's transport becomes the patch transport, and the Transport module's panel shows that it is following the host.
- Start and stop are patchable. A Button module, a MIDI note, or logic can run the clock, and the controls can go on a front panel.
