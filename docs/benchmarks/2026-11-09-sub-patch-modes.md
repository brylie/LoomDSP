# Sub-patch modes: what a select of 8 members costs

On 2026-11-09 the `modes` benchmarks ran on one macOS arm64 machine, for slice 6 of [#51](https://github.com/brylie/LoomDSP/issues/51) ([sub-patches](../design/sub-patches.md), Open points: CPU in select mode). Each used a 128-frame block, with criterion's default warmup and measurement.

Each voice is a phase generator into a saw into an SVF. The patches are one voice as a single sub-patch, and eight voices as a select, a layer, and a stack, each into the audio output. Nothing is modulated and the select does not change member.

| Patch | Mean per block |
|---|---:|
| One voice (single) | 4.15 µs |
| Select of 8 members | 19.86 µs |
| Layer of 8 members | 20.90 µs |
| Stack of 8 copies | 20.90 µs |

A select of 8 costs about what a layer of 8 does, as it should: every member runs (sub-patches.md, Modes). Each voice past the first adds about 2.2 µs; the rest of the single patch's 4.15 µs is the engine's own work, such as output safety and the Transport. The Select modules cost no more than the Sums: both read 16 inputs per sample.

At 48 kHz a 128-frame block has 2.67 ms, so a select of 8 such voices uses under 1% of one core. Pausing unselected members is not needed for now. If a heavier abstraction needs it, the spec's leaning stands: pause a member that has been silent and unselected for a fixed number of samples, never per block.

The numbers come from one laptop that was doing other work, so treat them as an estimate. Run it with `cargo bench -p loom-modules --bench engine -- modes`.
