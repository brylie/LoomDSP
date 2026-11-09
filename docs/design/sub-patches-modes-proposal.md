# Proposal: sub-patch modes (slice 6 of #51)

Temporary file. It lists what the [Modes](sub-patches.md#modes) section leaves open, with a recommendation and the alternative for each point. Once the maintainer agrees, the text moves into `sub-patches.md` (and ADR-0040 for point B), and this file is deleted.

Numbering: A file shape, B the overlay for copies, C the added modules, D the editor, E how modes meet what exists, F smaller points.

## A. The file shape

**A1. `mode`.** `mode: Some(Layer)` on a `SubPatch`, one of `Single`, `Layer`, `Select`, `Stack`. Omitted when `Single`.

**A2. Members are nested instances.** In layer and select mode, the definition's own `modules` hold only its members, each an ordinary sub-patch instance (`loom.struct.subpatch@1` with its own definition key), and `members: [3, 1, 2]` lists their module ids in order. No cables, Patch inputs or outputs, assignments, or other modules at that level. A member's name is its definition's name.
- *Why:* a member is then a nested sub-patch in every way that exists today: flat ids by path, opening it, the breadcrumb, renaming, exposing its surface items on the layer's surface (exposed through levels, rule 12), slots inside it, Save to library on it, its own linked library file. Nothing new is needed to edit inside a member.
- *Alternative:* `members: ["saw", "square"]` as a list of definition keys, with no member modules. Shorter text, but every path that works on instances (ids, exposing, opening, undo) would need a second form for "a member that is not a module".
- A file that breaks the rule (another module, a cable, a member listed twice or not at all, a member that is not an instance) is refused, naming the sub-patch.

**A3. Stack fields.** `copies: 4` (omitted when 1), `spread: {"cutoff": 0.2, "detune": 0.5}` (exposed name to amount, omitted when empty), `stereo_spread: true` (omitted when false). In stack mode the definition's `modules` are its one inside, as for single.

**A4. The Member value** is on the instance, as an exposed value is: `params: {"member": 2.0}` on a select instance, 1-based. `member` is a reserved name on a select surface: an exposed item cannot take it (the next free name is used, "member 2"). An assignment or MIDI mapping on it is written on the instance, the form flattening rule 3 already reads.

## B. The overlay for copies (stack mode)

ADR-0039 makes the flat graph the only home of parameters, and one tile is one flat module. With N copies, one inner module is N flat modules. Recommendation: **copy 0 is the one the editor shows; copies 1 to N-1 and every added module are implied by the flattener, the way boundary cables are today.** This needs a new ADR-0040 that extends ADR-0039 (an accepted ADR is not rewritten).

**B1. Paths.** Copy 0 keeps the paths it has as a single sub-patch: `[instance, inner]`. Copy k (k >= 1) is `[instance, Copy(k), inner]`, where `Copy(k)` is a path step that can never be a module id (a reserved range of `u32`). So turning a sub-patch into a stack, or raising Copies, never moves copy 0, and copy k keeps its id and state while Copies stays at least k + 1. A removed copy's ids are not reused (rule 4).

**B2. What the editor reads.** Tiles, selection, the inspector, values, probes, lights, meters, and live values all read copy 0. The breadcrumb names the stack with its count, `Voice ×4`. The outer tile's outputs carry the sum. *Why copy 0:* it has offset -1 under spread, but the knob shows the base value (B4), which is the same in every copy, and its signals are one real copy's, not an average that no module plays. *Alternative:* probes on the sum. Rejected: an inner port has no sum.

**B3. How an edit reaches every copy.** Two paths, both before the change is heard:
- **A parameter write** (`App::set_param`, the one place every control, inspector row, reset, and control-channel `set` goes through) writes the same value to the same parameter of every copy, at once, with no commit. A knob drag inside a stack moves every copy during the drag.
- **Everything else** (a module, a cable, an assignment, a mapping, an exposure, Copies, spread) reaches the copies through `sync`, which already writes the hierarchy out, flattens it on a scratch table, and applies the difference in cables. It is extended to apply the difference for implied modules too: add or remove them, set their values, and set their cables, assignments, and mappings, all from the flattener's output. The app runs it before every commit when the patch has a stack. So one edit is one commit, one schedule swap, and one undo step.
- MIDI and modulation inside a stack reach every copy by themselves, because the flattener gives every copy its own mapping and assignment.

**B4. Spread is modulation the flattener adds.** For each spread parameter, the flattener adds one hidden Constant at 1.0 on the instance and, on copy k's parameter, an assignment from it with depth `amount × offset(k)`, where `offset(k) = -1 + 2k/(N-1)` (0 for one copy). So:
- the base value is the same in every copy and stays the one home of the exposed value (ADR-0039 invariant 6 holds);
- the engine adds the offset in the control's 0 to 1 range and clamps it (modulation rules 2 and 5), so the amount is a share of the knob, like a depth: 0.1 on cutoff is a musical interval, on a whole-number parameter it rounds after the clamp;
- a change of the value, from a knob, MIDI, or modulation, keeps the offsets, because they are added to whatever the base is;
- a change of the amount or of Copies changes only depths, which take effect at the next buffer (modulation, depth changes);
- these assignments are hidden: not in the matrix, the inspector's modulation rows, the range arc, or the file.
- *Why not write `value + offset` into each copy:* a MIDI control change is applied on the audio thread to each copy's parameter, and would wipe the offsets; the flat graph would also hold no single base value to save. *Alternative:* spread in parameter units (Hz, cents), written per copy, with MIDI clamped in the engine per copy. More engine work, and MIDI would still need a per-copy offset there.

**B5. Undo.** A snapshot leaves implied modules out (as it leaves fades out), and `sync` rebuilds them after a restore. One edit, one step, at every copy.

**B6. Seeded modules differ per copy.** Copy k of a module with a `seed` parameter gets `seed + k`, wrapped in the parameter's range, from the flattener and from the parameter write fan-out. *Why:* the stereo rule (stereo.md rule 15): copies that play the same random numbers are one sound in disguise. *Alternative:* identical seeds; the player spreads an exposed seed by hand.

## C. The modules the flattener adds

All are registered as internal (not in the browser or help pages, checked by every all-modules test), have no panel, hold no buffer, allocate nothing, count per sample, and give the same output at any block size. Each has an id from a path `[instance, Added(n)]`, where `Added(n)` is another reserved step, numbered by the outer port's place in the port list, so ids are deterministic (rule 6). They are not shown and not saved, and their ids are implied (B3).

| Module | Inputs | Output | Parameters | For |
|---|---|---|---|---|
| `loom.struct.sum@1` (Sum) | `in 1` to `in 16` | `out` | `gain` | layer (gain 1) and a mono stack (gain 1/√N) |
| `loom.struct.unison_pan@1` (Stereo spread sum) | `in 1` to `in 16` | `out` (stereo) | `copies` | a stack with stereo spread |
| `loom.struct.member@1` (Member) | | `out` | `member` (1 to 16, whole number) | the select instance's Member control |
| `loom.struct.select@1` (Select) | `in 1` to `in 16`, `member` | `out` | `fade` (on/off) | each output of a select instance |

**C1. 16 is the limit** for members, as for copies and voices (polyphony rule 1): a fixed number of ports. Adding a 17th member is refused with `A layer holds at most 16 members`.

**C2. Sum and stack gain.** Out = gain × Σ inputs; gain is smoothed over 10 ms so a Copies change does not step the level (a copy added or removed fades in or out over 5 ms already, live-patching rule 9). A stacked output is scaled by 1/√N because N copies that differ (detuned, different seeds) are uncorrelated, and the power of a sum of uncorrelated signals is the sum of their powers, so 1/√N keeps the loudness of one copy. Copies with no spread are identical and sum coherently, √N louder than one; the explainer says why spread matters. Sources: A. Papoulis, S. U. Pillai, *Probability, Random Variables and Stochastic Processes*, 4th ed., McGraw-Hill, 2002 (variance of a sum of independent variables); C. Roads, *The Computer Music Tutorial*, MIT Press, 1996 (chorus and unison).

**C3. Stereo spread.** Copy k is panned by offset(k): `θ = (offset + 1)·π/4`, left `cos θ`, right `sin θ`, both times 1/√N. Equal power, so each copy keeps its loudness wherever it sits, and the centre is -3 dB on each side (the constant-power pan law; U. Zölzer (ed.), *DAFX*, 2nd ed., Wiley, 2011, spatial effects; Roads, panning). One copy with stereo spread sits in the centre. A stereo copy output is folded to mono first (its inputs are `Downmix`, ADR-0020), so the pan is exact. The output port is `Stereo`, so the stack's outer output is stereo (ADR-0020).
- *Alternative:* a linear pan law (centre -6 dB). Rejected: it dips in the middle for uncorrelated copies.

**C4. Select.** One Member module per select instance owns the Member parameter, the one home of the value (ADR-0039): it is modulatable and MIDI-mappable through the instance, and outputs it per sample. Every Select module reads it on its `member` input. Per sample, each Select takes `t = round(member)` clamped to 1..count, and runs the same state machine, so every output switches on the same frame:
- **Not fading, `t` differs from the current member:** start a fade from the current to `t`: `out = from·cos θ + to·sin θ`, `θ = (π/2)·n/N`, `N = round(0.010 × sample rate)`, one step per sample. After N samples it outputs exactly `to`.
- **Member changes back during a fade** (to `from`): the fade reverses from where it is (`from` and `to` swap, `n` becomes `N - n`). The gains are the same on that frame, so there is no step.
- **Member changes to a third member during a fade:** it waits; the fade ends, then a new one starts from there to the latest Member. Only the latest is kept, so a fast sweep ends on where the control stopped, never more than 10 ms later.
- *Why not finish at once* (as a second slot swap does): a slot swap is a gesture, but Member can be modulated at audio rate, and a jump would click every time.
- *Alternative:* a fractional Member morphs between two members (1.5 is an equal-power mix of 1 and 2). That is a different feature, a crossfader; the spec says Member is stepped.
- **Gate, trigger, and phase outputs** (the signal type of what feeds the member's Patch output) get `fade` off: they switch to `t` on the frame it changes, with no fade and no wait (live-patching rule 9, as a swap's gate outputs do).
- **A member without the output** feeds nothing into that Select input, which reads 0, so choosing it fades that output to silence.

**C5. Layer and select inputs.** An outer input feeds the same-named Patch input of every member that has it. No module is added: the flattener fans the cable out, as it does into a stack's copies.

## D. The editor

**D1. Mode** is a choice row in the sub-patch tile's inspector: Single, Layer, Select, Stack. Changing it is one undo step and **never changes the sound by itself**:
- **Single to layer or select:** the inside becomes member 1, named after the sub-patch. Its modules keep their flat ids and state (their paths move under the new member, as a collapse moves paths), its Patch inputs and outputs keep their names, and each exposed item is re-exposed through the member so the surface keeps its names and values. Select starts at Member 1.
- **Single to stack:** Copies starts at 1, so nothing changes until the player raises it. *Alternative:* start at 4, which sounds at once but is a change the player did not ask for.
- **Layer to select and back:** members kept.
- **Layer or select to single or stack:** only with one member left, which moves back up as the inside. With more, refused with `Remove members until one is left to make Voice single` (`stack` in place of `single`). *Alternative:* keep the first member and drop the rest. Rejected: it throws work away, and undo would be the only way back.
- **Stack to single:** copies 1 to N-1 go, copy 0 keeps its state, and Copies, spread, and stereo spread are cleared.

**D2. Members (layer and select).** The inspector lists the members by name, with **Open**, **Up**, **Down**, and **Remove** on each row, and **Add member** under them, which adds an independent copy of the last member, named as a duplicate is ("Saw 2"). Removing the last member is refused with `A layer needs at least one member`. Reordering changes the Member numbers. Renaming is the member tile's own inspector, as for any sub-patch. Each step is one undo step.

**D3. Inside a layer or select.** Opening it shows its members as tiles in a row, with no cables and no port tiles; the breadcrumb reads `Patch › Layer › Saw`. Opening a member shows its inside. At the member level, inserting a library item or pasting sub-patch tiles adds members; deleting a member tile removes it. Any other module, a cable, or a port is refused with `A layer holds only its members; open a member to add modules` (`select` for a select). *Why:* the level has no ports of its own; its ports come from the members.

**D4. Ports of a layer or select instance** are the union of its members' ports by name, in the order they first appear (member order, then each member's port order). A member without a port is skipped (inputs) or adds nothing (outputs), as the table says.

