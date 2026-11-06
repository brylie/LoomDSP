# Patch Output

<!-- Generated from the module's descriptor by `loom modules --markdown`. Edit the descriptor, not this part. -->

An output port of a sub-patch. Inside a sub-patch, the cable into it continues out of the port; at the top level it does nothing.

| | |
|---|---|
| ID | `loom.struct.output` |
| Version | 1 |
| Family | Interface |
| Category | Structure |

## Inputs

| Port | Signal | Unconnected | Description |
|---|---|---|---|
| **In** (`in`) | audio | 0 | The signal that leaves through the sub-patch's output port. |

## Outputs

None.

## Parameters

None.

<!-- Hand-written notes go below this line. `loom modules --markdown` keeps them. -->

## When to use it

Put a Patch Output inside a sub-patch where a signal should leave. Its label is the name of the port on the sub-patch's tile. The cable into its `in` continues out of the port; when the patch plays, that cable runs straight to the outside destination, so the port costs nothing (`docs/design/sub-patches.md`, Flattening).

At the top level of a patch, such as a library file opened on its own, it is an ordinary module that reads its input and does nothing. Making a sub-patch on the canvas arrives with the editor (#51, slice 2).

## Example patch

A sine into a Patch Output at the top level: nothing is heard, as a library file sounds when opened on its own.

```ron
Patch(
    version: 1,
    metadata: Metadata(name: "Patch Output"),
    modules: [
        PatchModule(id: 1, kind: "loom.osc.sine@1"),
        PatchModule(id: 2, kind: "loom.struct.output@1", label: Some("out")),
    ],
    cables: [
        PatchCable(from: (1, "out"), to: (2, "in")),
    ],
)
```
