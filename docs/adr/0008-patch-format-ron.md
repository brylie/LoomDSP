# ADR-0008: Patch format in RON

- **Status:** Accepted
- **Date:** 2026-10-13
- **Related requirements:** FR-FILE-01 to 07
- **Specification:** [Open, Save, and Save As](../design/files.md), [Autosave and crash recovery](../design/autosave.md)

## Context

Patches must be human-readable, work well with git, and survive upgrades. The editor and engine share Rust types (ADR-0005), so the format should map cleanly to those types.

## Options considered

1. **JSON.** Universal tooling. But Rust enums need a tagging convention, there are no comments, and tuples and structs lose their names.
2. **RON (Rusty Object Notation).** Supports Serde's full data model: enums, structs, tuples, options. Allows comments and trailing commas. Fewer tools outside Rust.
3. **Binary (for example bincode or postcard).** Small and fast, but not readable and poor in git.

## Decision

Use RON as the canonical patch format.

- Top-level structure carries a format version (for example `version: 1`).
- Modules are referenced by stable ID and version, for example `"loom.osc.sine@1"`.
- Output is canonical: stable field order, collections sorted by ID, fixed pretty-print settings. This keeps git diffs small.
- Each format version change comes with a migration function and a test that loads an old fixture patch.
- An optional JSON export (FR-FILE-07) can be added later for tools outside Rust.
- Exported plugins may embed the patch in a compact binary form for faster loading, generated from the RON source.

## Consequences

- The Rust types are the schema. A change to a type is a change to the format, so format changes need review.
- Readable patches are easy for both people and AI tools to inspect and edit.
- Third-party tools in other languages need a RON parser. Parsers exist for some languages, and the JSON export covers the rest.
