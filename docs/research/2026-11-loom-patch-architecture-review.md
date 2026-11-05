# Architecture Review: `loom-patch` before sub-patches

- **Kind:** Research snapshot. Non-normative.
- **Date:** 2026-11-04, on `main` at a671e30 (after #611).
- **Follows:** none. This is the first architecture snapshot. The external review of November 2026 was filed straight into issues #616 to #630, and its lessons are in `.claude/skills/review-architecture/references/lenses.md`.
- **Used by:** #651 (finding 1) and #652 (finding 2), filed 2026-11-05. The "Already filed" evidence for #617, #621, #526, and #441 is not yet on those issues.

This review reads `crates/loom-patch` and the seams it shares with `loom-core`, `loom-app`, and `loom-cli`, with two questions from the maintainer: does the crate still respect the portable-crate rule (AGENTS.md hard rule 6), and does anything in it belong somewhere else before sub-patches (#51, ADR-0031) start. Do not update this snapshot later; write a new one after the fixes, and read its "Deliberate" and "Already filed" sections first.

## Method

- Ran `python3 .claude/skills/review-architecture/scripts/measure.py crates/loom-patch` at a671e30: 2,466 Rust lines in two source files (`lib.rs` 854, `metadata.rs` 486) and three test files; one function over 80 lines (`load_into`, 119); no `unsafe`; eight filesystem or process calls; four transport-by-id sites; 16 commits in the last 90 days.
- Read all of `crates/loom-patch/src/lib.rs`, `metadata.rs`, `tests.rs`, `metadata/tests.rs`, and `tests/malformed.rs`.
- Read every caller outside the crate: `crates/loom-app/src/boot.rs` (`session`, the restore path), `main.rs` (`current_patch`), `clipboard.rs` (`Group::from_patch`, `Group::to_text`), `recovery.rs` (`write_atomic`), and `crates/loom-cli/src/main.rs` (`load_engine`); and the loom-core seams the crate uses: `graph.rs` (`UnknownModule`, `Node`), `registry.rs` (`ModuleKind::parse`), `controller/modules.rs` (`transport_module`), `descriptor.rs` (`ModuleRole`, `saved_values`, `load_range`).
- Read AGENTS.md, `docs/dev/architecture.md`, ADR-0008, ADR-0023, ADR-0031, and the specs `sub-patches.md`, `files.md`, `patch-metadata.md`, `autosave.md`, `transient-params.md`.
- Searched the backlog: every open `chore` issue, and open and closed issues for each candidate before calling it new.

**Limits.** Nothing was built, run, or benchmarked. The wasm32 build is CI's `mise run wasm`; the claim that `std::fs` errors and `std::process::id` panics on `wasm32-unknown-unknown` comes from Rust's standard library, not from a run here. `loom-canvas`, `loom-host-desktop`, and `loom-modules` were read only at their `loom_patch` call sites.

Priority means: **High** a bug a user can hear or see, a hard-rule break, or a hazard that loses data silently; **Medium** a cost paid on every change in an area that is changing now; **Low** a cost in a quiet area, or cleanup.

## Summary

`loom-patch` is small and its shape is right. The file types are the schema (ADR-0008), everything in it is plain data, the one apply path (`load_into`) is well tested including a fuzz harness that runs on stable, and the crate has no `unsafe`, no threads, and nothing on the audio thread. It respects the portable-crate rule in the build and breaks it in the letter: `Patch::load` and `Patch::save` call `std::fs` and `std::process::id`, which is filed as #617, and `docs/dev/architecture.md:37` states both halves of the contradiction in one row ("Loads, saves, and migrates patch files" under "No threads, files, or audio APIs"). Three more of last month's findings land in this crate and are filed (#621, #623, #627).

What is new concerns sub-patches. The crate holds one conversion from file names to a running graph, written straight into an empty `Controller` with the file's IDs as the flat `ModuleId`s, and the clipboard holds a second as plain data for renumbering on paste. The flattener ADR-0031 asks for is a third, of the clipboard's shape, and the ADR has not said which crate owns it. Do first: settle #617 and the flattener's crate together, since the library slice adds file reads; extract the resolved-module form so `load_into`, paste, and flattening share one; and give module labels a home before slice 1 (#526), because the sub-patch spec names ports by `label`, and today a label on a known module is dropped on save.

## What to preserve

- **The file types are the schema, and loom-core stays serde-free.** `PatchModule` stores parameters and ports by name, `PatchPolarity` mirrors `Polarity`, and `loom-core/Cargo.toml` has no serde dependency. A change to the engine's enums cannot change the file by accident. Keep the mirror; it is the price of the boundary.
- **One apply path with named lookups.** `load_into` reaches the controller only through `add_module_with_id`, `set_param_named`, `connect_named`, `add_assignment_named`, and `map_midi_named`, so the loader never knows an index. Flattening should go through the same door.
- **Lenient metadata, strict structure.** Metadata never stops a patch from opening (`metadata.rs:1-7`, patch-metadata.md rule 1), while a duplicate MIDI mapping, two Transports, or an unknown parameter is an error with a name in it (`lib.rs:261-285`). Keep that split.
- **Canonical output with one deliberate exception.** Modules, cables, and MIDI mappings are sorted; modulation keeps assignment order because float sums depend on it (`lib.rs:407-417`). The comment says why.
- **Tests in their own files, and the malformed harness.** `tests/malformed.rs` loads saved fuzz findings, edge cases, and seeded mutations of every example patch on stable, in every CI job.
- **`Prepared` and `Matcher` for the browser.** Each row's lowercase text and licence answers are computed once (`metadata.rs:423-483`), so typing in the search does not parse SPDX again.

## Table of findings

| # | Crate | Lens | Finding | Priority | Size |
|---|---|---|---|---|---|
| 1 | loom-patch, loom-app | Boundaries, shape | Two name-to-module conversions (loom-patch's bound to an empty `Controller`, the clipboard's as plain data), and no decided home for the flattener ADR-0031 needs | Medium | M |
| 2 | loom-patch | Duplication | `Metadata` repeats `Patch`'s five metadata fields and serde attributes, copied by hand in two methods; sub-patch definitions would be a third copy | Low | S |

The findings that are already filed, with new evidence, follow the two above under "Already filed". Two of them (#617 and #526) change order because of sub-patches.

## Findings

### 1. Two name-to-module conversions, and no home for the flattener

- **Where:** `crates/loom-patch/src/lib.rs:504-622` (`load_into`), `:627-689` (`load_ranges`), `:738-800` (`from_graph`), `:805-851` (`ranges_to_save`, `port_name`, `input_name`, `param_name`); `crates/loom-app/src/clipboard.rs:225-280` (`Group::from_patch`), `:355-395` (`Group::to_text`); positions and sizes filled by the app in `crates/loom-app/src/main.rs:834-846` and `boot.rs:195-205`.
- **The problem.** The crate converts a `PatchModule` (names) into a running module by writing straight into a `Controller`, and assumes the controller is empty and that the file's ids are the flat `ModuleId`s (`lib.rs:523, 529`). The clipboard needs the same conversion but with new ids, so it does it again as plain data (`Member`: descriptor or placeholder, params in index order after `saved_values`, ranges via `load_range`, position, size) at `clipboard.rs:235-280`, and writes the reverse at `:355-395`, repeating `from_graph`'s module branch and `ranges_to_save` inline. The two have not drifted in effect: both skip transient values (loom-patch by `is_transient` at `lib.rs:536-541` and `:760`; the clipboard by `saved_values` at copy time, `clipboard.rs:142`). They have drifted in shape: layout is a `Member` field on one side and an app-side table patched in after `from_graph` on the other.
- **The consequence.** ADR-0031 and sub-patches.md ("Flattening") need a third conversion: expand each definition into the flat graph before every compile, with a path-keyed id table that keeps a module's id across edits. That is the clipboard's shape (resolve against the registry, then apply with assigned ids), not `load_into`'s (resolve and apply in one pass into an empty controller). If slice 1 of #51 starts by adding a `subpatches` loop inside `load_into`, the flattener inherits "once, into an empty controller" and must be rewritten for re-flattening on edit; and a fourth copy of the conversion appears next to the clipboard's. ADR-0031 also leaves the crate open ("`loom-patch` or `loom-core`"), so the first slice would decide it by accident. Control thread only; nothing here touches the audio path.
- **Evidence:** read. The ADR text is at `docs/adr/0031-sub-patches-stored-as-definitions-and-flattened.md`, Consequences, "The CLI and the web runtime flatten too".
- **Direction.** Decide the crate first, and write the answer into ADR-0031's spec (`sub-patches.md`, "Code") rather than a new ADR, since the ADR already allows either. Recommendation: `loom-patch`. It owns the file types and the only apply path, it already depends on `loom-core`, and `loom-core`'s `Graph` stays the flat, serde-free engine graph (ADR-0002, ADR-0034). Then move the clipboard's resolve step into `loom-patch` as a plain-data resolved module (descriptor or `UnknownModule`, params in index order after `saved_values`, ranges from `load_range`, position, size, label), so that `load_into` becomes resolve plus apply into a controller with an id map, `Group::from_patch` becomes resolve plus renumber, and a definition flattens by the same resolve plus apply with a path-keyed map. `to_text` and `from_graph` then share one `PatchModule` writer that takes layout as an argument instead of patching it in afterwards. No behaviour change: the example patches, the clipboard tests, and the malformed harness stay as they are. Relates to #441 (name resolution for known and unknown nodes is one part of this), #617, and #621. Size M.

### 2. `Metadata` repeats five fields of `Patch`

- **Where:** `crates/loom-patch/src/lib.rs:52-74` and `crates/loom-patch/src/metadata.rs:25-45` (the same five fields with the same serde attributes, both `#[serde(rename = "Patch")]`); `lib.rs:305-329` (`metadata` and `set_metadata` copy field by field).
- **The problem.** `Metadata` exists so the browser can read a file's head without its modules (`metadata.rs:18-21`), which is a good reason. The cost is that a new metadata field must be added in four places, and only one fixture test (`metadata/tests.rs:55-62`) checks that the two structs agree.
- **The consequence.** sub-patches.md says a library item carries "its patch metadata (authors, tags, licence)" of its own, so a `SubPatch` definition is a third copy of the five fields and a third `metadata()` pair. A field added to one and not the others loads as its default silently; a test would catch the `Patch` and `Metadata` pair but not the new one.
- **Evidence:** read.
- **Direction.** Not now. `#[serde(flatten)]` is not an option: serde serialises a flattened struct as a map, which RON writes as `{ ... }`, and that would change the canonical `Patch(...)` form ADR-0008 asks for. When the format changes for sub-patches anyway (ADR-0031, "Ports change the patch format"), nest `metadata: Metadata` as one field of both `Patch` and `SubPatch`, give the browser a head struct with that one field, and update the example patches in the same pull request. Size S, as part of slice 1. Until then, leave it.

### Lenses with nothing to report

- **Real-time safety:** nothing in `loom-patch` is reachable from `Engine::process`; the crate speaks only to `Controller`.
- **Determinism:** parameters and ranges are `BTreeMap`s, the saved form is sorted, and the one unsorted list (modulation) is unsorted on purpose with the reason in the comment.
- **`unsafe`:** none (measured).
- **Licences:** serde, ron, spdx, thiserror, all covered by `cargo deny`.
- **Block-size invariance:** not applicable.
- **Performance:** no audio path. The browser's `Prepared` rows avoid re-parsing licences per keystroke.
- **Testability:** tests are in their own files; `load_into` needs a built engine, which every test builds in two lines; save, the read-only refusal, and the symlink case are tested.

## Deliberate

Things that look like debt and are decisions. None has grown in cost except where said.

- **User-facing prose in a portable crate.** `LicenceFilter::label`, the licence summaries, `TagError`'s `Display`, and the chooser's table live in `metadata.rs`. The module header (`metadata.rs:4-7`) and patch-metadata.md rule 8 decide it: the app, the CLI, and a future library browser must tidy and match the same way. Plain data is allowed in engine crates (the rule forbids Iced, cpal, and the app). If text is ever localised, the strings move and the rules stay.
- **`PatchPolarity` mirrors `Polarity`; `PatchModule` stores by name.** `loom-core` has no serde dependency, and ADR-0008 makes the patch types the schema. The mirror is the boundary's price, as `WidgetViewKind` is for the canvas.
- **A `kind` without `@version` is refused on load**, although `ModuleKind::parse` accepts one (`registry.rs:17, 30`) and `Controller::add_module` documents both forms. `unknown_module` checks for `@` first (`lib.rs:444-446`), and `unknown_version_and_malformed_kind` (`tests.rs:296-318`) asserts the refusal since #440. A file always names a version, so a placeholder can keep it.
- **Modulation is not sorted by `canonicalise`.** `lib.rs:407-411`: assignments on one parameter are summed in id order, and float sums depend on it.
- **Positions and sizes are not written by `from_graph`.** `Graph` has no layout (ADR-0010: the app owns positions), so `current_patch` (`main.rs:834-846`) and `session` (`boot.rs:195-205`) fill them. Fine as a boundary; finding 1 says the conversion should take layout as an argument so both sides stop patching it in.
- **`load_into` leaves a half-loaded controller on error.** The doc says the controller should be empty, and every caller builds a fresh one and drops it on failure (`boot.rs:181-183`, `cli/main.rs:212-214`, every test). No rollback is needed while that holds.
- **Format version stays at 1.** AGENTS.md: migrations start with the first release. `migrate` is the one place a step will go.

## Already filed

With the new evidence this review adds. Two change order because of sub-patches.

- **#617, `loom-patch` reads and writes files and calls `std::process::id`.** Confirmed at `lib.rs:340` (`load`) and `:376-402` (`save`). New evidence: (a) `docs/dev/architecture.md:37` states the contradiction itself: the job is "Loads, saves, and migrates patch files" and the rule is "Same as `loom-core`" (no files), while `loom-analysis` two rows down is "Desktop only: it reads WAV files", so the project already uses "reads files" as the dividing line; (b) the app has a second temp-file-and-rename writer, `recovery::write_atomic` (`crates/loom-app/src/recovery.rs:446-451`), which differs from `Patch::save` (no `create_new`, no pid in the temp name, no read-only refusal, no permission copy): harmless today because the recovery folder is locked per instance (autosave.md), but if option 2 of #617 is chosen, `write_atomic` is the shared home, not a third copy; (c) sub-patches slice 4 adds library reads and writes ("Save to library", "On load, if the library file differs"), and the library folder is a preference (`preferences.md`), so the desktop side will need the file layer anyway. Recommendation: settle #617 before slice 4, with option 2 (the file layer beside `write_atomic`, `Patch` keeping `from_ron` and `to_ron`), and add the grep check it asks for so rule 6 stops relying on a build that cannot fail.
- **#621, `load_into` reports warnings as strings matched by substring.** Confirmed: `lib.rs:577` formats the unknown-module note, `boot.rs:170` and `cli/main.rs:218` find it with `contains("not in this version:")`. New evidence: sub-patches.md adds at least four more status messages that would travel the same channel ("Updated from the library: …", "its Transport was left out; connect its clock ports", "is now a copy", and the refused self-containing definition), and the app will need to tell them apart. Do #621 before slice 1.
- **#623, Transport found by its id string.** Confirmed at `lib.rs:34, 513, 579-580`; `Controller::transport_module` (`controller/modules.rs:12-17`) does the same compare on the graph side. Nothing to add.
- **#627, temp directories in tests.** `tests.rs:397` and `:423` are two of the roughly twenty copies (`temp_dir()` appears 36 times in 20 files at a671e30). Nothing to add.
- **#526, Name modules: show and edit the label the format stores.** Filed as a UI feature (should, M2). New evidence that moves it ahead of #51 slice 1: `from_graph` writes `label: None` for every known module (`lib.rs:766`), `Group::to_text` does the same (`clipboard.rs:371`), and `Node` has no label field (`graph.rs:136-142`), so today any label on a known module is dropped on the next save; only placeholders keep theirs through `UnknownModule.label`. sub-patches.md's file example names a definition's ports by `label: Some("pitch")` on `loom.struct.input@1` modules, so slice 1 cannot save a definition's ports until a label has a home. Decide where it lives (a `Node` field in `loom-core`, like `params`, or an app-side table like positions; the first is simpler because the clipboard and undo already snapshot `Node`), and do that part of #526 first.
- **#441, centralise known and unknown module name resolution.** The port-name helpers at `lib.rs:817-851` and the clipboard's at `clipboard.rs:287-300` are the cases it names. Finding 1 absorbs it: the resolved form is where the one implementation goes.

## Stale or wrong

No pasted review was checked. Nothing in the measured map turned out wrong when read, except that the script's "Transport found by id string" count of four includes the `use` line (`lib.rs:34`), so the real sites are three.

## Limits

See Method. In one line: measured with the script at a671e30, read the whole crate and every caller, built and ran nothing, and did not read the other crates beyond their `loom_patch` call sites.
