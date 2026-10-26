# Control Ranges Specification

A choice of ranges for wide controls, such as Multiply's gain, so a player can make a knob fine and safe for the job it does in a patch.

- **Kind:** living specification.
- **Status:** Agreed ([#122](https://github.com/brylie/LoomDSP/issues/122)). Not built.
- **Decision behind it:** none yet. It extends [parameter mapping](modulation.md#parameter-mapping) (Agreed) and does not change a decision.
- **Use case and requirements:** FR-MOD-09 (proposed here), FR-MOD-03, FR-MOD-08, FR-SIG-03.
- **Background:** [Why feedback FM turns to noise](../learn/feedback-fm.md): exponential pitch, the one-sample loop, and aliasing at high depth.
- **Issues:** [#121](https://github.com/brylie/LoomDSP/issues/121) (the problem that started this), [#122](https://github.com/brylie/LoomDSP/issues/122) (this spec).
- **Code:** not built.

## Goal

Add, Multiply, and Constant are general math modules. Their parameters go from -10 to 10, because a cable may carry anything from a tiny pitch offset to a full "10 V" signal (ADR-0007). One knob over that whole span is too coarse for most jobs.

In `examples/patches/fm-feedback.ron`, the Multiply gain sets the feedback FM depth. The musical zone is about 0 to 0.1. The knob goes to 10, which is 100 octaves of pitch swing, and a small turn makes harsh noise ([#121](https://github.com/brylie/LoomDSP/issues/121)). The player wants to say "this knob goes from -0.1 to 0.1 here", and then turn it freely.

Eurorack modules do this with a range switch (for example an offset generator with a 5 V and a 10 V setting). VCV Rack and Bitwig Grid modules often let you choose an output or value range the same way.

## Behaviour

- A parameter that offers ranges shows a **Range** selector next to its slider in the inspector. The tile's knob has no selector; it uses the selected range.
- The choices are a fixed list for each parameter. For Add `offset`, Multiply `gain`, and Constant `value`:

| Name | Value at the knob's minimum | At its maximum |
|---|---|---|
| -0.1 to 0.1 | -0.1 | 0.1 |
| -1 to 1 | -1 | 1 |
| 0 to 1 | 0 | 1 |
| -5 to 5 | -5 | 5 |
| 0 to 5 | 0 | 5 |
| -10 to 10 | -10 | 10 |
| 0 to 10 | 0 | 10 |

- The knob, the inspector slider, keyboard steps (1% and 0.1% of the range), and double-click reset all work inside the selected range. A full knob turn covers the selected range, so -0.1 to 0.1 is 100 times finer than -10 to 10.
- Each module instance keeps its own choice. Two Multiply modules can have different ranges.
- The choice is saved in the patch file, next to the parameter values:

```ron
PatchModule(id: 5, kind: "loom.math.mul@1", params: {"gain": 0.08}, ranges: {"gain": (-0.1, 0.1)}),
```

- To move a signal further, the player combines modules as on hardware: a Multiply to scale it, then an Add to offset it.

## Rules

1. **A range limits the control, not the signal.** The selected range clamps the parameter value only. A signal arriving on a cable is never clamped by it. *Why:* cables are polymorphic and unclipped (ADR-0007, FR-SIG-03). Protection against hot signals belongs to the modules that need it and to the final output ([#121](https://github.com/brylie/LoomDSP/issues/121)), not to a knob setting.
2. **Ranges are declared by the module author.** `ParamOptions`, in each `ParamSpec`, gets an optional list of ranges and a default choice. Every range lies inside the parameter's `min` and `max`, which stay the outer limits the engine enforces. *Why:* the author knows which spans make sense. A fixed list keeps the inspector simple and keeps patches portable.
3. **Only linear, not whole-number parameters can offer ranges.** The registry refuses others. *Why:* a logarithmic range needs a minimum above 0, and whole-number parameters already step one value at a time. Neither need has come up yet.
4. **The selected range replaces `min` and `max` in the mapping.** `normalize` and `denormalize` take the selected range, so the inspector, the tile knob, and later the engine's modulation all agree. *Why:* rule 3 of [Modulation](modulation.md#rules): every user of a control maps through the same functions. Modulation depth then means "a share of the selected range" (FR-MOD-08), which is the point: depth 0.1 on a -0.1 to 0.1 gain is a small, musical move.
5. **Modulated values clamp to the selected range** (FR-MOD-03). *Why:* the player chose that range to be safe. Modulation must not leave it.
6. **Switching to a smaller range clamps the value into it. Switching to a wider range keeps the value.** *Why:* the knob always shows the real value, as the modulation arc does. A player who picks a smaller range is asking for safety, so the value moves into it. A wider range never moves the value, so switching up is never a jump in sound. Rescaling the value to keep the knob position (the other option) would make -0.1 to 0.1 to -10 to 10 a 100 times jump in one click.
7. **Ranges never change a saved value on load.** This covers every value the loader accepts today: finite and inside the parameter's outer `min` and `max`. Other values are handled as now, by `ParamSpec::clamp` (out of range is clamped, NaN becomes the default). Ranges add no new change. A module saved without a range for a parameter gets the default range if its value fits. Otherwise it gets the smallest range in the list that holds the value. A saved range that is not in the list, or that does not hold the saved value (for example gain 5 saved with -0.1 to 0.1), is treated the same way: the smallest range in the list that holds the value is selected, and the loader reports it. *Why:* patches made before ranges existed, or with a later build, must sound exactly as saved.
8. **The range goes to the audio thread only as prepared data.** It lives in the graph and the patch. Once built, and before modulation exists, the controller clamps values to it before sending them, and the engine keeps clamping to the outer `min` and `max`. When modulation is built, the engine needs the selected bounds for rules 4 and 5, because it maps and clamps modulated values per sample. The compiler then puts each modulated parameter's bounds into the schedule, as it does for other prepared data, so the audio thread only reads them. *Why:* the engine must enforce the range that modulation uses, and preparing it on the control thread keeps the audio thread free of allocation and locks.
9. **Range names come from their bounds.** The inspector shows "-0.1 to 0.1", never a word like "fine". *Why:* plain numbers need no translation and say exactly what the knob does.

## Acceptance criteria

- [ ] The registry refuses a range outside the parameter's `min` and `max`, a range with `min >= max`, a default range not in the list, a default value outside the default range, and ranges on a logarithmic or whole-number parameter.
- [ ] Add `offset`, Multiply `gain`, and Constant `value` offer the seven ranges in the table, with the default range from Open points (-1 to 1).
- [ ] `normalize` and `denormalize` with a selected range map its bounds to positions 0 and 1, and clamp out-of-range values and NaN as they do today.
- [ ] Selecting a smaller range clamps the value into it. Selecting a wider range leaves the value unchanged.
- [ ] The tile knob and the inspector slider span the selected range. Keyboard steps are 1% and 0.1% of it.
- [ ] A patch saves and reloads the selected ranges.
- [ ] Every example patch loads with the same values as before. A value outside the default range (for example a gain of 5) loads with the smallest range that holds it.
- [ ] A saved range that does not hold the saved value (gain 5 with -0.1 to 0.1) loads with the value unchanged, the smallest range that holds it (-5 to 5), and a report.
- [ ] Two instances of one module keep separate ranges.
- [ ] Manual: in `fm-feedback.ron`, with Multiply's range at -0.1 to 0.1, a full knob turn moves from a pure tone to a bright one without the harsh noise from #121.

## Out of scope

- Ranges the player types in. A fixed list first; a custom range can come later if the list proves too small.
- A range selector on the tile. The inspector is enough for now; a panel selector widget is part of FR-PANEL-02 (P2).
- Ranges on logarithmic or whole-number parameters (rule 3).
- Clamping or limiting signals on cables, and clipping at the output. Those are separate fixes, tracked in [#121](https://github.com/brylie/LoomDSP/issues/121).
- Exposing a parameter's range from a sub-patch (FR-ENCAP-03, P2).

## Open points

- **Default range: -1 to 1** for all three parameters (decided in [#122](https://github.com/brylie/LoomDSP/issues/122)). It is safe and fits pitch and audio. Multiply's default gain of 1 sits at the top of the knob, which is the natural place for "unity". *Why not -10 to 10:* it keeps today's feel but is not safe by default, which is the reason for this spec.
- **Rule 6 is confirmed:** a smaller range clamps the value, a wider range keeps it.
- **Clamp and Scale** ([P1 modules](p1-modules.md)) offer the same list for their limits and offset.
- **Ranges from what is connected.** Could a range follow the signals around the module? [Port ranges](port-ranges.md) (Draft) proposes that ports declare nominal ranges and that the inspector offers a one-click **Match** for the Range selector. It never changes a range or value on its own, because that would break rules 6 and 7 here. Nothing in this spec waits for it.
- **Other parameters.** Phase `tune` and `octave` are already in musical units and probably need no ranges. Check each new module as it comes.
- **Explainer.** Written: [Why feedback FM turns to noise](../learn/feedback-fm.md).
