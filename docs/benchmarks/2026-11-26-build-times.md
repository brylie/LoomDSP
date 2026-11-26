# Build times: test binaries and dependency debug info

On 2026-11-26, building the integration tests of a crate as one binary cut the rebuild of `loom-modules` and its tests after an edit from **about 6.9 s to about 2.4 s**. Building third-party crates without debug info made the `debug` folder about 0.9 GB smaller, but its effect on build time was within the noise. ([Where tests live](../../CONTRIBUTING.md#where-tests-live))

## What was measured

The debug build ran on macOS 27.0.1, an Apple M1 Pro with 10 cores and 32 GB of memory, with Cargo 1.100 and `jobs = 5` from the worktree's Cargo config. Other agents were building on the same machine, so the load average was 5 to 13 during the runs. Each run used the same script, with the compile cache's wrapper variable set to empty (`RUSTC_WRAPPER=`), so every build compiled for real:

1. `cargo clean`, then build every test binary: `cargo nextest run --workspace --no-run`.
2. Three times: add one statement to the body of `loom_modules::registry`, then build every test binary of the workspace again.
3. Three times: the same edit, then build only the test binaries of `loom-modules`: `cargo nextest run -p loom-modules --no-run`.

The first run of steps 2 and 3 is left out as a warm-up. The first `-p loom-modules` build after a workspace build took 16 to 39 s in every variant. The likely cause is dependencies built again with the features of one crate rather than the workspace's, but this was not checked. Running the tests was not timed.

## Results

| Variant | Clean build | Edit, workspace tests | Edit, `loom-modules` tests | `debug` folder | Test binaries |
|---|---:|---:|---:|---:|---:|
| A. Before (the parent commit, #896) | 137 s | 21.0, 20.6 s | 7.3, 6.5 s | 7.0 GB | 79 |
| B. A, with third-party crates built without debug info | 124 s | 20.6, 18.8 s | 6.0, 7.2 s | 6.1 GB | 79 |
| C. B, with the tests of `loom-modules` in one binary | 127 s | 16.3, 16.5 s | 2.6, 2.1 s | 5.7 GB | 42 |
| D. C, with the tests of `loom-core`, `loom-app`, `loom-cli`, and `loom-ui` in one binary each | 134 s | 24.1, 23.0 s | 2.3, 2.7 s | 5.7 GB | 29 |

Load averages over one minute: A 5 to 8, B 6 to 11, C 9 to 11, D 10 to 19.

## Reading the results

- **One binary per crate.** The rebuild of `loom-modules` and its tests fell by about 65% in C and stayed there in D, under the highest load of all four runs. After an edit, each test binary links the whole crate graph again: `loom-modules` had 41 of them and now has 4 (the merged one, `no_alloc`, `polyphony`, and the unit tests).
- **The workspace rebuild is not settled.** It fell to 16.5 s in C and rose to 23.5 s in D, whose load was the highest. Merging more crates removes link steps and adds none, so D's number most likely shows the load, not the change. A run on an idle machine would settle it.
- **Dependency debug info.** The clean build fell from 137 s to 124 s, but each clean build ran once, so this is within the noise. On macOS, Cargo's dev builds leave debug info in the object files and the linker does not copy it into the binaries, so only rustc's work shrinks. On Linux the linker copies it into every test binary, so CI may gain more; that was not measured.
