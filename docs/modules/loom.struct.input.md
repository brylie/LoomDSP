# Patch Input

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

An input port of a sub-patch. Inside a sub-patch, cables into the port continue from here; at the top level it outputs silence.

| | |
|---|---|
| ID | `loom.struct.input` |
| Version | 1 |
| Family | Interface |
| Category | Structure |

## Inputs

None.

## Outputs

| Port | Signal | Polarity | Description |
|---|---|---|---|
| **Out** (`out`) | audio | bipolar | The signal that arrives at the sub-patch's input port. |

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Put a Patch Input inside a sub-patch where a signal from outside should arrive. Its label is the name of the port on the sub-patch's tile. Cables from its `out` reach the modules inside; when the patch plays, those cables run straight from the outside source, so the port costs nothing (`docs/design/sub-patches.md`, Flattening).

At the top level of a patch, such as a library file opened on its own, it is an ordinary module that outputs silence. Making a sub-patch on the canvas arrives with the editor (#51, slice 2).

## Example patch

A Patch Input at the top level feeding the output: silence, as a library file sounds when opened on its own.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Patch Input"),
    modules: [
        PatchModule(id: 1, kind: "loom.struct.input@1", label: Some("in")),
        PatchModule(id: 2, kind: "loom.io.audio-out@1"),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
    ],
)
```
