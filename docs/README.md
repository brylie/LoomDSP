# LoomDSP Documentation

This folder holds the guides, product requirements, and architecture decisions for LoomDSP.

Status: **Phase 1, early** (2026-10-14). Requirements are a draft and open for discussion.

## Guides

| Document | For | Purpose |
|---|---|---|
| [Getting started](guide/getting-started.md) | Musicians and testers | Run the desktop app, play a patch, and patch while it plays |
| [Architecture](dev/architecture.md) | Developers | How the crates, threads, compiler, and canvas fit together, and where to start |
| [Contributing](../CONTRIBUTING.md) | Developers | Build, test, hooks, CI, and pull requests |
| [Module help pages](modules/README.md) | Everyone | One page per built-in module: ports, parameters, ranges, and notes |
| [Learn](learn/README.md) | Everyone | The theory behind LoomDSP: phase, cycles, triggers, and why things work the way they do |

## Kinds of documents

Each kind of document has one job, so readers know what to trust and writers know where things go.

| Kind | Where | Holds | Changes |
|---|---|---|---|
| **Decision (ADR)** | [`adr/`](adr/README.md) | One decision: context, options, the choice, consequences. | Frozen once accepted. A changed decision gets a new ADR that supersedes the old one. |
| **Specification** | [`design/`](design/README.md) | How a feature works: behaviour, rules, acceptance criteria, out of scope, open points. | Written before the code ([spec first](design/README.md#spec-first)). Living: updated with the code it describes. |
| **Product** | [`product/`](product/vision.md) | Vision, personas and use cases, requirements, roadmap, open questions. | Living. |
| **Guide** | `guide/`, `dev/` | How to use LoomDSP, or how to work on it. | Living. |
| **Explainer** | [`learn/`](learn/README.md) | The theory behind a feature: what the signal is, why the obvious approach fails, how other modular systems do it. | Living: updated with the behaviour it explains. |
| **Benchmark** | [`benchmarks/`](benchmarks/) | Measurements on a stated machine and date. | Add new results; keep old ones for comparison. |
| **Research** | [`research/`](research/2026-10-prior-art-survey.md) | What we learned from other products and sources, on a stated date. Non-normative: evidence, not decisions. | Frozen. Write a new snapshot instead of updating an old one. |

Why decisions are kept apart from specifications: a decision record explains *why* the code is the way it is, and stays useful only if it does not move. Details such as ports, parameters, and examples change as features are built. Keeping them in an ADR would mean either editing a "frozen" record or writing a new ADR for every small change.

## Product

| Document | Purpose |
|---|---|
| [Vision](product/vision.md) | Why LoomDSP exists, goals, non-goals, and success measures |
| [Personas and use cases](product/personas-and-use-cases.md) | Who we build for and what they need to do |
| [Functional requirements](product/requirements.md) | What the system must do, with IDs and priorities |
| [Non-functional requirements](product/non-functional-requirements.md) | Performance, real-time safety, portability, quality |
| [Roadmap](product/roadmap.md) | Phases, scope per phase, and exit criteria |
| [Open questions](product/open-questions.md) | Unresolved decisions and known risks |
| [Module catalogue](product/module-catalogue.md) | Every planned module type, trait, and role, with stable IDs, phases, and requirements |
| [Glossary](product/glossary.md) | Shared vocabulary |

## Architecture

| Document | Purpose |
|---|---|
| [ADR index](adr/README.md) | Architecture Decision Records and how to write them |
| [Design specifications](design/README.md) | Living specifications of features, linked to their ADRs |

## Research

| Document | Purpose |
|---|---|
| [Prior-art survey, 2026-10](research/2026-10-prior-art-survey.md) | Modules in 15 modular environments, as evidence for the module catalogue |

## Benchmarks

| Document | Purpose |
|---|---|
| [Phase 0 engine](benchmarks/p0-engine.md) | Feedback loop cost, example patch cost, live and offline stress results |
| [Phase 0 UI spikes](benchmarks/p0-ui-spikes.md) | Iced + iced_nodegraph and egui-snarl, measured on the reference laptop |
| [Phase 1 canvas](benchmarks/p1-canvas.md) | Our own canvas: how to measure it, and the results |
| [Phase 1 modulation](benchmarks/p1-modulation.md) | Cost of a modulated filter cutoff, and of 1 to 16 assignments on one parameter |

## How to read requirement IDs

Each requirement has an ID such as `FR-GRAPH-03` or `NFR-RT-01`.

- `FR` means functional requirement. `NFR` means non-functional requirement.
- The middle part is the area (for example `GRAPH`, `UI`, `WAM`).
- IDs are stable. If a requirement is dropped, mark it **Withdrawn** instead of reusing the ID. This keeps links from issues and commits valid.

Priorities use MoSCoW:

- **Must**: the phase fails without it.
- **Should**: important, but the phase can ship without it.
- **Could**: nice to have if time allows.
- **Won't (now)**: explicitly out of scope for this version.
