# Architecture Decision Records

An ADR records one important technical decision: the context, the choice, and its consequences. ADRs are short and never deleted. If a decision changes, write a new ADR that supersedes the old one.

Why we use them: contributors (human and AI) need to know *why* the code looks the way it does, not just *what* it does. Without a record, old debates restart and good reasons get lost.

## What belongs in an ADR

An ADR holds **one decision**: the problem, the options considered, what we chose, and what follows from it. A page or two is enough.

It does not hold specifications (module ports, parameters, data formats in detail), worked examples, use cases, or task lists. Those change as the work goes on, so they live in [design specifications](../design/README.md), [use cases](../product/personas-and-use-cases.md), [requirements](../product/requirements.md), and issues. The ADR links to them.

A useful test: if a detail could change without changing the decision, it belongs in a specification.

## Editing rules

- **Proposed:** edit freely while it is discussed, on its own branch. A Proposed ADR does not merge into `main`. Decide it first: it merges with a decided status (Accepted, Deferred, or Rejected), or as Superseded by an ADR that merges with it. A Rejected ADR still merges, so the choice and its reasons stay on record. CI enforces this: `mise run adr-status` (`scripts/check-adr-status.sh`) fails while any ADR is Proposed, or its status line holds anything but one allowed value, or the index row disagrees with it.
- **Accepted:** frozen. Only fix typos and broken links, or update the status line. To change the decision, write a new ADR and mark the old one "Superseded by ADR-NNNN".

Why merge only decided ADRs: a Proposed ADR on `main` looks like a decision but is not one, so work that depends on it waits with no clear owner. Several features were blocked this way. Keeping it on its branch, with its pull request open, keeps the open question visible until someone decides it.

Why freeze accepted ADRs: an ADR is a record of why things are the way they are. If it is rewritten after the fact, it can no longer explain the history it was written to explain.

## Status values

- **Proposed**: under discussion.
- **Accepted**: in effect.
- **Deferred**: valid, but only applies to work we have postponed.
- **Superseded by ADR-NNNN**: replaced.
- **Rejected**: considered and declined (kept for the record).

The status line holds one of these values and nothing else. The date line is the day the ADR got its current status: the day it was accepted, or deferred, superseded, or rejected. Why: a reader, or a script, can then read both at a glance, and the reasons stay in the ADR's own sections.

## Index

| ADR | Title | Status |
|---|---|---|
| [0000](0000-template.md) | Template | n/a |
| [0001](0001-lock-free-control-audio-boundary.md) | Lock-free boundary between control and audio threads | Accepted |
| [0002](0002-graph-execution-and-feedback.md) | Graph execution model and sample-accurate feedback | Accepted |
| [0003](0003-single-worklet-web-runtime.md) | One WASM instance per patch in a single AudioWorklet | Deferred |
| [0004](0004-export-as-runtime-plus-patch-data.md) | Export plugins as a prebuilt runtime plus patch data | Deferred |
| [0005](0005-rust-end-to-end.md) | Rust end to end | Accepted |
| [0006](0006-dependency-and-licence-policy.md) | Dependency and licence policy | Accepted |
| [0007](0007-polymorphic-signal-model.md) | Polymorphic signal model | Accepted |
| [0008](0008-patch-format-ron.md) | Patch format in RON | Accepted |
| [0009](0009-ui-stack-iced.md) | UI stack: Iced (canvas decided in ADR-0010) | Accepted |
| [0010](0010-own-patch-canvas.md) | Our own patch canvas, built as an Iced widget | Accepted |
| [0011](0011-everything-is-a-module.md) | Everything is a module | Accepted |
| [0012](0012-arrangement-from-phase-driven-primitives.md) | Arrangement from phase-driven primitives | Accepted |
| [0013](0013-modules-carry-their-own-panel.md) | Modules carry their own panel | Accepted |
| [0014](0014-modulation-without-cables.md) | Modulation without cables | Accepted |
| [0015](0015-one-transport-per-patch.md) | One transport per patch, read by modules without cables | Accepted |
| [0016](0016-ports-and-parameters-carry-their-own-text.md) | Ports and parameters carry a label and a description | Accepted |
| [0017](0017-primitives-traits-and-abstractions.md) | A module library of primitives, traits, and open abstractions | Accepted |
| [0018](0018-clean-room-module-development.md) | Clean-room module development | Accepted |
| [0019](0019-output-safety-stage.md) | A safety stage on the final output | Accepted |
| [0020](0020-inferred-channel-count.md) | Channel count inferred per cable, with per-channel copies | Accepted |
| [0021](0021-plain-data-view-models-and-keyed-identity.md) | Plain-data view models and keyed widget identity | Accepted |
| [0022](0022-copies-take-state-from-the-running-instance.md) | A new channel copy takes its state from the running instance | Accepted |
| [0023](0023-patch-licences-spdx-and-creative-commons.md) | Patch licences as SPDX expressions, chosen through Creative Commons questions | Accepted |
| [0024](0024-widgets-follow-the-kind-of-value.md) | Widgets follow the kind of value, and structured data has shared kinds | Accepted |
| [0025](0025-one-design-language-and-derived-panels.md) | One design language, and panels derived from the declaration | Accepted |
| [0026](0026-browser-demo-feasibility.md) | A browser demo waits for a spike; pre-rendered audio examples come first | Accepted |
| [0027](0027-prebuilt-release-downloads.md) | Prebuilt release downloads from a tagged GitHub Actions build | Accepted |
| [0028](0028-app-accessibility-waits-for-iced.md) | App accessibility waits for AccessKit support in Iced | Deferred |
| [0029](0029-sub-patches-stored-as-definitions-and-flattened.md) | Sub-patches are stored as definitions and flattened before compiling | Accepted |
| [0030](0030-module-data-in-typed-slots.md) | Module data lives in typed slots, with audio in a sidecar folder | Accepted |