**D5. Stack.** The inspector shows **Copies** (a whole number, 1 to 16, one undo step per change; not modulatable), **Stereo spread** (a toggle), and **Spread**: one row per exposed item with an amount from 0 to 1 (a share of the knob). A row at 0 is not spread and is left out of the file. *Why no separate pick list:* the amount says it, and an exposed item is what the player already chose to show. Raising Copies keeps the state of existing copies; new copies start from reset at copy 0's values (live-patching rule 4; polyphony rule 14 says the same for voices). Opening a stack shows copy 0's inside, as for single.

**D6. Select's Member control** is the first item on the select instance's surface, a stepped control labelled `Member`, spanning 1 to the member count (an exposed range, so MIDI sweeps exactly the members). Its inspector row says the chosen member's name, `Member: 2 (Square)`. Removing members clamps the value, as a narrower range does.

**D7. Dissolve** of a layer, select, or stack instance is refused with `Make Voice single before dissolving it`. Collapse at a member level is refused with the D3 text.

## E. How modes meet what exists

Each refusal below is a follow-up, not a gap.

- **E1. Linked library items.** Changing mode, members, Copies, spread, or stereo spread is an edit inside, so the first one unlinks a linked tile (the existing rule, no new code). **Save to library** of a non-single sub-patch is refused with `Cannot save a layer to the library yet` (`select`, `stack`): a patch file has no place for a mode yet. A member's own Save to library works, since a member is a single sub-patch.
- **E2. Roles and slots.** A non-single sub-patch fills no role: turning on Fills role on one is refused with `A layer cannot fill a role yet`, and changing the mode of one that fills a role is refused with `Voice fills Filter; turn that off before changing its mode`. That also covers a slot's part, which always fills its role.
- **E3. Slots inside.** Copies above 1 on a sub-patch that holds a slot at any depth is refused with `Cannot stack Voice: it holds a slot`, and Make slot inside a stack of more than one copy with `Cannot make a slot inside a stack yet`. *Why:* a swap's fade modules are added by the app for one part, and copies would each need their own. Changing the mode of a sub-patch that holds a slot is refused with `Cannot change the mode of a sub-patch that holds a slot yet; dissolve the slot first` (as collapse is). A slot inside a member works, since a member is a nested sub-patch.
- **E4. Clipboard.** A sub-patch tile copies whole, mode and members included. A copy inside a stack copies copy 0's modules. At a member level, a paste of sub-patch tiles adds members, and a paste with anything else is refused with the D3 text.
- **E5. Exposed parameters through members.** The layer's surface exposes a member's surface item with the existing Expose button ("Expose on Layer"). One surface knob for the same parameter in every member is not built (a follow-up: a macro across members).
- **E6. Nesting.** A stack inside a stack runs copies times copies; a layer inside a member works. No limit beyond CPU, which the inspector does not show yet.

## F. Smaller points

- **F1. CPU in select mode.** Measured with a new bench, a select of 8 members, recorded under Open points. No pausing.
- **F2. Explainer.** A new `docs/learn/unison.md`: why a stack sums at 1/√N, coherent and incoherent sums, the constant-power pan law, and why select fades and how a reversed fade stays continuous.
- **F3. Closing #51.** This slice ticks the last criteria of the umbrella. The PR says `Closes #51` if the maintainer agrees, with the refusals above filed as follow-ups.
