# Autosave copy

On 2026-11-18, LoomDSP averaged **0.196 to 0.209 ms per copy** across two runs building the recovery snapshot of a generated patch with 299 modules. This meets the 1 ms target for about 300 modules. [Autosave specification](../design/autosave.md#acceptance-criteria)

The release build ran on macOS 27.0.1, an Apple M1 Pro with 10 cores and 32 GB of memory. The host was also running development builds. The benchmark builds one snapshot to warm up, then reports the mean of 200 copies. Each sample includes creating and dropping the in-memory patch. The writer thread handles serialising and writing the patch outside this measurement.

```sh
cargo run --release -p loom-app -- --generate 300 --bench-autosave-copy
```

The command runs without a window or an audio device. It reports the result without applying a time limit.
