# ADR-0017: A module library of primitives, traits, and open abstractions

- **Status:** Accepted
- **Date:** 2026-10-28
- **Related requirements:** FR-LIB-12 to FR-LIB-15, FR-LIB-24, FR-ENCAP-08 to FR-ENCAP-10, FR-PANEL-11, FR-SDK-07, FR-SDK-08
- **Related decisions:** ADR-0011 (everything is a module), ADR-0012 (arrangement from phase-driven primitives), ADR-0013 (modules carry their own panel), ADR-0014 (modulation without cables)
- **Plan:** [Module catalogue](../product/module-catalogue.md). A design specification for roles and slots is written before P2 work on them starts.

## Context

ADR-0011 makes every feature a module. A survey of 15 modular environments ([prior-art survey](../research/2026-10-prior-art-survey.md)) found about 250 distinct kinds of module. Building each one as its own fixed module would make the library very large, which is risk R-11 ("the project spreads thin"). It would also make the library hard to learn, because many of those modules differ only in one option.

The survey also showed that much of the variety is not new DSP. It is the same few jobs, combined in different ways:

- A sample player, a multi-sample instrument, a slice player, and a granular player all read a buffer. They differ in how they choose what to read.
- Unison, stereo spread, and per-voice variation are the same idea: give each copy a different value. Several products build it into each oscillator separately.
- Many devices are fixed chains: oscillator, filter, envelope, amplifier. The more flexible products let the user swap one part of the chain (a different filter, a different oscillator) and keep the rest.

ADR-0012 already chose this path for arrangement: small phase-driven primitives, with groups and parts shipped as open abstractions made from them. ADR-0013 gave every module, sub-patch, and patch the same panel model. This ADR extends both to the whole library.

## Options considered

1. **One fixed module per familiar device.** A sampler module, a slicer module, a granular module, and so on. Quick for users who know the devices. But the library grows without limit, features are built many times, and nothing can be opened to learn from or changed.
2. **A few large modules with many modes.** One "sampler" that does everything, one "synth" with switchable sections. Fewer modules, but each one is a closed box with a long parameter list, and the modes cannot be combined in new ways.
3. **Primitives, traits, and open abstractions with swappable slots.** Small modules do one job each. Options that many modules share are defined once as traits. Familiar devices ship as open abstractions made from primitives, and the parts of an abstraction can be swapped through role-typed slots.

## Decision

Option 3. The module library has three layers.

1. **Primitives.** Modules with one job, such as a buffer reader, a curve reader, a phase warp, or a selector.
2. **Traits.** Shared parameter sets and behaviour, such as timebase, retrigger, seed, spread, and character, defined once in the module SDK and declared by any module that needs them (FR-SDK-07).
3. **Abstractions.** Open patches made from primitives that work as one module (FR-LIB-24). Their parts can be **slots**: places that accept anything filling a given **role**.

A **role** is a contract: port names and core parameter names that a module or sub-patch promises to have (FR-SDK-08). Swapping what fills a slot keeps the cables, because the ports match. Values carry across by role parameter name, and values the new part does not have stay stored, so swapping back restores them (FR-ENCAP-08). A panel widget can bind to a slot's role parameter, so a Cutoff knob keeps working after the filter changes (FR-PANEL-11). Any sub-patch, including a user's own, can declare the roles it fills (FR-ENCAP-09).

A swap is an ordinary graph edit: prepared on the control thread, applied at a buffer boundary, and crossfaded (FR-GRAPH-02, FR-GRAPH-05). It is safe during performance.

**The merge rule.** When two candidate module types look alike:

- **Make them one type with a setting** when they have the same ports, the same kind of stored data, and a similar tile.
- **Keep them as separate types** when their ports differ in kind, their tiles differ a lot, or a special-purpose version is much cheaper on the CPU.
- **Make an abstraction** when the familiar device is really a chain of primitives.

A type is not a module ID. One type can be built as more than one module when that is cheaper or clearer, and released module IDs never change.

## Consequences

- **A smaller library that does more.** The catalogue lists about 170 types instead of about 250, and abstractions cover the familiar devices without new DSP.
- **One way to learn.** Users learn each trait and each role once. Opening an abstraction shows how it works, which supports product principle 11 (every decision teaches).
- **The community extends abstractions without code.** A user's sub-patch that declares a role can fill any matching slot, in any abstraction.
- **The SDK grows.** The module descriptor needs a way to declare traits and roles, and the registry must check that a module really has the ports and parameters its roles promise. How roles are named and versioned is open (OQ-20). What happens to stored data, such as a pattern or a buffer, on a swap is open (OQ-21).
- **Panels need one new kind of binding,** to a slot's role parameter instead of a fixed module parameter. Parameters only one part has appear in a sub-panel that changes with the slot (FR-PANEL-08).
- **Some speed is traded for openness.** An abstraction can cost more CPU than a hand-written module. Encapsulation has no runtime cost (FR-ENCAP-07), but a chain of primitives can still do more work than a fused version. Where it matters, a fused primitive can fill the same role later.
- **Existing modules stay.** `loom.osc.sine` and `loom.osc.saw` are both built forms of one Oscillator type. Nothing already released is renamed.

## Prior art

Bitwig Studio's Polymer and its Filter+ and Sweep devices let the user swap one section of a device for another Grid module. Reaktor Blocks and Max abstractions show open, reusable patches as the unit of reuse. VCV Rack's polyphonic cables show per-voice values travelling on one cable. The [prior-art survey](../research/2026-10-prior-art-survey.md) has the details and sources. We take the ideas, not the designs ([ADR-0018](0018-clean-room-module-development.md)).
