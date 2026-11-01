# ADR-0034: The controller keeps a flat API over typed internal parts

- **Status:** Accepted
- **Date:** 2026-11-01
- **Related requirements:** none
- **Related decisions:** ADR-0001 (two threads, two halves), ADR-0002 (graph execution)
- **Issue:** [#472](https://github.com/brylie/LoomDSP/issues/472)

## Context

`loom_core::Controller` edits the graph on the control thread and sends commands to the engine. It has about 70 public methods: modules, cables, modulation assignments, parameters, MIDI mappings, transport, display values and probes, and audio output. All of its state sat in one struct, so any method could change any bookkeeping. After 0.1.0 people will depend on this API, so its shape should be settled first.

Most methods share three things: the editable `Graph`, the "uncommitted changes" flag, and the command queue to the engine. Other state belongs to one area, with two links between areas: removing a module removes its probes, and a screen edit on a parameter mapped to MIDI takes an edit token, so it is ordered against control changes.

## Options considered

1. **Flat API, grouped files.** One list of methods, split into one `impl Controller` block per area. Clear to read, but every method can still change every field.
2. **Facade views,** such as `controller.midi().map(..)`, each a small type that borrows the whole controller. Groups the calls, but a view that borrows the whole controller blocks every other call while it lives. A view for modules would still need assignments, probes, and mappings to remove a module, so the views add names, not boundaries. Every caller would change.
3. **Public parts,** such as `controller.midi.map(..)`. Each part needs the graph and the command queue, so either its methods take them as arguments, and the internals become public API, or its fields become public, and its rules with them.
4. **Flat API over typed internal parts.** Option 1 for callers. Inside, state that belongs to one area moves into a private type with its own methods, and only that type touches its fields.

## Decision

Option 4.

- **The public API stays one flat list,** grouped by area. Each group is an `impl Controller` block in a child module of `controller` (`modules.rs`, `cables.rs`, `assignments.rs`, `params.rs`, `midi.rs`, `transport.rs`, `displays.rs`, `audio.rs`), and the API docs show each block as a section.
- **State with its own rules is a typed part:** `Slots` (free, new, and retiring engine slots), `MidiSync` (mapping ids, and the order of screen edits and control changes), `Probes`, and `LiveValues` (display values and live modulated values). Their fields are private. The rest of the controller uses their methods.
- **`Controller` methods coordinate.** They borrow parts and the graph as separate fields (`self.midi.sync(&mut self.graph)`), which Rust allows inside one method. A part does not hold the graph or the command queue. It takes the graph as an argument, or returns a value for the controller to send.
- **Shared helpers stay in `controller.rs`.** A child module cannot see a sibling's private items, so a helper that two groups use, such as `send` or `check_param`, lives in the parent.

## Consequences

- **Callers do not change.** The app, the command-line tool, and the tests call the same methods.
- **Each rule has one owner.** For example, only `MidiSync` changes the edit tokens that order screen edits against MIDI, and only `Slots` decides when a slot is free again. Each part has unit tests that need no engine.
- **Links between areas are method calls,** such as `self.probes.remove_on(id)` in `remove_module`, rather than writes to another area's fields.
- **The pattern scales to other large types.** The app's state (#466) can keep one `update` entry point that coordinates typed parts in the same way.
- **The graph and the command queue stay shared.** That is by design: every edit changes the graph and most send a command. A new area adds a group file, and a part only if it has state of its own.
